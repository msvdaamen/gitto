//! The settings a pull or push depends on, and how git reads them.

use std::collections::HashMap;

use crate::core::errors::Result;
use crate::core::repo::GitCommand;

/// The settings a pull or push depends on, read through the accessors: the repository's own, a
/// branch's ([`RemoteConfig::branch`]) and a remote's ([`RemoteConfig::remote`]), which spell
/// the keys as git has them, its section and name lowercased but not a branch's or remote's name:
/// e.g. `branch.Feature.merge`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoteConfig {
    /// The keys, in the order they were first set, as `keys` lists them.
    order: Vec<String>,
    values: HashMap<String, Vec<String>>,
}

impl RemoteConfig {
    /// The value `key` is set to; the last, if it's set several times, as git takes it.
    fn get(&self, key: &str) -> Option<&str> {
        self.values
            .get(key)
            .and_then(|values| values.last())
            .map(String::as_str)
    }

    /// Every value `key` is set to, in order: a branch's `merge` can name several branches.
    fn all(&self, key: &str) -> &[String] {
        self.values.get(key).map_or(&[], Vec::as_slice)
    }

    fn has(&self, key: &str) -> bool {
        self.values.contains_key(key)
    }

    /// The keys set, in the order they were first set.
    fn keys(&self) -> impl Iterator<Item = &str> {
        self.order.iter().map(String::as_str)
    }

    fn add(&mut self, key: &str, value: &str) {
        match self.values.get_mut(key) {
            Some(values) => values.push(value.to_owned()),
            None => {
                self.order.push(key.to_owned());
                self.values.insert(key.to_owned(), vec![value.to_owned()]);
            }
        }
    }

    /// The settings of the branch `name`, by its short name.
    pub fn branch<'n>(&self, name: &'n str) -> BranchConfig<'_, 'n> {
        BranchConfig { config: self, name }
    }

    /// The settings of the remote `name`.
    pub fn remote<'n>(&self, name: &'n str) -> RemoteSettings<'_, 'n> {
        RemoteSettings { config: self, name }
    }

    /// `pull.ff`: whether a pull merges by fast-forwarding.
    pub fn pull_ff(&self) -> Option<PullFf> {
        self.get("pull.ff").map(PullFf::parse)
    }

    /// `push.default`: where `git push` pushes a branch; `None` if it isn't set, or is set to a
    /// value git doesn't know.
    pub fn push_default(&self) -> Option<PushDefault> {
        self.get("push.default").and_then(PushDefault::parse)
    }

    /// The remotes that have one of `settings` (e.g. `url`, which fetching needs), by name.
    pub fn remote_names(&self, settings: &[&str]) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for key in self.keys() {
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

    /// The remotes `git fetch --all` fetches (those not set to be skipped), but for `except`,
    /// which a pull already fetched.
    pub fn remotes_to_fetch(&self, except: &str) -> Vec<String> {
        self.remote_names(&["url"])
            .into_iter()
            .filter(|name| name != except && !self.remote(name).skip_fetch_all())
            .collect()
    }
}

/// A branch's settings (see [`RemoteConfig::branch`]).
#[derive(Debug, Clone, Copy)]
pub struct BranchConfig<'c, 'n> {
    config: &'c RemoteConfig,
    name: &'n str,
}

impl<'c> BranchConfig<'c, '_> {
    fn key(&self, setting: &str) -> String {
        format!("branch.{}.{setting}", self.name)
    }

    /// `branch.<name>.remote`: the remote it pulls from, by name or as a URL; "." for the
    /// repository itself.
    pub fn remote(&self) -> Option<&'c str> {
        self.config.get(&self.key("remote"))
    }

    /// `branch.<name>.merge`: the branch it pulls from, by its full name on the remote, e.g.
    /// `refs/heads/main`; the last, if it's set several times (see `merges`).
    pub fn merge(&self) -> Option<&'c str> {
        self.config.get(&self.key("merge"))
    }

    /// Every branch it's set to pull from: `git pull` merges them all at once.
    pub fn merges(&self) -> &'c [String] {
        self.config.all(&self.key("merge"))
    }

    /// How it's rebased when pulled: as its own `rebase` says, or else `pull.rebase`; `None` when
    /// neither is set.
    pub fn rebase(&self) -> Option<PullRebase> {
        self.config
            .get(&self.key("rebase"))
            .or_else(|| self.config.get("pull.rebase"))
            .map(PullRebase::parse)
    }

    /// Whether a pull of it rebases: as in `git pull`, `pull.ff=only` wins.
    pub fn rebases(&self) -> bool {
        self.config.pull_ff() != Some(PullFf::Only)
            && self
                .rebase()
                .is_some_and(|rebase| rebase != PullRebase::False)
    }

    /// Whether a pull of it can make a merge commit.
    pub fn makes_merge_commit(&self) -> bool {
        !self.rebases() && self.config.pull_ff() != Some(PullFf::Only)
    }

    /// The remote it's pushed to, as `git push` picks it: its `pushRemote`, the repository's
    /// `pushDefault`, or the remote it pulls from (unless that's the repository itself, "."), by
    /// name or as a URL. Without any, the only remote, or `origin` among several; `None` if
    /// there's none.
    pub fn push_remote(&self) -> Option<String> {
        let upstream = self.remote();
        let set = self
            .config
            .get(&self.key("pushremote"))
            .or_else(|| self.config.get("remote.pushdefault"))
            .or(upstream.filter(|upstream| *upstream != "."));
        if let Some(set) = set.filter(|set| !set.is_empty()) {
            return Some(set.to_owned());
        }
        let mut remotes = self.config.remote_names(&["url", "pushurl"]);
        if remotes.len() == 1 {
            return remotes.pop();
        }
        remotes
            .iter()
            .any(|remote| remote == "origin")
            .then(|| "origin".to_owned())
    }
}

/// A remote's settings (see [`RemoteConfig::remote`]).
#[derive(Debug, Clone, Copy)]
pub struct RemoteSettings<'c, 'n> {
    config: &'c RemoteConfig,
    name: &'n str,
}

impl<'c> RemoteSettings<'c, '_> {
    fn key(&self, setting: &str) -> String {
        format!("remote.{}.{setting}", self.name)
    }

    /// Whether it has a URL, which a remote that's fetched from needs.
    pub fn has_url(&self) -> bool {
        self.config.has(&self.key("url"))
    }

    /// Its `fetch` refspecs (see `map_ref`).
    pub fn fetch_specs(&self) -> &'c [String] {
        self.config.all(&self.key("fetch"))
    }

    /// Its `push` refspecs (see `map_ref`).
    pub fn push_specs(&self) -> &'c [String] {
        self.config.all(&self.key("push"))
    }

    /// Whether `git fetch --all` leaves it out (`skipFetchAll`).
    pub fn skip_fetch_all(&self) -> bool {
        self.config
            .get(&self.key("skipfetchall"))
            .is_some_and(|skip| !is_false(skip))
    }
}

/// How a pull rebases (`pull.rebase`, or a branch's `rebase`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullRebase {
    /// It merges.
    False,
    True,
    /// With `--rebase-merges` (`merges`).
    Merges,
    /// Interactively (`interactive`), which Gitto doesn't: there's no one to use the editor. It
    /// rebases as for `true`.
    Interactive,
}

impl PullRebase {
    /// `value` as git takes it, in any case. One git doesn't know, which it refuses, is taken for
    /// `true`.
    fn parse(value: &str) -> Self {
        match value.to_lowercase().as_str() {
            "merges" | "m" => PullRebase::Merges,
            "interactive" | "i" => PullRebase::Interactive,
            value if is_false(value) => PullRebase::False,
            _ => PullRebase::True,
        }
    }
}

/// Whether a pull merges by fast-forwarding (`pull.ff`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullFf {
    /// Only by fast-forwarding, failing otherwise (`only`); it never rebases either.
    Only,
    /// Never: always with a merge commit (`false`).
    No,
    /// When it can (`true`, git's default).
    Yes,
}

impl PullFf {
    /// `value` as git takes it, in any case. One git doesn't know, which it refuses, is taken for
    /// `true`.
    fn parse(value: &str) -> Self {
        match value.to_lowercase().as_str() {
            "only" => PullFf::Only,
            value if is_false(value) => PullFf::No,
            _ => PullFf::Yes,
        }
    }
}

/// Where `git push` pushes a branch without being told (`push.default`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushDefault {
    Nothing,
    Current,
    /// To the branch it pulls from (`upstream`, or `tracking`, its old name).
    Upstream,
    Simple,
    Matching,
}

impl PushDefault {
    /// `value` as git takes it, in any case; `None` for one git doesn't know.
    fn parse(value: &str) -> Option<Self> {
        Some(match value.to_lowercase().as_str() {
            "nothing" => PushDefault::Nothing,
            "current" => PushDefault::Current,
            "upstream" | "tracking" => PushDefault::Upstream,
            "simple" => PushDefault::Simple,
            "matching" => PushDefault::Matching,
            _ => return None,
        })
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

/// Whether git takes `value`, in any case, for false; an empty one is false too.
fn is_false(value: &str) -> bool {
    matches!(
        value.to_lowercase().as_str(),
        "false" | "no" | "off" | "0" | ""
    )
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
        let rebases = |config: &RemoteConfig, branch: &str| config.branch(branch).rebases();
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
        assert!(!ff_only.branch("main").rebases());
        assert!(!ff_only.branch("main").makes_merge_commit());
        assert!(
            !config(&[("pull.rebase", "true")])
                .branch("main")
                .makes_merge_commit()
        );
        assert!(parse_config("").branch("main").makes_merge_commit());
    }

    #[test]
    fn settings_are_read_as_git_takes_their_values_in_any_case() {
        let rebase = |value: &str| config(&[("pull.rebase", value)]).branch("main").rebase();
        assert_eq!(rebase("Merges"), Some(PullRebase::Merges));
        assert_eq!(rebase("m"), Some(PullRebase::Merges));
        assert_eq!(rebase("interactive"), Some(PullRebase::Interactive));
        assert_eq!(rebase("off"), Some(PullRebase::False));
        assert_eq!(rebase("yes"), Some(PullRebase::True));
        assert_eq!(parse_config("").branch("main").rebase(), None);

        let ff = |value: &str| config(&[("pull.ff", value)]).pull_ff();
        assert_eq!(ff("ONLY"), Some(PullFf::Only));
        assert_eq!(ff("false"), Some(PullFf::No));
        assert_eq!(ff("true"), Some(PullFf::Yes));
        assert_eq!(parse_config("").pull_ff(), None);

        let push = |value: &str| config(&[("push.default", value)]).push_default();
        assert_eq!(push("tracking"), Some(PushDefault::Upstream));
        assert_eq!(push("Upstream"), Some(PushDefault::Upstream));
        assert_eq!(push("simple"), Some(PushDefault::Simple));
        assert_eq!(push("sideways"), None);
    }

    #[test]
    fn a_branchs_and_a_remotes_settings_are_read_by_their_names_as_they_are() {
        let config = config(&[
            ("branch.Feature.remote", "origin"),
            ("branch.Feature.merge", "refs/heads/a"),
            ("branch.Feature.merge", "refs/heads/b"),
            ("remote.my.fork.url", "https://example.com/a.git"),
            ("remote.my.fork.push", "refs/heads/*:refs/for/*"),
            (
                "remote.my.fork.fetch",
                "+refs/heads/*:refs/remotes/my.fork/*",
            ),
        ]);
        let branch = config.branch("Feature");
        assert_eq!(branch.remote(), Some("origin"));
        assert_eq!(branch.merge(), Some("refs/heads/b"));
        assert_eq!(branch.merges(), ["refs/heads/a", "refs/heads/b"]);
        assert_eq!(config.branch("feature").remote(), None);
        let remote = config.remote("my.fork");
        assert!(remote.has_url());
        assert_eq!(remote.push_specs(), ["refs/heads/*:refs/for/*"]);
        assert_eq!(
            remote.fetch_specs(),
            ["+refs/heads/*:refs/remotes/my.fork/*"]
        );
        assert!(!config.remote("origin").has_url());
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
        assert_eq!(config.remotes_to_fetch("origin"), ["fork", "other"]);
        assert_eq!(config.remotes_to_fetch("none"), ["origin", "fork", "other"]);
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
