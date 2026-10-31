//! One Gitto at a time on a profile, as the Electron app's `requestSingleInstanceLock`: a second
//! would share the database and what the UI remembers (`ui-storage.json`), each saving over the
//! other's. Launched again, Gitto tells the one running, which comes forward, and quits.
//!
//! The first to lock `gitto.lock` in the `userData` folder runs; the OS lets go of the lock as it
//! goes, however it goes. It listens on a socket beside it, `gitto.sock`, for the next launches:
//! a Unix domain socket, which Windows has too (since Windows 10 1803), through `uds_windows`
//! there, as std only has them on Unix.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, Read as _, Write as _};
use std::path::Path;
use std::time::Duration;

use futures::StreamExt as _;
use futures::channel::mpsc;

#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
#[cfg(windows)]
use uds_windows::{UnixListener, UnixStream};

const LOCK: &str = "gitto.lock";
const SOCKET: &str = "gitto.sock";
/// What a launch tells the one running: to come forward.
const SHOW: &str = "show";
/// How long a launch tries to reach the one running, which may have just locked and not be
/// listening yet.
const CONNECT_TRIES: u32 = 20;
const CONNECT_WAIT: Duration = Duration::from_millis(100);

/// Whether this Gitto is the one to run.
pub enum Instance {
    /// It is: the next launches come through [`Running::launched`].
    First(Running),
    /// Another is running on the profile, and was told to come forward: this one quits.
    Second,
}

/// The Gitto running on the profile: it holds the lock while it lives.
pub struct Running {
    _lock: File,
    launches: mpsc::UnboundedReceiver<()>,
}

impl Running {
    /// Resolves each time Gitto is launched again; `None` once that can't be heard any more.
    pub async fn launched(&mut self) -> Option<()> {
        self.launches.next().await
    }
}

/// Runs this Gitto on the profile in `user_data`, unless another is: then it's told to come
/// forward.
pub fn claim(user_data: &Path) -> io::Result<Instance> {
    std::fs::create_dir_all(user_data)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(user_data.join(LOCK))?;
    let socket = user_data.join(SOCKET);
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            if let Err(error) = tell_running(&socket) {
                log::warn!("Couldn't tell the Gitto running to come forward: {error}");
            }
            return Ok(Instance::Second);
        }
        Err(TryLockError::Error(error)) => return Err(error),
    }
    let (send, launches) = mpsc::unbounded();
    // Without it, the next launches still quit; they just can't bring this one forward.
    if let Err(error) = listen(&socket, send) {
        log::warn!("Couldn't listen for Gitto's next launches: {error}");
    }
    Ok(Instance::First(Running {
        _lock: lock,
        launches,
    }))
}

/// Listens on `socket` for the next launches, on a thread of its own, sending each that asks
/// to come forward to `send`.
fn listen(socket: &Path, send: mpsc::UnboundedSender<()>) -> io::Result<()> {
    // Left behind by a Gitto that didn't quit as it should: no one listens on it, as the lock
    // was free.
    match std::fs::remove_file(socket) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    let listener = UnixListener::bind(socket)?;
    std::thread::Builder::new()
        .name("gitto-launches".into())
        .spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else {
                    continue;
                };
                if said_show(stream) && send.unbounded_send(()).is_err() {
                    return;
                }
            }
        })?;
    Ok(())
}

/// Whether a launch said to come forward: no more than that is read, nor waited for long.
fn said_show(stream: UnixStream) -> bool {
    let mut said = String::new();
    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
    let read = stream.take(SHOW.len() as u64 + 1).read_to_string(&mut said);
    read.is_ok() && said.trim_end() == SHOW
}

/// Tells the Gitto listening on `socket` to come forward.
fn tell_running(socket: &Path) -> io::Result<()> {
    let mut tries = 0;
    loop {
        match UnixStream::connect(socket) {
            Ok(mut stream) => return stream.write_all(format!("{SHOW}\n").as_bytes()),
            Err(error) => {
                tries += 1;
                if tries == CONNECT_TRIES {
                    return Err(error);
                }
                std::thread::sleep(CONNECT_WAIT);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first(user_data: &Path) -> Running {
        match claim(user_data).unwrap() {
            Instance::First(running) => running,
            Instance::Second => panic!("another Gitto was running"),
        }
    }

    #[test]
    fn brings_the_one_running_forward_rather_than_running_again() {
        let dir = tempfile::tempdir().unwrap();
        let mut running = first(dir.path());

        assert!(matches!(claim(dir.path()).unwrap(), Instance::Second));
        assert_eq!(smol::block_on(running.launched()), Some(()));
        assert!(matches!(claim(dir.path()).unwrap(), Instance::Second));
        assert_eq!(smol::block_on(running.launched()), Some(()));
    }

    #[test]
    fn runs_once_the_one_running_is_gone() {
        let dir = tempfile::tempdir().unwrap();
        drop(first(dir.path()));
        // Its socket is left behind, as by one that crashed.
        assert!(dir.path().join(SOCKET).exists());

        let mut running = first(dir.path());
        assert!(matches!(claim(dir.path()).unwrap(), Instance::Second));
        assert_eq!(smol::block_on(running.launched()), Some(()));
    }

    #[test]
    fn hears_nothing_from_what_isnt_a_launch() {
        let dir = tempfile::tempdir().unwrap();
        let mut running = first(dir.path());
        let socket = dir.path().join(SOCKET);
        UnixStream::connect(&socket)
            .unwrap()
            .write_all(b"something else\n")
            .unwrap();
        tell_running(&socket).unwrap();
        // Only the launch is heard.
        assert_eq!(smol::block_on(running.launched()), Some(()));
        assert!(running.launches.try_recv().is_err());
    }
}
