//! Process trees, timeouts and interrupts (SPEC.md 9.6, 12.1).
//!
//! A command with a `timeout` must be stopped together with every process
//! it started, or a `sh -c "sleep 60 & ..."` keeps running and keeps the
//! output pipes open long after the deadline. On Unix such a command runs
//! in its own process group, which is killed as a whole; on Windows it is
//! put in a job object that is closed with "kill on close" set. Commands
//! without a timeout stay in ok's process group, so they keep reading from
//! the terminal and get Ctrl-C from it directly.
//!
//! Because a command in its own process group does not get the
//! terminal's Ctrl-C, ok handles SIGINT, SIGTERM and SIGHUP itself as soon
//! as it starts such a command or creates a temporary file: it passes
//! SIGTERM on to the process groups that are still running, removes the
//! files of `tempdir()` and `tempfile()`, and exits with code 130.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Instant;

/// What an interrupt must clean up before ok exits.
struct Pending {
    /// Process groups (Unix) of running commands with a timeout.
    #[cfg_attr(not(unix), allow(dead_code))]
    groups: Vec<u32>,
    /// Files and directories made by `tempdir()` and `tempfile()`.
    temps: Vec<PathBuf>,
}

static PENDING: Mutex<Pending> = Mutex::new(Pending {
    groups: Vec::new(),
    temps: Vec::new(),
});

fn pending() -> MutexGuard<'static, Pending> {
    // The registry holds plain data, so a panic elsewhere cannot leave it
    // inconsistent; keep using it.
    PENDING.lock().unwrap_or_else(|e| e.into_inner())
}

/// Install the interrupt handler, once. It is installed only when ok first
/// needs it, so a script that makes no temporary files and sets no timeout
/// keeps the system's default handling of Ctrl-C.
pub fn watch_interrupts() {
    #[cfg(any(unix, windows))]
    {
        static INSTALL: std::sync::Once = std::sync::Once::new();
        INSTALL.call_once(|| {
            // Failing to install (another handler in an embedding program)
            // only loses the clean-up on interrupt.
            let _ = ctrlc::set_handler(on_interrupt);
        });
    }
}

#[cfg(any(unix, windows))]
fn on_interrupt() {
    let state = pending();
    #[cfg(unix)]
    for &g in &state.groups {
        kill_group(g, nix::sys::signal::Signal::SIGTERM);
    }
    for p in state.temps.iter().rev() {
        remove_temp(p);
    }
    std::process::exit(130);
}

/// Remember a temporary file or directory for removal on interrupt.
pub fn track_temp(p: PathBuf) {
    watch_interrupts();
    pending().temps.push(p);
}

/// Forget a temporary entry that was removed normally.
pub fn untrack_temp(p: &std::path::Path) {
    let mut state = pending();
    if let Some(i) = state.temps.iter().rposition(|t| t == p) {
        state.temps.remove(i);
    }
}

/// Remove a temporary file or directory, ignoring errors.
pub fn remove_temp(p: &std::path::Path) {
    match std::fs::symlink_metadata(p) {
        Ok(m) if m.is_dir() => {
            let _ = std::fs::remove_dir_all(p);
        }
        Ok(_) => {
            let _ = std::fs::remove_file(p);
        }
        Err(_) => {}
    }
}

#[cfg(unix)]
fn kill_group(pgid: u32, signal: nix::sys::signal::Signal) {
    if let Ok(id) = i32::try_from(pgid)
        && id > 1
    {
        let _ = nix::sys::signal::killpg(nix::unistd::Pid::from_raw(id), signal);
    }
}

/// Prepare a command that has a timeout, before it is spawned: on Unix it
/// becomes the leader of a new process group.
pub fn prepare(c: &mut std::process::Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        c.process_group(0);
    }
    #[cfg(not(unix))]
    let _ = c;
}

/// A started command with a timeout and the processes it starts.
pub struct Tree {
    #[cfg(unix)]
    pgid: u32,
    #[cfg(windows)]
    job: Mutex<Option<win32job::Job>>,
    /// Killed or released: nothing more to do.
    done: AtomicBool,
    killed: AtomicBool,
}

impl Tree {
    /// Take charge of a child spawned after [`prepare`].
    pub fn adopt(child: &std::process::Child) -> Tree {
        watch_interrupts();
        #[cfg(unix)]
        pending().groups.push(child.id());
        #[cfg(not(any(unix, windows)))]
        let _ = child;
        Tree {
            #[cfg(unix)]
            pgid: child.id(),
            #[cfg(windows)]
            job: Mutex::new(windows_job(child)),
            done: AtomicBool::new(false),
            killed: AtomicBool::new(false),
        }
    }

    /// Kill the command and every process it started that is still in its
    /// process group or job.
    pub fn kill(&self) {
        if self.done.swap(true, Ordering::SeqCst) {
            return;
        }
        self.killed.store(true, Ordering::SeqCst);
        #[cfg(unix)]
        {
            kill_group(self.pgid, nix::sys::signal::Signal::SIGKILL);
            self.forget();
        }
        #[cfg(windows)]
        {
            // Closing the only handle of a job with "kill on close" ends
            // every process in it.
            drop(self.job.lock().unwrap_or_else(|e| e.into_inner()).take());
        }
    }

    /// Whether [`Tree::kill`] ran.
    pub fn killed(&self) -> bool {
        self.killed.load(Ordering::SeqCst)
    }

    /// The command finished in time: leave the processes it started in the
    /// background running, as they would be without a timeout.
    pub fn release(&self) {
        let first = !self.done.swap(true, Ordering::SeqCst);
        #[cfg(unix)]
        if first {
            self.forget();
        }
        #[cfg(windows)]
        if first && let Some(job) = self.job.lock().unwrap_or_else(|e| e.into_inner()).take() {
            // Without the limit, closing the handle leaves the job's
            // processes alone. If it cannot be cleared, keep the handle
            // open rather than kill them.
            if job
                .set_extended_limit_info(&win32job::ExtendedLimitInfo::new())
                .is_err()
            {
                std::mem::forget(job);
            }
        }
        #[cfg(not(any(unix, windows)))]
        let _ = first;
    }

    #[cfg(unix)]
    fn forget(&self) {
        let mut state = pending();
        if let Some(i) = state.groups.iter().position(|&g| g == self.pgid) {
            state.groups.remove(i);
        }
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        self.release();
    }
}

/// A job object with "kill on close" holding `child`, or `None` when
/// Windows refuses (then only the command itself is stopped on timeout).
#[cfg(windows)]
fn windows_job(child: &std::process::Child) -> Option<win32job::Job> {
    use std::os::windows::io::AsRawHandle;
    let mut info = win32job::ExtendedLimitInfo::new();
    info.limit_kill_on_job_close();
    let job = win32job::Job::create_with_limit_info(&info).ok()?;
    job.assign_process(child.as_raw_handle() as isize).ok()?;
    Some(job)
}

/// Kills a [`Tree`] when its deadline passes, even while ok is blocked
/// reading the command's output.
pub struct Watchdog {
    fired: Arc<AtomicBool>,
    stop: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Watchdog {
    /// Start the thread that kills `tree` at `deadline`. If the thread
    /// cannot be started, the watchdog never fires and only the caller's
    /// own deadline checks stop the command.
    pub fn start(tree: Arc<Tree>, deadline: Instant) -> Watchdog {
        let (tx, rx) = mpsc::channel::<()>();
        let fired = Arc::new(AtomicBool::new(false));
        let flag = fired.clone();
        let thread = std::thread::Builder::new()
            .name("ok-timeout".into())
            .spawn(move || {
                let wait = deadline.saturating_duration_since(Instant::now());
                if let Err(RecvTimeoutError::Timeout) = rx.recv_timeout(wait) {
                    flag.store(true, Ordering::SeqCst);
                    tree.kill();
                }
            })
            .ok();
        Watchdog {
            fired,
            stop: Some(tx),
            thread,
        }
    }

    /// Whether the deadline passed and the tree was killed.
    pub fn fired(&self) -> bool {
        self.fired.load(Ordering::SeqCst)
    }

    /// Stop watching. Returns whether the deadline had already passed.
    pub fn stop(&mut self) -> bool {
        drop(self.stop.take());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        self.fired()
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        self.stop();
    }
}
