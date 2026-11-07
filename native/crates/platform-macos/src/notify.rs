//! Desktop notifications.
//!
//! **Why not `UNUserNotificationCenter`:** it requires a properly signed and (on macOS 13+)
//! notarized bundle, and refuses to deliver from an ad-hoc build — the notification simply never
//! appears, with no error the user can act on. `NSUserNotification` is deprecated. What works
//! everywhere, including an unsigned development build, is asking AppleScript to display the
//! notification: it goes through the system's own notification path, so Focus modes, the
//! notification centre and Do Not Disturb all apply as usual.
//!
//! The visible cost is the banner's source: it reads as the scripting host rather than NeoHub.
//! That is a fair trade for a notification that actually arrives; switching to
//! `UNUserNotificationCenter` later is a change inside this module only.

use std::process::{Command, Stdio};

/// One notification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notification {
    pub title: String,
    pub body: String,
}

impl Notification {
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Notification {
            title: title.into(),
            body: body.into(),
        }
    }
}

/// The AppleScript that displays `notification`.
///
/// Both strings are user-visible text that can contain quotes (an app named `Bob's api`), so they
/// are escaped for AppleScript string literals: backslash first, then the double quote.
pub fn script(notification: &Notification) -> String {
    format!(
        "display notification \"{}\" with title \"{}\"",
        escape(&notification.body),
        escape(&notification.title)
    )
}

fn escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for character in input.chars() {
        match character {
            // AppleScript has no escape for a newline inside a literal; a space keeps the
            // message readable instead of silently truncating it.
            '\r' | '\n' => out.push(' '),
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            other => out.push(other),
        }
    }
    out
}

/// Leave a notification on screen for the user.
///
/// Failure is reported, never fatal: a missing banner must not interrupt the app.
pub fn send(notification: &Notification) -> Result<(), String> {
    if notification.title.trim().is_empty() && notification.body.trim().is_empty() {
        return Err("empty notification".to_string());
    }
    let status = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script(notification))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| error.to_string())?;
    if status.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&status.stderr).into_owned())
    }
}

/// Build the notification for a task that left a running state, or `None` when the transition is
/// not one a user wants to be told about.
///
/// Only crashes and unexpected exits notify: a user-initiated stop is not news, and telling
/// someone about an action they just took is how notifications get switched off.
pub fn for_task_exit(
    app: &str,
    task: &str,
    running_before: bool,
    crashed: bool,
    user_stopped: bool,
) -> Option<Notification> {
    if !running_before || user_stopped {
        return None;
    }
    if crashed {
        return Some(Notification::new(
            format!("{app} 崩溃了"),
            format!("{task} 意外退出，详情见任务日志"),
        ));
    }
    None
}

/// Notification for a task that just became ready, or `None` when the user does not want it.
///
/// Ready notifications are only interesting when the window is *not* in front of the user — that
/// is the whole point of starting a stack from the menu bar. The caller decides that part; this
/// function only enforces the setting.
pub fn for_task_ready(app: &str, task: &str, enabled: bool) -> Option<Notification> {
    if !enabled {
        return None;
    }
    Some(Notification::new(
        format!("{app} 已就绪"),
        format!("{task} 已完成启动"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_script_quotes_both_strings() {
        let notification = Notification::new("NeoHub", "web 崩溃了");
        assert_eq!(
            script(&notification),
            "display notification \"web 崩溃了\" with title \"NeoHub\""
        );
    }

    #[test]
    fn quotes_and_backslashes_are_escaped() {
        // An app can be called `Bob's "api"`; an unescaped quote would end the literal and the
        // script would fail at runtime, silently losing the notification.
        let notification = Notification::new(r#"Bob's "api""#, r#"path C:\work\"#);
        let script = script(&notification);
        assert!(script.contains(r#"title "Bob's \"api\"""#), "{script}");
        assert!(script.contains(r#"path C:\\work\\"#), "{script}");
    }

    #[test]
    fn newlines_become_spaces_instead_of_breaking_the_literal() {
        let notification = Notification::new("title", "line one\nline two");
        let script = script(&notification);
        assert!(
            !script.contains('\n'),
            "a raw newline would end the statement"
        );
        assert!(script.contains("line one line two"));
    }

    #[test]
    fn an_empty_notification_is_refused_before_spawning() {
        let error = send(&Notification::new("   ", "")).expect_err("nothing to show");
        assert!(error.contains("empty"));
    }

    #[test]
    fn only_unexpected_exits_notify() {
        assert!(
            for_task_exit("web", "dev", true, true, false).is_some(),
            "crash"
        );
        assert!(
            for_task_exit("web", "dev", true, false, false).is_none(),
            "a clean exit is not news"
        );
        assert!(
            for_task_exit("web", "dev", true, true, true).is_none(),
            "the user just stopped it"
        );
        assert!(
            for_task_exit("web", "dev", false, true, false).is_none(),
            "it was not running, so nothing changed"
        );
    }

    #[test]
    fn a_ready_notification_honours_the_setting() {
        assert!(
            for_task_ready("web", "dev", false).is_none(),
            "the setting is off by default and must be respected"
        );
        let notification = for_task_ready("web", "dev", true).expect("notify");
        assert!(notification.title.contains("web"));
        assert!(notification.body.contains("dev"));
    }

    #[test]
    fn the_crash_notification_names_the_app_and_task() {
        let notification = for_task_exit("web", "dev", true, true, false).expect("notify");
        assert!(notification.title.contains("web"));
        assert!(notification.body.contains("dev"));
    }
}
