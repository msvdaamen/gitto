//! Ported from `git/remote.ts`: where a remote is hosted.

use url::Url;

/// Hosts known by the name of their service.
const PROVIDERS: [(&str, &str); 5] = [
    ("github.com", "GitHub"),
    ("gitlab.com", "GitLab"),
    ("bitbucket.org", "Bitbucket"),
    ("codeberg.org", "Codeberg"),
    ("dev.azure.com", "Azure DevOps"),
];

/// Whether `url` starts with a scheme and `//`: `^[a-z][a-z\d+.-]*:\/\/`, case-insensitively.
fn has_scheme(url: &str) -> bool {
    let Some((scheme, _)) = url.split_once("://") else {
        return false;
    };
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
}

/// The host of a remote's URL, e.g. `github.com` for `git@github.com:owner/repo.git` or
/// `https://github.com/owner/repo`; `None` for a remote on this machine.
pub fn remote_host(url: &str) -> Option<String> {
    if has_scheme(url) {
        if url
            .get(..5)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("file:"))
        {
            return None;
        }
        // As the browser's `URL` reads it.
        let host = Url::parse(url).ok()?.host_str()?.to_lowercase();
        return (!host.is_empty()).then_some(host);
    }
    // scp-like, `[user@]host:path`; a Windows path's drive letter (`C:\`) isn't a host.
    scp_host(url).map(str::to_lowercase)
}

/// The host of an scp-like address: `^(?:[^@/\\]+@)?([^/\\:]{2,}):`.
fn scp_host(url: &str) -> Option<&str> {
    let host_at = |start: usize| {
        let rest = &url[start..];
        let end = rest.find(['/', '\\', ':']).unwrap_or(rest.len());
        let host = &rest[..end];
        (rest[end..].starts_with(':') && host.chars().count() >= 2).then_some(host)
    };
    // With a user first, if there's one: up to the first `@`, which no `/` or `\` comes before.
    let user = url
        .find(['@', '/', '\\'])
        .filter(|&at| at > 0 && url.as_bytes()[at] == b'@');
    user.and_then(|at| host_at(at + 1)).or_else(|| host_at(0))
}

/// What to call where a remote is hosted: "GitHub", or its host if it isn't a known one.
pub fn remote_provider(url: &str) -> String {
    let Some(host) = remote_host(url) else {
        return "Local".to_owned();
    };
    let known = host
        .strip_prefix("www.")
        .or_else(|| host.strip_prefix("ssh."))
        .unwrap_or(&host);
    PROVIDERS
        .iter()
        .find(|(provider_host, _)| *provider_host == known)
        .map_or(host.clone(), |(_, name)| (*name).to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    mod remote_host {
        use super::*;

        #[test]
        fn reads_the_host_of_urls_and_scp_like_addresses() {
            assert_eq!(
                remote_host("https://github.com/owner/repo.git").as_deref(),
                Some("github.com")
            );
            assert_eq!(
                remote_host("https://user:token@GitLab.com/owner/repo").as_deref(),
                Some("gitlab.com")
            );
            assert_eq!(
                remote_host("ssh://git@git.example.com:2222/owner/repo.git").as_deref(),
                Some("git.example.com")
            );
            assert_eq!(
                remote_host("git@github.com:owner/repo.git").as_deref(),
                Some("github.com")
            );
            assert_eq!(
                remote_host("example.com:repo.git").as_deref(),
                Some("example.com")
            );
        }

        #[test]
        fn has_none_for_a_repository_on_this_machine() {
            assert_eq!(remote_host("/Users/ada/upstream"), None);
            assert_eq!(remote_host("../upstream"), None);
            assert_eq!(remote_host("file:///Users/ada/upstream"), None);
            assert_eq!(remote_host(r"C:\Users\ada\upstream"), None);
        }
    }

    mod remote_provider {
        use super::*;

        #[test]
        fn names_known_hosts_by_their_service() {
            assert_eq!(remote_provider("git@github.com:owner/repo.git"), "GitHub");
            assert_eq!(
                remote_provider("ssh://git@ssh.github.com:443/owner/repo.git"),
                "GitHub"
            );
            assert_eq!(remote_provider("https://gitlab.com/owner/repo"), "GitLab");
        }

        #[test]
        fn names_others_by_their_host() {
            assert_eq!(
                remote_provider("https://git.example.com/repo.git"),
                "git.example.com"
            );
            assert_eq!(remote_provider("/Users/ada/upstream"), "Local");
        }
    }

    #[test]
    fn reads_an_scp_like_address_as_its_regex_did() {
        // Without a host of two characters after the user, the user is part of the host.
        assert_eq!(remote_host("x@y:repo").as_deref(), Some("x@y"));
        assert_eq!(remote_host("a/b@host:repo"), None);
    }
}
