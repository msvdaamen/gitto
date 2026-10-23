//! Reading HEAD's reflog as activity, and picking the remote a repository is hosted on. Ported
//! from `overview/parse.ts`.

use std::sync::LazyLock;

use regex::Regex;

use super::schema::{Activity, ActivityKind, HostedRemote};

/// With -z, entries are NUL-separated as well, so the output is a flat list of fields where every
/// three make up one entry: the reflog selector (with `--date=unix`, `HEAD@{<seconds>}`), git's
/// message for it, and the subject of the commit HEAD moved to.
pub const REFLOG_FORMAT: &str = "--format=%gd%x00%gs%x00%s";

/// JS's `.`, which doesn't match any line terminator; the regex crate's only leaves out `\n`.
macro_rules! any {
    () => {
        r"[^\n\r\x{2028}\x{2029}]"
    };
}

fn regex(pattern: &str) -> Regex {
    Regex::new(pattern).expect("a valid pattern")
}

static SELECTOR: LazyLock<Regex> = LazyLock::new(|| regex(r"@\{([0-9]+)\}$"));
static PHASE: LazyLock<Regex> = LazyLock::new(|| regex(r"\(([^)]*)\)$"));
static WITHOUT_PHASE: LazyLock<Regex> = LazyLock::new(|| regex(r"\s*\([^)]*\)$"));
static CHECKOUT: LazyLock<Regex> = LazyLock::new(|| regex(r" to (\S+)$"));
static REBASE: LazyLock<Regex> =
    LazyLock::new(|| regex(concat!(r"^returning to refs/heads/(", any!(), "+)$")));
static RESET: LazyLock<Regex> = LazyLock::new(|| regex(concat!(r"^moving to (", any!(), "+)$")));
static REMOTE_URL: LazyLock<Regex> =
    LazyLock::new(|| regex(concat!(r"^remote\.(", any!(), r"+)\.url$")));

/// The first group `pattern` captures in `text`, if it matches.
fn capture(pattern: &Regex, text: &str) -> Option<String> {
    pattern
        .captures(text)
        .map(|captures| captures[1].to_owned())
}

/// HEAD's reflog, as `log -g -z --date=unix` prints it with [`REFLOG_FORMAT`], as activity.
pub fn parse_reflog(output: &str) -> Vec<Activity> {
    let fields: Vec<&str> = output.split('\0').collect();
    let mut activity = Vec::new();
    for entry in fields.chunks_exact(3) {
        let [selector, message, subject] = entry else {
            unreachable!("chunks of 3")
        };
        let Some(seconds) = capture(&SELECTOR, selector.trim()) else {
            continue;
        };
        if message.is_empty() {
            continue;
        }
        if let Some((kind, target)) = to_activity(message) {
            activity.push(Activity {
                at: seconds.parse::<i64>().unwrap_or(0).saturating_mul(1000),
                kind,
                target,
                subject: (*subject).to_owned(),
                message: (*message).to_owned(),
            });
        }
    }
    activity
}

/// What a reflog message (e.g. `checkout: moving from main to feature`) says was done, and with
/// what; `None` for a step of something that's shown once it's done (a rebase's picks), or for what
/// didn't move HEAD (a stash resets it to itself).
fn to_activity(message: &str) -> Option<(ActivityKind, Option<String>)> {
    // `<command>: <details>`, the command with its phase if it has steps, e.g. `rebase (pick)`.
    let (command, details) = message.split_once(": ").unwrap_or((message, ""));
    let phase = capture(&PHASE, command);
    let command = WITHOUT_PHASE.replace(command, "");
    let mut words = command.split(' ');
    let name = words.next().unwrap_or("");
    // What the command was given, but for options: e.g. `origin main` from
    // `pull --ff-only origin main`.
    let operands = words
        .filter(|arg| !arg.starts_with('-'))
        .collect::<Vec<_>>()
        .join(" ");
    let operands = (!operands.is_empty()).then_some(operands);

    let activity = match name {
        "commit" => {
            let kind = if phase.as_deref() == Some("amend") {
                ActivityKind::Amend
            } else {
                ActivityKind::Commit
            };
            (kind, None)
        }
        // Branch names can't have spaces; a detached HEAD is named by its commit.
        "checkout" => (ActivityKind::Checkout, capture(&CHECKOUT, details)),
        "merge" => (ActivityKind::Merge, operands),
        "pull" => {
            // `pull --rebase` goes through the same steps as a rebase.
            if phase.as_deref().is_some_and(|phase| phase != "finish") {
                return None;
            }
            (ActivityKind::Pull, operands)
        }
        "rebase" => {
            if phase.as_deref() != Some("finish") {
                return None;
            }
            (ActivityKind::Rebase, capture(&REBASE, details))
        }
        "reset" => {
            let target = capture(&RESET, details);
            // `git stash` resets to HEAD, which leaves it where it was.
            if target.as_deref() == Some("HEAD") {
                return None;
            }
            (ActivityKind::Reset, target)
        }
        "cherry-pick" => (ActivityKind::CherryPick, None),
        "revert" => (ActivityKind::Revert, None),
        "clone" => (ActivityKind::Clone, None),
        _ => (ActivityKind::Other, None),
    };
    Some(activity)
}

/// The remote a repository is shown as hosted on, from its `remote.<name>.url` settings as
/// `config -z --get-regexp` prints them: `origin`, or else the first one; `None` without any.
pub fn pick_remote(output: &str) -> Option<HostedRemote> {
    // In the order they're set.
    let mut remotes: Vec<(String, &str)> = Vec::new();
    for entry in output.split('\0') {
        let Some((key, url)) = entry.split_once('\n') else {
            continue;
        };
        let Some(name) = capture(&REMOTE_URL, key) else {
            continue;
        };
        // A remote with several URLs fetches from the first.
        if !remotes.iter().any(|(known, _)| *known == name) {
            remotes.push((name, url));
        }
    }
    let (name, url) = remotes
        .iter()
        .find(|(name, _)| name == "origin")
        .or_else(|| remotes.first())?;
    Some(HostedRemote {
        name: name.clone(),
        url: (*url).to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `log -g -z` output for reflog messages, a second apart, each moving HEAD to commit
    /// "subject".
    fn reflog(messages: &[&str]) -> String {
        messages
            .iter()
            .enumerate()
            .flat_map(|(i, message)| {
                [
                    format!("HEAD@{{{}}}", 1_700_000_000 - i),
                    (*message).to_owned(),
                    "subject".to_owned(),
                ]
            })
            .collect::<Vec<_>>()
            .join("\0")
    }

    /// `config -z --get-regexp` output for remotes' URLs.
    fn config(remotes: &[(&str, &str)]) -> String {
        let entries: Vec<String> = remotes
            .iter()
            .map(|(name, url)| format!("remote.{name}.url\n{url}"))
            .collect();
        entries.join("\0") + "\0"
    }

    fn kinds(output: &str) -> Vec<String> {
        parse_reflog(output)
            .into_iter()
            .map(|Activity { kind, target, .. }| match target {
                Some(target) => format!("{} {target}", kind.as_str()),
                None => kind.as_str().to_owned(),
            })
            .collect()
    }

    fn remote(name: &str, url: &str) -> Option<HostedRemote> {
        Some(HostedRemote {
            name: name.to_owned(),
            url: url.to_owned(),
        })
    }

    #[test]
    fn reads_when_what_and_gits_message() {
        assert_eq!(
            parse_reflog(&reflog(&["commit: Fix it"])),
            [Activity {
                at: 1_700_000_000_000,
                kind: ActivityKind::Commit,
                target: None,
                subject: "subject".into(),
                message: "commit: Fix it".into(),
            }]
        );
    }

    #[test]
    fn tells_what_was_done_and_with_what() {
        assert_eq!(
            kinds(&reflog(&[
                "commit (initial): First",
                "commit (amend): Fix it",
                "commit (merge): Merge branch 'feature'",
                "checkout: moving from main to feature/x",
                "checkout: moving from main to 1a2b3c4d",
                "merge origin/main: Fast-forward",
                "pull --ff-only origin main: Fast-forward",
                "pull: Merge made by the 'ort' strategy.",
                "reset: moving to HEAD~1",
                "cherry-pick: Fix it",
                "revert: Revert \"Fix it\"",
                "clone: from https://example.com/repo.git",
                "Branch: renamed refs/heads/a to refs/heads/b",
            ])),
            [
                "commit",
                "amend",
                "commit",
                "checkout feature/x",
                "checkout 1a2b3c4d",
                "merge origin/main",
                "pull origin main",
                "pull",
                "reset HEAD~1",
                "cherry-pick",
                "revert",
                "clone",
                "other",
            ]
        );
    }

    #[test]
    fn shows_a_rebase_once_its_done_not_its_steps() {
        assert_eq!(
            kinds(&reflog(&[
                "rebase (finish): returning to refs/heads/feature",
                "rebase (pick): Fix it",
                "rebase (start): checkout main",
                "pull --rebase (finish): returning to refs/heads/main",
                "pull --rebase (pick): Fix it",
                "pull --rebase (start): checkout 1a2b3c4d",
            ])),
            ["rebase feature", "pull"]
        );
    }

    #[test]
    fn leaves_out_what_didnt_move_head_or_has_no_message() {
        assert_eq!(
            kinds(&reflog(&["reset: moving to HEAD", "", "commit: Fix it"])),
            ["commit"]
        );
    }

    #[test]
    fn reads_nothing_from_no_output() {
        assert_eq!(parse_reflog(""), []);
    }

    #[test]
    fn picks_origin() {
        assert_eq!(
            pick_remote(&config(&[("fork", "git@a:x"), ("origin", "git@b:y")])),
            remote("origin", "git@b:y")
        );
    }

    #[test]
    fn picks_the_first_remote_without_origin_and_its_first_url() {
        assert_eq!(
            pick_remote(&config(&[
                ("up.stream", "git@a:x"),
                ("up.stream", "git@a:z"),
                ("fork", "git@b:y"),
            ])),
            remote("up.stream", "git@a:x")
        );
    }

    #[test]
    fn picks_none_without_remotes() {
        assert_eq!(pick_remote(""), None);
    }
}
