//! What the remote feature hands the UI.

use serde::{Deserialize, Serialize};

/// Where the Push button pushes the checked-out branch, or why it can't.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PushTarget {
    /// Not on a branch, a branch without commits, or no remote to push to.
    Blocked { reason: String },
    Remote {
        /// The remote branch, by its remote's name and its own, e.g. `origin/feature`.
        name: String,
        /// Whether it's not there yet, as of the last fetch or push, so pushing makes it. Never
        /// for one that isn't fetched into a remote-tracking branch, which can't tell.
        is_new: bool,
        /// How many commits pushing would send, as of the last fetch or push.
        ahead: u64,
        /// Whether it's also the branch pulled from, so pulling brings in its commits.
        pulls_from: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_as_the_ui_knew_it() {
        let target = PushTarget::Remote {
            name: "origin/main".into(),
            is_new: false,
            ahead: 1,
            pulls_from: true,
        };
        assert_eq!(
            serde_json::to_string(&target).unwrap(),
            r#"{"kind":"remote","name":"origin/main","isNew":false,"ahead":1,"pullsFrom":true}"#
        );
        let blocked = PushTarget::Blocked {
            reason: "no".into(),
        };
        assert_eq!(
            serde_json::to_string(&blocked).unwrap(),
            r#"{"kind":"blocked","reason":"no"}"#
        );
    }
}
