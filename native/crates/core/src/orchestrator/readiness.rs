//! Readiness probes: how the core decides a task is actually up.
//!
//! Six kinds, unchanged from `src/main/services/readiness/`:
//!
//! * `none`  — nothing is verified. Reported as [`ReadinessOutcome::Unverified`], never as
//!   "ready", so MCP cannot claim `readinessVerified: true` for an unprobed task.
//! * `port`  — the task's process group is listening on the port (lsof + ancestor walk).
//! * `log`   — a regex matches ANSI-stripped output.
//! * `http`  — a GET returns the expected status (default 2xx/3xx). TLS is only unverified
//!   for loopback targets (self-signed dev certs); never globally.
//! * `exit`  — one-shot tasks: the process exited with the expected code.
//! * `delay` — a fixed wait.
//!
//! Every probe aborts early when the task exits, and the whole wait is bounded by the
//! caller's timeout (default 60 s from settings).

use neohub_domain::{ProcessState, ReadinessSignal};
use regex::RegexBuilder;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Default readiness timeout (mirrors `readiness_timeout_ms`).
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);
const PORT_POLL: Duration = Duration::from_millis(500);
const LOG_POLL: Duration = Duration::from_millis(250);
const HTTP_POLL: Duration = Duration::from_millis(700);
const HTTP_REQUEST_TIMEOUT: Duration = Duration::from_millis(2_500);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadinessOutcome {
    /// The probe fired.
    Ready,
    /// `none`: nothing was verified. Callers must not report this as ready.
    Unverified,
    /// The probe failed or the task died first.
    Failed(String),
    /// The caller's timeout elapsed.
    TimedOut,
}

/// What a probe needs to know about the running task. Everything is a closure so the probe
/// stays testable without a runner.
pub struct ProbeContext {
    pub pid: u32,
    pub pgid: i32,
    pub state: Arc<dyn Fn() -> ProcessState + Send + Sync>,
    /// Retained raw output (ANSI included); the log probe strips escapes itself.
    pub raw_logs: Arc<dyn Fn() -> String + Send + Sync>,
    pub exit_code: Arc<dyn Fn() -> Option<i32> + Send + Sync>,
}

impl ProbeContext {
    fn is_dead(&self) -> bool {
        !(self.state)().is_live()
    }
}

/// Wait for a task's readiness signal.
pub async fn wait_for_readiness(
    signal: &ReadinessSignal,
    ctx: &ProbeContext,
    timeout: Duration,
) -> ReadinessOutcome {
    if matches!(signal, ReadinessSignal::None) {
        return ReadinessOutcome::Unverified;
    }
    let deadline = Instant::now() + timeout;
    match signal {
        ReadinessSignal::None => ReadinessOutcome::Unverified,
        ReadinessSignal::Delay { ms } => wait_delay(*ms, ctx, deadline).await,
        ReadinessSignal::Exit { code } => wait_exit(code.unwrap_or(0), ctx, deadline).await,
        ReadinessSignal::Log { regex, flags } => {
            wait_log(regex, flags.as_deref(), ctx, deadline).await
        }
        ReadinessSignal::Port { port, host } => {
            wait_port(*port, host.as_deref(), ctx, deadline).await
        }
        ReadinessSignal::Http { url, status } => wait_http(url, *status, ctx, deadline).await,
    }
}

async fn wait_delay(ms: u64, ctx: &ProbeContext, deadline: Instant) -> ReadinessOutcome {
    let target = Instant::now() + Duration::from_millis(ms);
    loop {
        if ctx.is_dead() {
            return ReadinessOutcome::Failed("task exited before the delay elapsed".into());
        }
        if Instant::now() >= target {
            return ReadinessOutcome::Ready;
        }
        if Instant::now() >= deadline {
            return ReadinessOutcome::TimedOut;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn wait_exit(expected: i32, ctx: &ProbeContext, deadline: Instant) -> ReadinessOutcome {
    loop {
        if !(ctx.state)().is_live() {
            let code = (ctx.exit_code)();
            return if code == Some(expected) {
                ReadinessOutcome::Ready
            } else {
                ReadinessOutcome::Failed(format!("exited with {code:?}, expected {expected}"))
            };
        }
        if Instant::now() >= deadline {
            return ReadinessOutcome::TimedOut;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
}

async fn wait_log(
    pattern: &str,
    flags: Option<&str>,
    ctx: &ProbeContext,
    deadline: Instant,
) -> ReadinessOutcome {
    // JS flags are not Rust regex syntax; the only flag with a real equivalent is `i`.
    let case_insensitive = flags.map(|f| f.contains('i')).unwrap_or(false);
    if let Some(unsupported) = flags.filter(|f| f.chars().any(|c| c != 'i')) {
        return ReadinessOutcome::Failed(format!(
            "regex flags \"{unsupported}\" are not supported (only \"i\" is)"
        ));
    }
    let regex = match RegexBuilder::new(pattern)
        .case_insensitive(case_insensitive)
        .build()
    {
        Ok(regex) => regex,
        Err(err) => {
            return ReadinessOutcome::Failed(format!("invalid log regex: {err}"));
        }
    };
    loop {
        if regex.is_match(&strip_ansi(&(ctx.raw_logs)())) {
            return ReadinessOutcome::Ready;
        }
        if ctx.is_dead() {
            return ReadinessOutcome::Failed("task exited before the log pattern matched".into());
        }
        if Instant::now() >= deadline {
            return ReadinessOutcome::TimedOut;
        }
        tokio::time::sleep(LOG_POLL).await;
    }
}

async fn wait_port(
    port: u16,
    host: Option<&str>,
    ctx: &ProbeContext,
    deadline: Instant,
) -> ReadinessOutcome {
    let host = host.unwrap_or("127.0.0.1").to_string();
    loop {
        let pid = ctx.pid;
        let probe = tokio::task::spawn_blocking(move || {
            listening_pids_on_port(port)
                .map(|pids| {
                    pids.iter()
                        .any(|candidate| is_self_or_ancestor(pid, *candidate))
                })
                .unwrap_or(false)
        })
        .await
        .unwrap_or(false);
        if probe {
            return ReadinessOutcome::Ready;
        }
        if ctx.is_dead() {
            return ReadinessOutcome::Failed(format!(
                "task exited before it listened on {host}:{port}"
            ));
        }
        if Instant::now() >= deadline {
            return ReadinessOutcome::TimedOut;
        }
        tokio::time::sleep(PORT_POLL).await;
    }
}

async fn wait_http(
    url: &str,
    expected: Option<u16>,
    ctx: &ProbeContext,
    deadline: Instant,
) -> ReadinessOutcome {
    let parsed = match reqwest::Url::parse(url) {
        Ok(parsed) => parsed,
        Err(err) => return ReadinessOutcome::Failed(format!("invalid readiness URL: {err}")),
    };
    let loopback = matches!(
        parsed.host_str().map(|h| h.to_ascii_lowercase()).as_deref(),
        Some("127.0.0.1") | Some("localhost") | Some("[::1]") | Some("::1")
    );
    let client = match reqwest::Client::builder()
        // Self-signed dev certs are the norm on loopback; anything else is verified.
        .danger_accept_invalid_certs(loopback)
        .timeout(HTTP_REQUEST_TIMEOUT)
        .build()
    {
        Ok(client) => client,
        Err(err) => return ReadinessOutcome::Failed(format!("http client: {err}")),
    };

    loop {
        match client.get(parsed.clone()).send().await {
            Ok(response) => {
                let code = response.status().as_u16();
                let ok = match expected {
                    Some(want) => code == want,
                    None => (200..400).contains(&code),
                };
                if ok {
                    return ReadinessOutcome::Ready;
                }
            }
            Err(_) => { /* not serving yet (or refused) — keep polling */ }
        }
        if ctx.is_dead() {
            return ReadinessOutcome::Failed("task exited before the URL answered".into());
        }
        if Instant::now() >= deadline {
            return ReadinessOutcome::TimedOut;
        }
        tokio::time::sleep(HTTP_POLL).await;
    }
}

/// Pids with a LISTEN socket on `port` (`lsof -Fp`). `lsof` exits 1 when nothing matches.
fn listening_pids_on_port(port: u16) -> Option<Vec<i32>> {
    let output = std::process::Command::new("/usr/sbin/lsof")
        .args(["-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN", "-Fp"])
        .output()
        .ok()?;
    if !output.status.success() && output.status.code() != Some(1) {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Some(
        text.lines()
            .filter_map(|line| line.strip_prefix('p'))
            .filter_map(|pid| pid.trim().parse::<i32>().ok())
            .collect(),
    )
}

/// True when `candidate` is `parent` or a descendant of it (bounded walk).
fn is_self_or_ancestor(parent: u32, candidate: i32) -> bool {
    if candidate <= 0 {
        return false;
    }
    if candidate == parent as i32 {
        return true;
    }
    let mut current = candidate;
    for _ in 0..8 {
        let Some(ppid) = parent_of(current) else {
            return false;
        };
        if ppid <= 1 {
            return false;
        }
        if ppid == parent as i32 {
            return true;
        }
        current = ppid;
    }
    false
}

fn parent_of(pid: i32) -> Option<i32> {
    let output = std::process::Command::new("/bin/ps")
        .args(["-o", "ppid=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

/// Strip ANSI escapes so user regexes do not have to anticipate colour codes.
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.next() {
                Some('[') => {
                    // CSI: consume until a final byte in @..~
                    for next in chars.by_ref() {
                        if ('@'..='~').contains(&next) && !next.is_ascii_digit() {
                            break;
                        }
                    }
                }
                Some(']') => {
                    // OSC: consume until BEL or ST
                    while let Some(next) = chars.next() {
                        if next == '\u{7}' {
                            break;
                        }
                        if next == '\u{1b}' {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_with(state: ProcessState, logs: &'static str, exit: Option<i32>) -> ProbeContext {
        ProbeContext {
            pid: 1,
            pgid: 1,
            state: Arc::new(move || state),
            raw_logs: Arc::new(move || logs.to_string()),
            exit_code: Arc::new(move || exit),
        }
    }

    #[tokio::test]
    async fn none_is_reported_as_unverified_not_ready() {
        let ctx = ctx_with(ProcessState::Running, "", None);
        let outcome =
            wait_for_readiness(&ReadinessSignal::None, &ctx, Duration::from_millis(100)).await;
        assert_eq!(outcome, ReadinessOutcome::Unverified);
    }

    #[tokio::test]
    async fn a_delay_signal_fires_after_its_wait() {
        let ctx = ctx_with(ProcessState::Running, "", None);
        let started = Instant::now();
        let outcome = wait_for_readiness(
            &ReadinessSignal::Delay { ms: 120 },
            &ctx,
            Duration::from_secs(2),
        )
        .await;
        assert_eq!(outcome, ReadinessOutcome::Ready);
        assert!(started.elapsed() >= Duration::from_millis(100));
    }

    #[tokio::test]
    async fn exit_readiness_matches_the_expected_code() {
        let ok = ctx_with(ProcessState::Exited, "", Some(0));
        assert_eq!(
            wait_for_readiness(
                &ReadinessSignal::Exit { code: Some(0) },
                &ok,
                DEFAULT_TIMEOUT
            )
            .await,
            ReadinessOutcome::Ready
        );
        let bad = ctx_with(ProcessState::Crashed, "", Some(7));
        match wait_for_readiness(
            &ReadinessSignal::Exit { code: Some(0) },
            &bad,
            DEFAULT_TIMEOUT,
        )
        .await
        {
            ReadinessOutcome::Failed(message) => assert!(message.contains('7')),
            other => panic!("expected failure, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn log_readiness_ignores_ansi_sequences_and_honours_the_i_flag() {
        let ctx = ctx_with(
            ProcessState::Running,
            "\u{1b}[32mLISTENING\u{1b}[0m on 4319",
            None,
        );
        assert_eq!(
            wait_for_readiness(
                &ReadinessSignal::Log {
                    regex: "listening".into(),
                    flags: Some("i".into())
                },
                &ctx,
                Duration::from_secs(2)
            )
            .await,
            ReadinessOutcome::Ready
        );
        // Without the flag it must not match.
        let ctx = ctx_with(ProcessState::Running, "LISTENING", None);
        assert_eq!(
            wait_for_readiness(
                &ReadinessSignal::Log {
                    regex: "listening".into(),
                    flags: None
                },
                &ctx,
                Duration::from_millis(400)
            )
            .await,
            ReadinessOutcome::TimedOut
        );
    }

    #[tokio::test]
    async fn unsupported_regex_flags_and_bad_patterns_fail_loudly() {
        let ctx = ctx_with(ProcessState::Running, "", None);
        match wait_for_readiness(
            &ReadinessSignal::Log {
                regex: "x".into(),
                flags: Some("gi".into()),
            },
            &ctx,
            DEFAULT_TIMEOUT,
        )
        .await
        {
            ReadinessOutcome::Failed(message) => assert!(message.contains("not supported")),
            other => panic!("expected failure, got {other:?}"),
        }
        match wait_for_readiness(
            &ReadinessSignal::Log {
                regex: "[unclosed".into(),
                flags: None,
            },
            &ctx,
            DEFAULT_TIMEOUT,
        )
        .await
        {
            ReadinessOutcome::Failed(message) => assert!(message.contains("invalid log regex")),
            other => panic!("expected failure, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_dead_task_fails_the_probe_instead_of_waiting_for_the_timeout() {
        let ctx = ctx_with(ProcessState::Crashed, "", Some(1));
        let started = Instant::now();
        let outcome = wait_for_readiness(
            &ReadinessSignal::Delay { ms: 30_000 },
            &ctx,
            Duration::from_secs(30),
        )
        .await;
        assert!(matches!(outcome, ReadinessOutcome::Failed(_)));
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "must abort early"
        );
    }

    #[tokio::test]
    async fn an_invalid_url_fails_immediately() {
        let ctx = ctx_with(ProcessState::Running, "", None);
        match wait_for_readiness(
            &ReadinessSignal::Http {
                url: "not a url".into(),
                status: None,
            },
            &ctx,
            DEFAULT_TIMEOUT,
        )
        .await
        {
            ReadinessOutcome::Failed(message) => assert!(message.contains("invalid readiness URL")),
            other => panic!("expected failure, got {other:?}"),
        }
    }

    #[test]
    fn ansi_stripping_keeps_text_and_drops_sequences() {
        assert_eq!(strip_ansi("\u{1b}[31mred\u{1b}[0m plain"), "red plain");
        assert_eq!(strip_ansi("plain"), "plain");
        assert_eq!(strip_ansi("\u{1b}]0;title\u{7}after"), "after");
    }
}
