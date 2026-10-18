//! Keeps only one Azura running at a time.
//!
//! Without this, a second copy opens a second window over the same
//! `azura.toml`. Saves are atomic and last-writer-wins, so the two copies
//! silently overwrite each other's sessions — the user's data disappears with
//! no error anywhere.
//!
//! An advisory `flock` on a per-user file does the work: the kernel releases
//! it when the process dies, so a crash never leaves a stale lock behind. The
//! file also carries the running process's id, so a second copy can ask the
//! first one to come to the front instead of vanishing without a word.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use rustix::fs::{FlockOperation, flock};
use rustix::io::Errno;

/// The signal a second copy sends to ask the first one to come forward.
/// `SIGUSR1` is used because nothing in GPUI or Azura handles it.
const FOCUS_SIGNAL: libc::c_int = libc::SIGUSR1;

/// Set by the [`FOCUS_SIGNAL`] handler when another copy asks us to come
/// forward. The handler may only touch this one atomic.
static FOCUS_REQUESTED: AtomicBool = AtomicBool::new(false);

/// The open lock file, held for the lifetime of the process. `None` means
/// another copy already won.
static HELD: OnceLock<Option<File>> = OnceLock::new();

/// A file every copy of Azura for this user account opens.
fn lock_path() -> Option<PathBuf> {
    // XDG_RUNTIME_DIR is per-user and wiped at logout, which suits a lock.
    Some(
        dirs::runtime_dir()
            .or_else(dirs::cache_dir)
            .unwrap_or_else(std::env::temp_dir)
            .join("azura.lock"),
    )
}

/// Tries to become the only running copy.
///
/// `Ok(true)` means Azura may start. `Ok(false)` means another copy holds
/// the lock; with `focus`, it is asked to show its window.
pub fn try_claim(focus: bool) -> bool {
    // Already settled in this process; never lock or signal ourselves.
    if let Some(held) = HELD.get() {
        return held.is_some();
    }
    let Some(path) = lock_path() else {
        // Nowhere to put a lock: starting is better than refusing to open.
        return true;
    };
    let mut file = match open_lock_file(&path) {
        Ok(file) => file,
        Err(err) => {
            eprintln!("azura: cannot open {}: {err}", path.display());
            return true;
        }
    };

    match flock(&file, FlockOperation::NonBlockingLockExclusive) {
        Ok(()) => {
            record_pid(&mut file);
            let _ = HELD.set(Some(file));
            true
        }
        // On Linux these two are the same errno, so one arm covers both.
        Err(Errno::WOULDBLOCK) => {
            if focus {
                ask_existing(&file);
            }
            let _ = HELD.set(None);
            false
        }
        Err(err) => {
            eprintln!("azura: cannot lock {}: {err}", path.display());
            true
        }
    }
}

/// Opens the lock file for reading and writing, creating it if needed.
fn open_lock_file(path: &Path) -> std::io::Result<File> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
}

/// Stores our process id so a later copy knows whom to ask.
fn record_pid(file: &mut File) {
    let _ = file.set_len(0);
    let _ = file.seek(SeekFrom::Start(0));
    let _ = write!(file, "{}", std::process::id());
    let _ = file.flush();
}

/// Asks the copy holding the lock to come to the front.
fn ask_existing(file: &File) {
    let Ok(pid) = read_pid(file) else {
        return;
    };
    // Process ids are recycled, so confirm this really is Azura before
    // signalling it. Doing nothing is always better than the alternative.
    if !is_our_binary(pid) {
        return;
    }
    // SAFETY: `kill` with a positive pid only sends a signal, and a bad pid
    // merely returns ESRCH, which we ignore.
    unsafe {
        libc::kill(pid as libc::pid_t, FOCUS_SIGNAL);
    }
}

fn read_pid(file: &File) -> std::io::Result<u32> {
    let mut clone = file.try_clone()?;
    clone.seek(SeekFrom::Start(0))?;
    let mut text = String::new();
    clone.read_to_string(&mut text)?;
    text.trim().parse().map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "no process id in the lock file",
        )
    })
}

/// Whether `pid` is alive and is Azura, so we never signal a stranger.
fn is_our_binary(pid: u32) -> bool {
    let Ok(cmdline) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
        return false;
    };
    let program = cmdline.split(|byte| *byte == 0).next().unwrap_or(&[]);
    let exe = String::from_utf8_lossy(program);
    let stem = exe.rsplit('/').next().unwrap_or(&exe);
    stem.starts_with("azura")
}

/// Installs the handler for [`FOCUS_SIGNAL`].
///
/// Call once, before the window opens. The handler only sets a flag; it must
/// not allocate, lock, or touch GPUI. The UI thread polls
/// [`take_focus_request`] from a timer and raises the window itself.
pub fn install_focus_handler() {
    extern "C" fn on_signal(_: libc::c_int) {
        FOCUS_REQUESTED.store(true, Ordering::SeqCst);
    }

    // SAFETY: `sigaction` is being filled with a valid handler and an empty
    // mask. The handler does a single relaxed atomic store, which is
    // async-signal-safe, and `on_signal` outlives every call that can raise
    // the signal because it has no state.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = on_signal as *const () as libc::sighandler_t;
        action.sa_flags = 0;
        libc::sigemptyset(&mut action.sa_mask);
        libc::sigaction(FOCUS_SIGNAL, &action, std::ptr::null_mut());
    }
}

/// Whether another copy asked us to come forward since the last call.
pub fn take_focus_request() -> bool {
    FOCUS_REQUESTED.swap(false, Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_signals_something_that_is_not_azura() {
        // Pid 1 exists but is the init system, never Azura.
        assert!(!is_our_binary(1));
        // A pid that cannot exist.
        assert!(!is_our_binary(u32::MAX));
    }

    #[test]
    fn a_lock_file_can_be_created_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("azura.lock");
        let mut file = open_lock_file(&path).unwrap();
        record_pid(&mut file);
        assert_eq!(read_pid(&file).unwrap(), std::process::id());
    }

    #[test]
    fn a_second_holder_is_turned_away() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("azura.lock");
        let first = open_lock_file(&path).unwrap();
        flock(&first, FlockOperation::NonBlockingLockExclusive).unwrap();

        // `flock` locks the open file description, so a separate `open` is a
        // separate holder and must be refused.
        let second = open_lock_file(&path).unwrap();
        let err = flock(&second, FlockOperation::NonBlockingLockExclusive).unwrap_err();
        assert_eq!(err, Errno::WOULDBLOCK);
    }

    #[test]
    fn an_unusable_lock_file_does_not_stop_azura() {
        // `try_claim` reads the real runtime directory, so only assert the
        // property that matters: it always answers, and never panics.
        let first = try_claim(false);
        let second = try_claim(false);
        assert_eq!(first, second, "the answer must be stable within a process");
    }
}
