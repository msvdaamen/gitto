//! Listing the refs. Ported from `refs/commands.ts`.

use super::parse::{REFS_ARGS, parse_refs};
use super::schema::Ref;
use crate::core::errors::Result;
use crate::core::repo::Repo;

/// Local branches, remote branches and tags.
pub async fn list_refs(repo: &Repo) -> Result<Vec<Ref>> {
    Ok(parse_refs(&repo.read(REFS_ARGS).await?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Fixture;

    #[test]
    fn lists_branches_and_tags_marking_the_checked_out_branch() {
        let fx = Fixture::new();
        smol::block_on(async {
            let refs = list_refs(&fx.create_history_repo("history").await)
                .await
                .unwrap();
            let shown: Vec<String> = refs
                .iter()
                .map(|r| {
                    let current = if r.current { "*" } else { "" };
                    format!("{}:{}{current}", r.kind.as_str(), r.name)
                })
                .collect();
            assert_eq!(shown, ["local:main*", "local:side", "tag:v1"]);
        });
    }

    #[test]
    fn lists_nothing_in_a_repository_without_commits() {
        let fx = Fixture::new();
        fx.create_repo("empty");
        smol::block_on(async {
            let repo = fx.repos.open("empty").await.unwrap();
            assert_eq!(list_refs(&repo).await.unwrap(), []);
        });
    }
}
