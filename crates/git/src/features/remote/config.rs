//! The settings a pull or push depends on, and how git reads them.

use std::collections::HashMap;

use crate::core::errors::Result;
use crate::core::repo::GitCommand;

/// The settings a pull or push depends on, by key. Git lowercases the section and name, but not a
/// branch's name or a URL: e.g. `branch.Feature.merge`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoteConfig {
    /// The keys, in the order they were first set, as `keys` lists them.
    order: Vec<String>,
    values: HashMap<String, Vec<String>>,
}

impl RemoteConfig {
    /// The value `key` is set to; the last, if it's set several times, as git takes it.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values
            .get(key)
            .and_then(|values| values.last())
            .map(String::as_str)
    }

    /// Every value `key` is set to, in order: a branch's `merge` can name several branches.
    pub fn all(&self, key: &str) -> &[String] {
        self.values.get(key).map_or(&[], Vec::as_slice)
    }

    pub fn has(&self, key: &str) -> bool {
        self.values.contains_key(key)
    }

    /// The keys set, in the order they were first set.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.order.iter().map(String::as_str)
    }

    pub fn add(&mut self, key: &str, value: &str) {
        match self.values.get_mut(key) {
            Some(values) => values.push(value.to_owned()),
            None => {
                self.order.push(key.to_owned());
                self.values.insert(key.to_owned(), vec![value.to_owned()]);
            }
        }
    }
}

/// The settings a pull or push depends on, in one read.
pub async fn read_config(run: &GitCommand) -> Result<RemoteConfig> {
    const PATTERN: &str = r"^(pull\.(rebase|ff)|push\.default|remote\.pushdefault|branch\..+\.(remote|merge|rebase|pushremote)|remote\..+\.(url|pushurl|fetch|push|skipfetchall))$";
    let output = match run.run(["config", "-z", "--get-regexp", PATTERN]).await {
        Ok(output) => output,
        // None of them is set.
        Err(error) if error.is_exit(1) => String::new(),
        Err(error) => return Err(error),
    };
    Ok(parse_config(&output))
}

/// The settings in `git config -z --get-regexp`'s output: each entry a key, a newline and its
/// value.
pub fn parse_config(output: &str) -> RemoteConfig {
    let mut config = RemoteConfig::default();
    for entry in output.split('\0') {
        if entry.is_empty() {
            continue;
        }
        match entry.split_once('\n') {
            Some((key, value)) => config.add(key, value),
            // A boolean set without a value (`[pull] rebase`) has no newline: it's true.
            None => config.add(entry, "true"),
        }
    }
    config
}

/// Whether git takes `value`, lowercased, for false; an empty one is false too.
pub fn is_false(value: &str) -> bool {
    matches!(value, "false" | "no" | "off" | "0" | "")
}

/// `key`'s value in `config`, lowercased.
fn lowercase(config: &RemoteConfig, key: &str) -> Option<String> {
    config.get(key).map(str::to_lowercase)
}

/// The remotes `git fetch --all` fetches (those not set to be skipped), going by `config`, but for
/// `except`, which a pull already fetched.
pub fn remotes_to_fetch(config: &RemoteConfig, except: &str) -> Vec<String> {
    remote_names(config, &["url"])
        .into_iter()
        .filter(|name| {
            let skip = lowercase(config, &format!("remote.{name}.skipfetchall"));
            name != except && skip.as_deref().is_none_or(is_false)
        })
        .collect()
}

/// The remotes that have one of `settings` (e.g. `url`, which fetching needs), by name, as
/// `config` has them.
pub fn remote_names(config: &RemoteConfig, settings: &[&str]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for key in config.keys() {
        // `remote.<name>.<setting>`: the name is all up to the last dot, as it can have dots.
        let Some((name, setting)) = key
            .strip_prefix("remote.")
            .and_then(|rest| rest.rsplit_once('.'))
        else {
            continue;
        };
        if name.is_empty() || setting.is_empty() {
            continue;
        }
        if settings.contains(&setting) && !names.iter().any(|known| known == name) {
            names.push(name.to_owned());
        }
    }
    names
}

/// How `branch` is set to be rebased when pulled, lowercased; `None` when it isn't set.
pub fn rebase_setting(config: &RemoteConfig, branch: &str) -> Option<String> {
    config
        .get(&format!("branch.{branch}.rebase"))
        .or_else(|| config.get("pull.rebase"))
        .map(str::to_lowercase)
}

/// Whether a pull of `branch` can make a merge commit, going by `config`.
pub fn makes_merge_commit(config: &RemoteConfig, branch: &str) -> bool {
    !rebases(config, branch) && lowercase(config, "pull.ff").as_deref() != Some("only")
}

/// Whether a pull of `branch` rebases, going by `config`: as in `git pull`, `pull.ff=only` wins.
pub fn rebases(config: &RemoteConfig, branch: &str) -> bool {
    let rebase = rebase_setting(config, branch);
    lowercase(config, "pull.ff").as_deref() != Some("only")
        && rebase.as_deref().is_some_and(|rebase| !is_false(rebase))
}

/// The remote `branch` is pushed to, as `git push` picks it: the branch's `pushRemote`, the
/// repository's `pushDefault`, or the remote it pulls from (unless that's the repository itself,
/// "."), by name or as a URL. Without any, the only remote, or `origin` among several; `None` if
/// there's none.
pub fn push_remote(config: &RemoteConfig, branch: &str) -> Option<String> {
    let upstream = config.get(&format!("branch.{branch}.remote"));
    let set = config
        .get(&format!("branch.{branch}.pushremote"))
        .or_else(|| config.get("remote.pushdefault"))
        .or(upstream.filter(|upstream| *upstream != "."));
    if let Some(set) = set.filter(|set| !set.is_empty()) {
        return Some(set.to_owned());
    }
    let mut remotes = remote_names(config, &["url", "pushurl"]);
    if remotes.len() == 1 {
        return remotes.pop();
    }
    remotes
        .iter()
        .any(|remote| remote == "origin")
        .then(|| "origin".to_owned())
}

/// Where `reference` (a full ref name, e.g. `refs/heads/main`) goes through `refspecs`, a remote's
/// `fetch` or `push` settings, as git maps it: e.g. to `refs/remotes/origin/main` through
/// `+refs/heads/*:refs/remotes/origin/*`. `None` if none of them takes it, or a negative one
/// (`^…`) leaves it out. A refspec without a destination takes a ref to the same name when
/// `pushing`, and nowhere when fetching. When `pushing`, `reference` is the branch checked out,
/// which a refspec can name as `HEAD`, and names that aren't full are taken for branches', as git
/// does.
pub fn map_ref<S: AsRef<str>>(refspecs: &[S], reference: &str, pushing: bool) -> Option<String> {
    let full = |name: &str| -> String {
        if !pushing || name.starts_with("refs/") {
            name.to_owned()
        } else if name == "HEAD" {
            reference.to_owned()
        } else {
            format!("refs/heads/{name}")
        }
    };
    let specs: Vec<&str> = refspecs
        .iter()
        .map(|spec| {
            let spec = spec.as_ref();
            spec.strip_prefix('+').unwrap_or(spec)
        })
        .collect();
    if specs.iter().any(|spec| {
        spec.strip_prefix('^')
            .is_some_and(|negative| match_ref(&full(negative), reference).is_some())
    }) {
        return None;
    }
    for spec in specs {
        if spec.starts_with('^') {
            continue;
        }
        // `:` alone pushes each branch to the same name.
        if spec == ":" {
            if pushing {
                return Some(reference.to_owned());
            }
            continue;
        }
        let mut parts = spec.split(':');
        let src = parts.next().unwrap_or("");
        let dst = parts.next().unwrap_or(if pushing { src } else { "" });
        if let Some(star) = match_ref(&full(src), reference)
            && !dst.is_empty()
        {
            return Some(full(dst).replacen('*', star, 1));
        }
    }
    None
}

/// What `pattern`'s `*` stands for in `reference` if it matches (`""` if it has none), or `None`.
fn match_ref<'a>(pattern: &str, reference: &'a str) -> Option<&'a str> {
    let Some((before, after)) = pattern.split_once('*') else {
        return (pattern == reference).then_some("");
    };
    if reference.len() < before.len() + after.len() {
        return None;
    }
    if !reference.starts_with(before) || !reference.ends_with(after) {
        return None;
    }
    Some(&reference[before.len()..reference.len() - after.len()])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `git config -z --get-regexp`'s output for `entries`, each a key and a value, or a key
    /// alone.
    fn output(entries: &[(&str, Option<&str>)]) -> String {
        entries
            .iter()
            .map(|(key, value)| match value {
                None => format!("{key}\0"),
                Some(value) => format!("{key}\n{value}\0"),
            })
            .collect()
    }

    fn config(entries: &[(&str, &str)]) -> RemoteConfig {
        let entries: Vec<_> = entries
            .iter()
            .map(|(key, value)| (*key, Some(*value)))
            .collect();
        parse_config(&output(&entries))
    }

    #[test]
    fn parse_config_takes_the_last_value_of_a_key_set_several_times_as_git_does_but_keeps_them_all()
    {
        let config = config(&[("pull.ff", "true"), ("pull.ff", "only")]);
        assert_eq!(config.get("pull.ff"), Some("only"));
        assert_eq!(config.all("pull.ff"), ["true", "only"]);
        assert!(config.has("pull.ff"));
        assert_eq!(config.get("pull.rebase"), None);
        assert!(config.all("pull.rebase").is_empty());
    }

    #[test]
    fn parse_config_takes_a_boolean_set_without_a_value_for_true() {
        assert_eq!(
            parse_config(&output(&[("pull.rebase", None)])).get("pull.rebase"),
            Some("true")
        );
    }

    #[test]
    fn parse_config_keeps_a_branchs_name_and_a_url_as_they_are_with_no_output_being_no_settings() {
        let config = config(&[("branch.Feature.merge", "refs/heads/Feature")]);
        assert_eq!(
            config.get("branch.Feature.merge"),
            Some("refs/heads/Feature")
        );
        assert_eq!(parse_config("").keys().count(), 0);
    }

    #[test]
    fn rebase_unless_the_setting_is_false_with_the_branchs_own_setting_first() {
        assert!(!rebases(&parse_config(""), "main"));
        assert!(rebases(&config(&[("pull.rebase", "true")]), "main"));
        assert!(rebases(&config(&[("pull.rebase", "merges")]), "main"));
        assert!(!rebases(&config(&[("pull.rebase", "No")]), "main"));
        let own = config(&[("pull.rebase", "true"), ("branch.main.rebase", "false")]);
        assert!(!rebases(&own, "main"));
        assert!(rebases(&own, "other"));
    }

    #[test]
    fn only_fast_forward_wins_over_rebasing_and_makes_no_merge_commit() {
        let ff_only = config(&[("pull.rebase", "true"), ("pull.ff", "only")]);
        assert!(!rebases(&ff_only, "main"));
        assert!(!makes_merge_commit(&ff_only, "main"));
        assert!(!makes_merge_commit(
            &config(&[("pull.rebase", "true")]),
            "main"
        ));
        assert!(makes_merge_commit(&parse_config(""), "main"));
    }

    #[test]
    fn remotes_to_fetch_is_every_remote_but_the_one_given_and_those_set_to_be_skipped() {
        let config = config(&[
            ("remote.origin.url", "https://example.com/a.git"),
            ("remote.fork.url", "https://example.com/b.git"),
            ("remote.mirror.url", "https://example.com/c.git"),
            ("remote.mirror.skipfetchall", "true"),
            ("remote.other.url", "https://example.com/d.git"),
            ("remote.other.skipfetchall", "off"),
        ]);
        assert_eq!(remotes_to_fetch(&config, "origin"), ["fork", "other"]);
        assert_eq!(
            remotes_to_fetch(&config, "none"),
            ["origin", "fork", "other"]
        );
    }

    #[test]
    fn map_ref_maps_a_ref_through_fetch_refspecs_as_git_does() {
        let fetch = ["+refs/heads/*:refs/remotes/origin/*"];
        assert_eq!(
            map_ref(&fetch, "refs/heads/main", false).as_deref(),
            Some("refs/remotes/origin/main")
        );
        assert_eq!(map_ref(&fetch, "refs/tags/v1", false), None);
        assert_eq!(
            map_ref(
                &["refs/heads/main:refs/remotes/origin/main"],
                "refs/heads/feature",
                false
            ),
            None
        );
        // Left out by a negative refspec, wherever it is.
        assert_eq!(
            map_ref(&[fetch[0], "^refs/heads/wip/*"], "refs/heads/wip/x", false),
            None
        );
        // Without a destination, a fetch keeps it nowhere.
        assert_eq!(
            map_ref(&["refs/heads/main"], "refs/heads/main", false),
            None
        );
    }

    #[test]
    fn map_ref_maps_the_branch_checked_out_through_push_refspecs_as_git_does() {
        assert_eq!(
            map_ref(&["refs/heads/*:refs/for/*"], "refs/heads/main", true).as_deref(),
            Some("refs/for/main")
        );
        assert_eq!(
            map_ref(&["HEAD:refs/for/main"], "refs/heads/x", true).as_deref(),
            Some("refs/for/main")
        );
        assert_eq!(
            map_ref(&["main:trunk"], "refs/heads/main", true).as_deref(),
            Some("refs/heads/trunk")
        );
        assert_eq!(
            map_ref(&["refs/heads/main"], "refs/heads/main", true).as_deref(),
            Some("refs/heads/main")
        );
        assert_eq!(
            map_ref(&[":"], "refs/heads/x", true).as_deref(),
            Some("refs/heads/x")
        );
        assert_eq!(
            map_ref(&["refs/heads/main:refs/heads/main"], "refs/heads/x", true),
            None
        );
    }
}
