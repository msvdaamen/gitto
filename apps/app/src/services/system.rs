//! What the UI asks of the system: a folder to add, Gitto's own updates, what's new in it, and
//! whether it sends error reports. Ported from `packages/system`'s `SystemContext` (`server.ts`),
//! with its types (`schema.ts`), and from what `apps/electron/src/main.ts` gave it.
//!
//! The folder picker is GPUI's. The other three have no Rust backend yet: each is a trait with
//! TS's shape ([`Updates`], [`WhatsNew`], [`Telemetry`]) and a **stub** behind it, said so on
//! each, until the real one is ported (Sentry, the updater, the changelog the release workflow
//! builds).

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use futures::future::BoxFuture;
use futures::stream::{self, BoxStream, StreamExt};
use gpui_kit::{App, PathPromptOptions};
use serde::{Deserialize, Serialize};

/// The running Gitto's version: what the release build is given in `GITTO_VERSION`, as the
/// Electron app's comes from its package.json. [`OWN_BUILD`] for a build of one's own.
pub const APP_VERSION: &str = match option_env!("GITTO_VERSION") {
    Some(version) => version,
    None => OWN_BUILD,
};

/// The version of a build of one's own, which, like the Electron app's, has no changelog and
/// doesn't remember what's been seen.
pub const OWN_BUILD: &str = "0.0.0";

/// Where a build gets its updates from: a nightly only from the next nightly, a release only from
/// the next release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    Release,
    Nightly,
}

impl UpdateChannel {
    /// A nightly's version is `nightly` and when it was built, in minutes: 1.2.4-nightly29853462
    /// (`channelOf` in `@gitto/release/version`).
    pub fn of(version: &str) -> Self {
        let prerelease = version.split_once('-').map_or("", |(_, rest)| rest);
        let nightly = prerelease.split('.').any(|part| {
            part.strip_prefix("nightly")
                .is_some_and(|built| !built.is_empty() && built.bytes().all(|b| b.is_ascii_digit()))
        });
        if nightly {
            UpdateChannel::Nightly
        } else {
            UpdateChannel::Release
        }
    }
}

/// A newer version, while it's downloaded, then once it's installed on restart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Update {
    pub version: String,
    pub ready: bool,
}

/// The running Gitto, and the update it's getting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateState {
    /// The running version, e.g. `1.2.3` or `1.2.4-nightly29853462`.
    pub version: String,
    pub channel: UpdateChannel,
    pub update: Option<Update>,
}

/// Whether Gitto sends error reports, as the user's set it to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelemetryState {
    /// Whether this build sends them at all: one built without a place to send them to doesn't.
    pub available: bool,
    /// Whether the user lets it.
    pub enabled: bool,
    /// Whether it's sending them now. Once turned on, it only starts when Gitto restarts.
    pub sending: bool,
}

/// A change's Conventional Commits type (`CHANGE_TYPES` in `@gitto/release/changelog`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeType {
    Feat,
    Fix,
    Perf,
    Refactor,
    Docs,
    Style,
    Test,
    Build,
    Ci,
    Chore,
    Revert,
}

/// A change users see, from its commit's Conventional Commits title.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    #[serde(rename = "type")]
    pub kind: ChangeType,
    pub scope: Option<String>,
    pub breaking: bool,
    /// As it's titled, without the type: `add worktrees`.
    pub description: String,
    /// What users are told about it beyond its title.
    pub detail: Option<String>,
    pub pr: Option<u64>,
    pub commit: String,
}

/// A version, and what changed in it since the one before it on its channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionChanges {
    pub version: String,
    /// When it was built, as YYYY-MM-DD.
    pub date: String,
    pub changes: Vec<Change>,
}

/// Gitto's own updates.
pub trait Updates: Send + Sync {
    /// The state now, then whenever it changes, until it's dropped.
    fn watch(&self) -> BoxStream<'static, UpdateState>;
    /// Quits, installs the downloaded update and starts it. Does nothing until one is ready.
    fn install(&self);
}

/// What changed in Gitto, from the changelog it's built with.
pub trait WhatsNew: Send + Sync {
    /// The versions since the last one the user saw what changed in, up to this one.
    fn unseen(&self) -> BoxFuture<'static, io::Result<Vec<VersionChanges>>>;
    /// Every version the changelog has, up to this one.
    fn all(&self) -> Vec<VersionChanges>;
    /// Remembers this version as seen.
    fn seen(&self) -> BoxFuture<'static, io::Result<()>>;
}

/// The user's setting for sending error reports.
pub trait Telemetry: Send + Sync {
    fn state(&self) -> TelemetryState;
    /// Saves it; turning it off stops sending at once.
    fn set_enabled(&self, enabled: bool) -> BoxFuture<'static, io::Result<TelemetryState>>;
}

/// Native OS capabilities (TS's `SystemContext`). Cheap to clone.
#[derive(Clone)]
pub struct SystemContext {
    pub updates: Arc<dyn Updates>,
    pub changelog: Arc<dyn WhatsNew>,
    pub telemetry: Arc<dyn Telemetry>,
}

impl SystemContext {
    /// The stubs (see the module's doc), the telemetry setting kept in `settings_file` and the
    /// last version seen in `last_seen_file`.
    pub fn stubs(settings_file: PathBuf, last_seen_file: PathBuf) -> Self {
        SystemContext {
            updates: Arc::new(NoUpdates::new(APP_VERSION)),
            changelog: Arc::new(Changes::new(APP_VERSION, Vec::new(), last_seen_file)),
            telemetry: Arc::new(TelemetrySetting::new(
                settings_file.clone(),
                TelemetryState {
                    available: false,
                    enabled: read_telemetry_setting(&settings_file),
                    sending: false,
                },
            )),
        }
    }

    /// Opens the native folder picker; resolves to `None` when the user cancels. The picker is
    /// opened from a click, so it's over the window that asked.
    pub fn select_folder(
        cx: &App,
    ) -> impl std::future::Future<Output = anyhow::Result<Option<PathBuf>>> + use<> {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        });
        async move {
            let paths = paths.await??;
            Ok(paths.and_then(|paths| paths.into_iter().next()))
        }
    }
}

/// Whether the user lets Gitto send error reports, kept in `file` (`readTelemetrySetting` in
/// `apps/electron/src/telemetry.ts`). It's on until they turn it off.
pub fn read_telemetry_setting(file: &Path) -> bool {
    match std::fs::read_to_string(file) {
        Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(settings) => settings.get("telemetry") != Some(&serde_json::Value::Bool(false)),
            Err(error) => {
                log::error!("[telemetry] Couldn't read {}: {error}", file.display());
                true
            }
        },
        Err(error) => {
            if error.kind() != io::ErrorKind::NotFound {
                log::error!("[telemetry] Couldn't read {}: {error}", file.display());
            }
            true
        }
    }
}

/// **Stub** of `TelemetrySetting` (`apps/electron/src/telemetry.ts`): the user's setting, kept in
/// `file`, which the Electron app reads too, and whether Gitto's sending. There's no crash
/// reporter in the Rust app yet, so this build isn't `available` and never sends; the setting is
/// still kept as the user sets it, for the Electron app and for when one is added. Turning it
/// off stops sending at once; turning it on only starts when Gitto restarts.
pub struct TelemetrySetting {
    file: PathBuf,
    state: Arc<Mutex<TelemetryState>>,
}

impl TelemetrySetting {
    /// The setting kept in `file`, starting from `state`.
    pub fn new(file: PathBuf, state: TelemetryState) -> Self {
        TelemetrySetting {
            file,
            state: Arc::new(Mutex::new(state)),
        }
    }
}

impl Telemetry for TelemetrySetting {
    fn state(&self) -> TelemetryState {
        *self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn set_enabled(&self, enabled: bool) -> BoxFuture<'static, io::Result<TelemetryState>> {
        let file = self.file.clone();
        let state = Arc::clone(&self.state);
        let text = format!(
            "{}\n",
            serde_json::to_string_pretty(&serde_json::json!({ "telemetry": enabled }))
                .unwrap_or_default()
        );
        Box::pin(async move {
            if let Some(dir) = file.parent() {
                async_fs::create_dir_all(dir).await?;
            }
            // Saved first: the setting only changes once it's written.
            async_fs::write(&file, text).await?;
            let mut current = state.lock().unwrap_or_else(|e| e.into_inner());
            current.enabled = enabled;
            if !enabled {
                // Nothing to stop: nothing is ever sent (see the type's doc).
                current.sending = false;
            }
            Ok(*current)
        })
    }
}

/// **Stub** of the updater (`apps/electron/src/updater.ts`): there's no updater in the Rust app
/// yet, so it's never getting an update. Its state is the running version's.
pub struct NoUpdates {
    state: UpdateState,
}

impl NoUpdates {
    pub fn new(version: &str) -> Self {
        NoUpdates {
            state: UpdateState {
                version: version.to_owned(),
                channel: UpdateChannel::of(version),
                update: None,
            },
        }
    }
}

impl Updates for NoUpdates {
    fn watch(&self) -> BoxStream<'static, UpdateState> {
        // The state now, and no change after it, ever.
        stream::once(futures::future::ready(self.state.clone()))
            .chain(stream::pending())
            .boxed()
    }

    fn install(&self) {}
}

/// **Stub** of `Changes` (`apps/electron/src/whats-new.ts`): what changed for users since the
/// last version they saw what changed in, kept in `last_seen_file` as the Electron app keeps it.
/// The changelog the release workflow builds isn't read yet, nor is `userFacingChanges` ported:
/// it's given its versions, already the ones users see, newest first (none, for now).
pub struct Changes {
    version: String,
    versions: Vec<VersionChanges>,
    last_seen_file: PathBuf,
}

impl Changes {
    pub fn new(version: &str, versions: Vec<VersionChanges>, last_seen_file: PathBuf) -> Self {
        Changes {
            version: version.to_owned(),
            versions,
            last_seen_file,
        }
    }

    /// A build of one's own is neither in a changelog nor worth remembering.
    fn is_own_build(&self) -> bool {
        self.version == OWN_BUILD
    }
}

impl WhatsNew for Changes {
    fn unseen(&self) -> BoxFuture<'static, io::Result<Vec<VersionChanges>>> {
        if self.is_own_build() {
            return Box::pin(async { Ok(Vec::new()) });
        }
        let version = self.version.clone();
        let file = self.last_seen_file.clone();
        let versions = self.versions.clone();
        Box::pin(async move {
            let last_seen = async_fs::read_to_string(&file)
                .await
                .ok()
                .map(|text| text.trim().to_owned())
                .filter(|text| !text.is_empty());
            // Without a changelog to place it in, every version listed is after the last seen.
            let unseen = match &last_seen {
                Some(last) if *last != version => versions,
                _ => Vec::new(),
            };
            // Nothing to show: this one counts as seen, so going back to a newer one doesn't
            // show what's been seen already.
            if unseen.is_empty() && last_seen.as_deref() != Some(&version) {
                async_fs::write(&file, &version).await?;
            }
            Ok(unseen)
        })
    }

    fn all(&self) -> Vec<VersionChanges> {
        self.versions.clone()
    }

    fn seen(&self) -> BoxFuture<'static, io::Result<()>> {
        if self.is_own_build() {
            return Box::pin(async { Ok(()) });
        }
        let file = self.last_seen_file.clone();
        let version = self.version.clone();
        Box::pin(async move { async_fs::write(&file, version).await })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_a_nightly_from_a_release() {
        assert_eq!(
            UpdateChannel::of("1.2.4-nightly29853462"),
            UpdateChannel::Nightly
        );
        assert_eq!(UpdateChannel::of("1.2.3"), UpdateChannel::Release);
        assert_eq!(UpdateChannel::of("1.3.0-beta.1"), UpdateChannel::Release);
    }

    // `telemetry.test.ts`.

    #[test]
    fn is_on_until_its_turned_off() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_telemetry_setting(&dir.path().join("settings.json")));
    }

    #[test]
    fn is_on_when_the_settings_cant_be_read() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        std::fs::write(&file, "{").unwrap();
        assert!(read_telemetry_setting(&file));
    }

    #[test]
    fn stops_sending_at_once_when_its_turned_off_and_is_remembered() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        let setting = TelemetrySetting::new(
            file.clone(),
            TelemetryState {
                available: true,
                enabled: true,
                sending: true,
            },
        );

        let state = smol::block_on(setting.set_enabled(false)).unwrap();
        assert_eq!(
            state,
            TelemetryState {
                available: true,
                enabled: false,
                sending: false
            }
        );
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(saved, serde_json::json!({ "telemetry": false }));
        assert!(!read_telemetry_setting(&file));
    }

    #[test]
    fn only_starts_sending_once_gitto_restarts_when_its_turned_on() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        std::fs::write(&file, r#"{"telemetry":false}"#).unwrap();
        let setting = TelemetrySetting::new(
            file.clone(),
            TelemetryState {
                available: true,
                enabled: false,
                sending: false,
            },
        );

        let state = smol::block_on(setting.set_enabled(true)).unwrap();
        assert_eq!(
            state,
            TelemetryState {
                available: true,
                enabled: true,
                sending: false
            }
        );
        assert!(read_telemetry_setting(&file));
    }

    // `whats-new.test.ts`, with the changelog given as the versions users see.

    fn version(name: &str) -> VersionChanges {
        VersionChanges {
            version: name.to_owned(),
            date: "2026-10-12".to_owned(),
            changes: vec![Change {
                kind: ChangeType::Feat,
                scope: None,
                breaking: false,
                description: "add it".to_owned(),
                detail: None,
                pr: None,
                commit: "a".to_owned(),
            }],
        }
    }

    #[test]
    fn is_nothing_on_the_first_run_which_is_remembered_as_seen() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("last-seen-version");
        let changes = Changes::new(
            "1.2.4-nightly3",
            vec![version("1.2.4-nightly3")],
            file.clone(),
        );

        assert_eq!(smol::block_on(changes.unseen()).unwrap(), []);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "1.2.4-nightly3");
    }

    #[test]
    fn is_what_changed_since_the_last_seen_until_its_seen() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("last-seen-version");
        std::fs::write(&file, "1.2.4-nightly1\n").unwrap();
        let changes = Changes::new(
            "1.2.4-nightly3",
            vec![version("1.2.4-nightly3")],
            file.clone(),
        );

        assert_eq!(
            smol::block_on(changes.unseen()).unwrap(),
            [version("1.2.4-nightly3")]
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "1.2.4-nightly1\n");
        smol::block_on(changes.seen()).unwrap();
        assert_eq!(smol::block_on(changes.unseen()).unwrap(), []);
    }

    #[test]
    fn is_nothing_for_a_build_of_ones_own_which_isnt_remembered() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("last-seen-version");
        let changes = Changes::new(OWN_BUILD, vec![version("1.2.4-nightly3")], file.clone());

        assert_eq!(smol::block_on(changes.unseen()).unwrap(), []);
        smol::block_on(changes.seen()).unwrap();
        assert!(!file.exists());
    }

    #[test]
    fn never_has_an_update_to_install() {
        let updates = NoUpdates::new("1.2.3");
        let first = smol::block_on(updates.watch().next()).unwrap();
        assert_eq!(
            first,
            UpdateState {
                version: "1.2.3".into(),
                channel: UpdateChannel::Release,
                update: None
            }
        );
    }
}
