//! `neohub://` links.
//!
//! What this module does is parse and validate; what it deliberately does not do is receive the
//! event from macOS. Delivering a URL to a running app goes through an Apple Event
//! (`kAEGetURL`), which needs an `NSAppleEventManager` handler and an Objective-C target object.
//! Until that exists, opening a link activates the app but cannot select anything — which is
//! stated in PROGRESS rather than papered over with a link that looks like it works.
//!
//! The parsing rules still matter on their own: a URL arrives from outside the process, so it is
//! untrusted input, and the only fields a link may influence are an app id and a path.

/// A validated link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeepLink {
    /// `neohub://open?id=<app-id>` or `neohub://open?path=/abs/dir`
    Open {
        app_id: Option<String>,
        path: Option<String>,
    },
    /// `neohub://start?id=<app-id>`
    Start { app_id: Option<String> },
}

/// Scheme the app registers.
pub const SCHEME: &str = "neohub";

/// Parse a link. Anything unexpected is `None` — a malformed link must not half-apply.
pub fn parse(url: &str) -> Option<DeepLink> {
    let rest = url
        .strip_prefix("neohub://")
        .or_else(|| url.strip_prefix("NeoHub://"))?;
    // `neohub://open?id=x` → host `open`, query `id=x`; also accept a path form `neohub:open?id=x`.
    let (host, query) = match rest.split_once('?') {
        Some((host, query)) => (host.trim_end_matches('/'), Some(query)),
        None => (rest.trim_end_matches('/'), None),
    };
    let params = parse_query(query.unwrap_or(""));
    let app_id = params
        .iter()
        .find(|(key, _)| key == "id")
        .map(|(_, value)| value.clone())
        .filter(|value| is_plausible_id(value));
    let path = params
        .iter()
        .find(|(key, _)| key == "path")
        .map(|(_, value)| value.clone())
        .filter(|value| value.starts_with('/') && !value.contains(".."));

    match host {
        "open" => {
            if app_id.is_none() && path.is_none() {
                return None;
            }
            Some(DeepLink::Open { app_id, path })
        }
        "start" => {
            let app_id = app_id?;
            Some(DeepLink::Start {
                app_id: Some(app_id),
            })
        }
        _ => None,
    }
}

/// App ids are opaque tokens; anything with separators or spaces is not one of ours.
fn is_plausible_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
}

/// `a=1&b=2` → pairs, percent-decoded, with `+` treated as a space.
fn parse_query(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            Some((percent_decode(key), percent_decode(value)))
        })
        .collect()
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let hex = &input[index + 1..index + 3];
                match u8::from_str_radix(hex, 16) {
                    Ok(byte) => {
                        out.push(byte);
                        index += 3;
                    }
                    Err(_) => {
                        out.push(b'%');
                        index += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The first argument that looks like one of our links, if any.
///
/// Some launch paths (and a developer typing `open -a NeoHub --args neohub://…`) hand the link
/// over as an argument. Using it when present costs nothing and makes those paths work.
pub fn from_args<I: IntoIterator<Item = String>>(args: I) -> Option<DeepLink> {
    args.into_iter().find_map(|arg| parse(&arg))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_open_link_carries_an_app_id() {
        assert_eq!(
            parse("neohub://open?id=abc-123"),
            Some(DeepLink::Open {
                app_id: Some("abc-123".into()),
                path: None
            })
        );
    }

    #[test]
    fn an_open_link_can_carry_a_path() {
        assert_eq!(
            parse("neohub://open?path=%2FUsers%2Fme%2Fcode%2Fweb"),
            Some(DeepLink::Open {
                app_id: None,
                path: Some("/Users/me/code/web".into())
            })
        );
    }

    #[test]
    fn a_start_link_needs_an_app_id() {
        assert!(matches!(
            parse("neohub://start?id=abc"),
            Some(DeepLink::Start { .. })
        ));
        assert_eq!(parse("neohub://start"), None, "nothing to start");
    }

    #[test]
    fn other_schemes_and_hosts_are_ignored() {
        assert_eq!(
            parse("devharbor://open?id=x"),
            None,
            "the old scheme is not ours"
        );
        assert_eq!(parse("https://example.com/open?id=x"), None);
        assert_eq!(parse("neohub://unknown?id=x"), None);
        assert_eq!(parse("neohub://open"), None, "open with no target");
    }

    #[test]
    fn an_implausible_id_is_dropped_rather_than_used() {
        // An id with a slash or a space is someone probing, not one of our ids.
        assert_eq!(parse("neohub://open?id=../../etc/passwd"), None);
        assert_eq!(parse("neohub://open?id=has%20space"), None);
        assert_eq!(parse("neohub://open?id=".to_string().as_str()), None);
    }

    #[test]
    fn a_traversing_path_is_dropped() {
        assert_eq!(
            parse("neohub://open?path=%2FUsers%2Fme%2F..%2F..%2Fetc"),
            None,
            "a link must not be able to point outside a project"
        );
        assert_eq!(
            parse("neohub://open?path=relative%2Fdir"),
            None,
            "only absolute paths are meaningful"
        );
    }

    #[test]
    fn a_path_may_contain_a_space_and_unicode() {
        assert_eq!(
            parse("neohub://open?path=%2FUsers%2Fme%2FMy%20Code%2F%E4%B8%AD%E6%96%87"),
            Some(DeepLink::Open {
                app_id: None,
                path: Some("/Users/me/My Code/中文".into())
            })
        );
    }

    #[test]
    fn an_id_and_a_path_together_are_both_kept() {
        match parse("neohub://open?id=abc&path=%2Ftmp%2Fweb") {
            Some(DeepLink::Open { app_id, path }) => {
                assert_eq!(app_id.as_deref(), Some("abc"));
                assert_eq!(path.as_deref(), Some("/tmp/web"));
            }
            other => panic!("expected an open link, got {other:?}"),
        }
    }

    #[test]
    fn the_first_link_in_the_arguments_wins() {
        let args = vec![
            "/Applications/NeoHub.app/Contents/MacOS/neohub-desktop".to_string(),
            "--data-dir".to_string(),
            "/tmp/x".to_string(),
            "neohub://open?id=web".to_string(),
        ];
        assert_eq!(
            from_args(args),
            Some(DeepLink::Open {
                app_id: Some("web".into()),
                path: None
            })
        );
    }

    #[test]
    fn arguments_without_a_link_are_ignored() {
        let args = vec!["--data-dir".to_string(), "/tmp/x".to_string()];
        assert_eq!(from_args(args), None);
    }
}
