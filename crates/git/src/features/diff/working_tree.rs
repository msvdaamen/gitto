//! Files in the working tree: confined to it, read as they are on disk, and saved with the user's
//! edits. Ported from `working-tree.ts`.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::fs::{File, Metadata, OpenOptions};
use std::future::Future;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use futures_lite::AsyncReadExt;
use serde::{Deserialize, Serialize};

use super::limits::MAX_BLOB_BYTES;
use crate::core::errors::{DiskChange, Error, Result};
use crate::core::repo::Repo;
use crate::core::runner::{WriteQueue, lock};

/// Where the file at `path` (relative to the repository, as git names it) is on disk, links
/// followed. Fails with `Error::OutsideRepository` for a path that leads out of the working tree
/// (with `..`, or through a link) or into the git directory, and with
/// `Error::WorkingTreeFileNotFound` if there's nothing there.
async fn resolve_working_tree_path(repo: &Repo, path: &str) -> Result<PathBuf> {
    confine(repo, path, true).await
}

/// Checks that `path` names something inside the working tree, as `resolve_working_tree_path`
/// does, but without following a link at its end: for git to read, which reads a link as the path
/// it points to, not the file there.
pub async fn check_working_tree_path(repo: &Repo, path: &str) -> Result<()> {
    confine(repo, path, false).await.map(|_| ())
}

async fn confine(repo: &Repo, path: &str, follow_link: bool) -> Result<PathBuf> {
    let outside = || Error::OutsideRepository {
        path: path.to_owned(),
    };
    let relative = Path::new(path);
    if path.is_empty() || relative.is_absolute() || relative.has_root() || !is_plain(path) {
        return Err(outside());
    }
    let root = real_path(repo.path()).await?;
    let full = root.join(relative);
    let resolved = if follow_link {
        real_path(&full).await
    } else {
        match (full.parent(), full.file_name()) {
            (Some(dir), Some(name)) => real_path(dir).await.map(|dir| dir.join(name)),
            _ => return Err(outside()),
        }
    };
    let resolved = match resolved {
        Ok(resolved) => resolved,
        Err(error) if is_not_found(&error) => {
            return Err(Error::WorkingTreeFileNotFound {
                path: path.to_owned(),
            });
        }
        Err(error) => return Err(error.into()),
    };
    let Ok(inside) = resolved.strip_prefix(&root) else {
        return Err(outside());
    };
    // Also where a link led: into the git directory, say.
    if inside.as_os_str().is_empty() || !is_plain(&inside.to_string_lossy()) {
        return Err(outside());
    }
    Ok(resolved)
}

/// `path` with links resolved, as Node's `realpath` has it (not as a `\\?\` path on Windows).
async fn real_path(path: &Path) -> io::Result<PathBuf> {
    let real = async_fs::canonicalize(path).await?;
    Ok(dunce::simplified(&real).to_owned())
}

/// Whether `error` says there's nothing at a path: Node's `ENOENT`, or `ENOTDIR`.
fn is_not_found(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
    )
}

/// Whether a relative path stays where it starts and out of git directories: `.git` in any case,
/// as file systems on macOS and Windows don't tell `.GIT` from it.
fn is_plain(path: &str) -> bool {
    path.split(['\\', '/'])
        .all(|part| part != ".." && !part.eq_ignore_ascii_case(".git"))
}

/// A file in the working tree as it was read, and `version`, which names its bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkingTreeFile {
    pub contents: String,
    /// Changes whenever the file's bytes do: what a save checks the file still has.
    pub version: String,
}

/// The bytes of a file in the working tree, and their version (see `read_working_tree_bytes`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkingTreeBytes {
    pub bytes: Vec<u8>,
    pub version: String,
}

/// The contents of a file in the working tree, as they are on disk: to show more of its unstaged
/// changes, which git only has a patch of, and to edit it. Fails with `Error::FileTooLarge` above
/// `MAX_BLOB_BYTES`, like a blob, and with `Error::NotUtf8` for one that isn't UTF-8.
pub async fn read_working_tree_file(repo: &Repo, path: &str) -> Result<WorkingTreeFile> {
    let resolved = resolve_working_tree_path(repo, path).await?;
    let FileBytes { bytes, .. } = read_file(resolved, path, MAX_BLOB_BYTES).await?;
    let version = version_of(&bytes);
    let contents = String::from_utf8(bytes).map_err(|_| Error::NotUtf8)?;
    Ok(WorkingTreeFile { contents, version })
}

/// The bytes of the file at `path` in the working tree, as `read_working_tree_file` reads them, and
/// their version; `None` if there's no file there: nothing, or a folder, like a submodule's. Fails
/// with `Error::FileTooLarge` above `max_bytes` (TS's default was `MAX_BLOB_BYTES`).
pub async fn read_working_tree_bytes(
    repo: &Repo,
    path: &str,
    max_bytes: u64,
) -> Result<Option<WorkingTreeBytes>> {
    let Some(resolved) = resolve_file(repo, path).await? else {
        return Ok(None);
    };
    let FileBytes { bytes, .. } = read_file(resolved, path, max_bytes).await?;
    let version = version_of(&bytes);
    Ok(Some(WorkingTreeBytes { bytes, version }))
}

/// How much of a file `scan_working_tree_file` reads at a time.
const SCAN_CHUNK_BYTES: u64 = 1024 * 1024;

/// Reads the file at `path` in the working tree a piece at a time, handing each to `on_chunk`,
/// which can't keep it: the buffer's used again for the next. Resolves to the file's version (see
/// `read_working_tree_file`), never having all of it in memory, whatever its size up to
/// `max_bytes` (`Error::FileTooLarge` past that; `u64::MAX` for no limit, TS's default); `None` if
/// there's no file there.
pub async fn scan_working_tree_file(
    repo: &Repo,
    path: &str,
    on_chunk: impl FnMut(&[u8]),
    max_bytes: u64,
) -> Result<Option<String>> {
    let Some(resolved) = resolve_file(repo, path).await? else {
        return Ok(None);
    };
    let (file, metadata) = blocking::unblock(move || {
        let file = open_file(&resolved)?;
        let metadata = file.metadata()?;
        io::Result::Ok((file, metadata))
    })
    .await?;
    if !metadata.is_file() {
        return Err(Error::OutsideRepository {
            path: path.to_owned(),
        });
    }
    if metadata.len() > max_bytes {
        return Err(Error::FileTooLarge {
            bytes: metadata.len(),
        });
    }
    let file = async_fs::File::from(file);
    let buffer = vec![0; SCAN_CHUNK_BYTES.min(metadata.len().max(1)) as usize];
    scan(file, buffer, max_bytes, on_chunk).await.map(Some)
}

/// Reads `reader` to its end into `buffer` a piece at a time, as `scan_working_tree_file` does;
/// fails with `Error::FileTooLarge` once it's read more than `max_bytes`, which a file still being
/// written can have grown past since its size was checked.
async fn scan(
    mut reader: impl futures_lite::AsyncRead + Unpin,
    mut buffer: Vec<u8>,
    max_bytes: u64,
    mut on_chunk: impl FnMut(&[u8]),
) -> Result<String> {
    let mut hash = sha1_smol::Sha1::new();
    let mut total: u64 = 0;
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        total += read as u64;
        if total > max_bytes {
            return Err(Error::FileTooLarge { bytes: total });
        }
        let chunk = &buffer[..read];
        hash.update(chunk);
        on_chunk(chunk);
    }
    Ok(hash.digest().to_string())
}

/// Where the file at `path` is on disk (see `resolve_working_tree_path`); `None` if none is.
async fn resolve_file(repo: &Repo, path: &str) -> Result<Option<PathBuf>> {
    let resolved = match resolve_working_tree_path(repo, path).await {
        Ok(resolved) => resolved,
        Err(Error::WorkingTreeFileNotFound { .. }) => return Ok(None),
        Err(error) => return Err(error),
    };
    let is_file = async_fs::metadata(&resolved).await?.is_file();
    Ok(is_file.then_some(resolved))
}

/// A file's bytes, and its permissions (`None` where it has none, as on Windows).
struct FileBytes {
    bytes: Vec<u8>,
    mode: Option<u32>,
}

/// Opens the file at `resolved` to read it: not following a link put in its place since it was
/// checked, and not waiting for a writer, as opening a named pipe would: it's no file to read,
/// which is only known once it's open.
fn open_file(resolved: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    options.open(resolved)
}

#[cfg(unix)]
fn mode_of(metadata: &Metadata) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    Some(metadata.permissions().mode())
}

#[cfg(not(unix))]
fn mode_of(_metadata: &Metadata) -> Option<u32> {
    None
}

/// The bytes and permissions of the file at `resolved` (the path `path` leads to); fails with
/// `Error::FileTooLarge` above `max_bytes`.
async fn read_file(resolved: PathBuf, path: &str, max_bytes: u64) -> Result<FileBytes> {
    let path = path.to_owned();
    blocking::unblock(move || {
        let mut file = open_file(&resolved)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(Error::OutsideRepository { path });
        }
        if metadata.len() > max_bytes {
            return Err(Error::FileTooLarge {
                bytes: metadata.len(),
            });
        }
        Ok(FileBytes {
            bytes: read_at_most(&mut file, metadata.len(), max_bytes)?,
            mode: mode_of(&metadata),
        })
    })
    .await
}

/// All of `reader`, which is `len` bytes long as far as is known; fails with `Error::FileTooLarge`
/// past `max_bytes`, which a file still being written can have grown past since its size was
/// checked.
fn read_at_most(reader: impl Read, len: u64, max_bytes: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(len.min(max_bytes) as usize);
    reader
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max_bytes {
        return Err(Error::FileTooLarge {
            bytes: bytes.len() as u64,
        });
    }
    Ok(bytes)
}

/// Names `bytes`, the contents of a file in the working tree: what a save checks it still has.
pub fn version_of(bytes: &[u8]) -> String {
    sha1_smol::Sha1::from(bytes).hexdigest()
}

/// Saves to the repository's files, one after the other: a later one checks the file an earlier
/// one wrote. One line for all of them, rather than one per file: a file has more than one path
/// (`a.txt`, `./a.txt`, or through a linked folder), and which file a path leads to is only known
/// once it's been looked up on disk, which a later save could do sooner than an earlier one.
#[derive(Default)]
struct Saves(WriteQueue<()>);

/// The version a file was at when Gitto last saved over it, and the one it saved.
#[derive(Debug, Clone)]
struct SavedVersion {
    from: String,
    to: String,
}

/// The version each file was at when Gitto last saved over it, and the one it saved, by its path
/// on disk: a save over the first is one over the second, as long as nothing else wrote the file
/// since. The last edits are sent as the window closes, without waiting for the version a save
/// under way will leave the file at, which the window won't be there to get. The latest
/// `MAX_SAVED_VERSIONS` files are kept, oldest first: only a file saved again needs its last save
/// remembered.
#[derive(Default)]
struct SavedVersions(Mutex<VecDeque<(PathBuf, SavedVersion)>>);

const MAX_SAVED_VERSIONS: usize = 1000;

impl SavedVersions {
    fn get(&self, target: &Path) -> Option<SavedVersion> {
        lock(&self.0)
            .iter()
            .find(|(path, _)| path == target)
            .map(|(_, saved)| saved.clone())
    }

    /// Remembers `saved` for `target`, as the newest.
    fn set(&self, target: PathBuf, saved: SavedVersion) {
        let mut versions = lock(&self.0);
        versions.retain(|(path, _)| *path != target);
        versions.push_back((target, saved));
        while versions.len() > MAX_SAVED_VERSIONS {
            versions.pop_front();
        }
    }
}

/// What a save expects of the file it saves over (see `save_working_tree_file`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveExpectation {
    /// The version of the file that was read to be edited.
    pub version: String,
    /// Saves whatever the file is now.
    pub overwrite: bool,
}

/// Saves `contents` as the file at `path` in the working tree, which has to be as it was read
/// (`expected.version`, or one Gitto saved over it since) unless `expected.overwrite`: fails with
/// `Error::FileChangedOnDisk` otherwise, or if it's gone, when `overwrite` makes it again. Only for
/// a save the user asked for: Gitto doesn't write to the working tree on its own.
///
/// The file keeps its permissions, and its CRLF line ends if it has them throughout: the edits can
/// have LF ones, as git shows a file with `core.autocrlf` in its patches. Written to a file next to
/// it, then put in its place, so it's never found half written. A link isn't saved through, nor is
/// a file that isn't UTF-8. Resolves to the saved file's version.
///
/// Its place in line, after the saves to the repository's files before it, is taken when it's
/// first polled (see `Saves`).
pub fn save_working_tree_file(
    repo: &Repo,
    path: &str,
    contents: &str,
    expected: &SaveExpectation,
) -> impl Future<Output = Result<String>> + Send + 'static + use<> {
    let saved_versions = repo.keep::<SavedVersions>();
    let saves = repo.keep::<Saves>();
    let repo = repo.clone();
    let path = path.to_owned();
    let contents = contents.to_owned();
    let expected = expected.clone();
    // One after the other, in the order they're first polled (see `Saves`).
    saves.0.run((), async move {
        // Not following a link at its end, which is refused below.
        let target = confine(&repo, &path, false).await?;
        let before = read_existing(target.clone(), &path).await?;
        let mut text = String::new();
        if let Some(before) = &before {
            text = decode_utf8(&before.bytes)?;
            let version = version_of(&before.bytes);
            let saved = saved_versions.get(&target);
            let unchanged = version == expected.version
                || saved.is_some_and(|saved| saved.from == expected.version && saved.to == version);
            if !expected.overwrite && !unchanged {
                return Err(Error::FileChangedOnDisk {
                    path,
                    how: DiskChange::Changed,
                });
            }
        } else if !expected.overwrite {
            return Err(Error::FileChangedOnDisk {
                path,
                how: DiskChange::Deleted,
            });
        }
        let bytes = if has_crlf_line_ends(&text) {
            to_crlf(&contents)
        } else {
            contents
        }
        .into_bytes();
        let written = version_of(&bytes);
        let mode = before.and_then(|before| before.mode);
        replace_file(target.clone(), bytes, mode).await?;
        saved_versions.set(
            target,
            SavedVersion {
                from: expected.version,
                to: written.clone(),
            },
        );
        Ok(written)
    })
}

/// The bytes and permissions of the file at `target` (the path `path` leads to); `None` if
/// there's none. Fails for a link, or anything else that isn't a file.
async fn read_existing(target: PathBuf, path: &str) -> Result<Option<FileBytes>> {
    match read_file(target, path, MAX_BLOB_BYTES).await {
        Ok(file) => Ok(Some(file)),
        Err(Error::Io(error)) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        // `O_NOFOLLOW` refuses a link with ELOOP.
        #[cfg(unix)]
        Err(Error::Io(error)) if error.raw_os_error() == Some(libc::ELOOP) => {
            Err(Error::OutsideRepository {
                path: path.to_owned(),
            })
        }
        Err(error) => Err(error),
    }
}

/// Puts `bytes` in place of the file at `target`, all at once, with permissions `mode` (the file's
/// own), or those of a new file if there was none.
async fn replace_file(target: PathBuf, bytes: Vec<u8>, mode: Option<u32>) -> Result<()> {
    blocking::unblock(move || {
        let mut name = OsString::from(".");
        name.push(target.file_name().unwrap_or_default());
        name.push(format!(".{}.gitto", uuid::Uuid::new_v4()));
        let temporary = target.with_file_name(name);
        let written = (|| {
            write_new(&temporary, &bytes, mode)?;
            std::fs::rename(&temporary, &target)
        })();
        if let Err(error) = written {
            // Cleaned up if it can be, failing with why the save did, as TS's `rm(…, { force })`.
            let _ = std::fs::remove_file(&temporary);
            return Err(error.into());
        }
        Ok(())
    })
    .await
}

/// Writes `bytes` to a new file at `path`, with permissions `mode`, or those of a new file.
fn write_new(path: &Path, bytes: &[u8], mode: Option<u32>) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode.map_or(0o666, |mode| mode & 0o7777));
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    // Permissions as they were, whatever the umask took off when it was made.
    #[cfg(unix)]
    if let Some(mode) = mode {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(mode & 0o7777))?;
    }
    #[cfg(not(unix))]
    let _ = mode;
    file.sync_all()
}

/// Whether every line of `text` but its last ends in CRLF.
fn has_crlf_line_ends(text: &str) -> bool {
    let bytes = text.as_bytes();
    text.contains("\r\n")
        && memchr::memchr_iter(b'\n', bytes).all(|at| at > 0 && bytes[at - 1] == b'\r')
}

/// `text` with its LF line ends made CRLF, and the CRLF ones left alone.
fn to_crlf(text: &str) -> String {
    let mut crlf = String::with_capacity(text.len() + text.len() / 16);
    for line in text.split_inclusive('\n') {
        match line.strip_suffix('\n') {
            Some(line) => {
                crlf.push_str(line.strip_suffix('\r').unwrap_or(line));
                crlf.push_str("\r\n");
            }
            None => crlf.push_str(line),
        }
    }
    crlf
}

/// `bytes` as UTF-8; fails with `Error::NotUtf8` if they aren't. A byte order mark is kept, as git
/// keeps it in a patch's lines.
pub fn decode_utf8(bytes: &[u8]) -> Result<String> {
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| Error::NotUtf8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_no_more_than_the_limit_of_a_file_thats_grown() {
        // A file that was 3 bytes when its size was checked.
        let grown = b"abcdef".as_slice();
        assert_eq!(read_at_most(grown, 3, 6).unwrap(), b"abcdef");
        let error = read_at_most(grown, 3, 5).unwrap_err();
        assert!(
            matches!(error, Error::FileTooLarge { bytes: 6 }),
            "{error:?}"
        );

        smol::block_on(async {
            let mut chunks = Vec::new();
            let scanned = scan(grown, vec![0; 2], 6, |chunk| chunks.push(chunk.to_vec())).await;
            assert_eq!(scanned.unwrap(), version_of(grown));
            assert_eq!(chunks, [b"ab", b"cd", b"ef"]);
            let error = scan(grown, vec![0; 2], 5, |_| {}).await.unwrap_err();
            assert!(
                matches!(error, Error::FileTooLarge { bytes: 6 }),
                "{error:?}"
            );
        });
    }
}
