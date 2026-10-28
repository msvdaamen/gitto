//! The name the user commits with. Ported from `user/commands.ts`.

use std::path::Path;

use crate::core::runner::{RunOptions, run_git};

/// The name the user commits with: `user.name` from git's global config; `None` if it isn't set,
/// or git couldn't be asked (it couldn't be run, or there's no home folder).
pub async fn get_user_name() -> Option<String> {
    let home = std::env::home_dir()?;
    user_name(&home, RunOptions::default()).await
}

/// `user.name` from git's global config, asking git in `home` with `options`.
async fn user_name(home: &Path, options: RunOptions) -> Option<String> {
    // Run in the home folder: it's the global config that's asked for, whatever folder git is in.
    let name = run_git(home, ["config", "--global", "user.name"], options)
        .await
        .map(|output| output.trim().to_owned())
        .unwrap_or_default();
    (!name.is_empty()).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Fixture;

    /// Runs git with its global config in `home`: named outright, as the tests' git is run with
    /// none (see `test_support::hermetic_env`).
    fn at_home(home: &Path) -> RunOptions {
        RunOptions::default()
            .env("HOME", home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("GIT_CONFIG_GLOBAL", home.join(".gitconfig"))
    }

    #[test]
    fn reads_the_global_user_name_or_none() {
        let fx = Fixture::new();
        let home = fx.root();
        smol::block_on(async {
            assert_eq!(user_name(home, at_home(home)).await, None);
            std::fs::write(home.join(".gitconfig"), "[user]\n\tname = Ada Lovelace \n").unwrap();
            assert_eq!(
                user_name(home, at_home(home)).await.as_deref(),
                Some("Ada Lovelace")
            );
            // Git that can't start in a folder that isn't there.
            assert_eq!(user_name(&home.join("missing"), at_home(home)).await, None);
        });
    }
}
