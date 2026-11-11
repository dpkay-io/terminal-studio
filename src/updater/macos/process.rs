//! Run a helper program (brew, hdiutil, ditto, plutil) with a timeout.
//!
//! Output goes to unlinked temp files, not pipes: if the app quits while a
//! helper runs, a pipe's read end would close and the helper would die on
//! EPIPE part-way through its work (for brew, possibly mid-move of the app).
//! A file never refuses a write, and needs no reader threads.

use std::ffi::OsString;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// How often the child's exit status is polled while waiting.
const POLL_INTERVAL: Duration = Duration::from_millis(50);
/// Prefix of the (immediately unlinked) output capture files.
const OUTPUT_FILE_PREFIX: &str = "terminal-studio-cmd-";
/// Used to kill the child's whole process group on timeout (no libc dep).
const KILL_PROGRAM: &str = "/bin/kill";
/// `kill -KILL -- -<pgid>`: the `--` lets the negative group id through.
const KILL_GROUP_ARGS: [&str; 2] = ["-KILL", "--"];
/// Process group id 0 = put the child in a new group led by itself.
const NEW_PROCESS_GROUP: i32 = 0;

/// What to do with a helper that outlives its timeout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OnTimeout {
    /// Kill its whole process group (safe for read-only helpers).
    Kill,
    /// Stop waiting but let it finish (brew: killing it mid-move can leave
    /// the app missing, and its rollback would not run).
    Detach,
}

/// A helper program invocation. Built with a small builder so call sites
/// read like the command line they produce.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CommandSpec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub envs: Vec<(&'static str, &'static str)>,
    pub timeout: Duration,
    pub on_timeout: OnTimeout,
}

impl CommandSpec {
    pub fn new(program: impl Into<PathBuf>, timeout: Duration) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            envs: Vec::new(),
            timeout,
            on_timeout: OnTimeout::Kill,
        }
    }

    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn args<A: Into<OsString>>(mut self, args: impl IntoIterator<Item = A>) -> Self {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn env(mut self, key: &'static str, value: &'static str) -> Self {
        self.envs.push((key, value));
        self
    }

    pub fn detach_on_timeout(mut self) -> Self {
        self.on_timeout = OnTimeout::Detach;
        self
    }

    /// Short name for messages, e.g. `brew` for `/opt/homebrew/bin/brew`.
    fn name(&self) -> String {
        self.program
            .file_name()
            .unwrap_or(self.program.as_os_str())
            .to_string_lossy()
            .into_owned()
    }
}

/// Run `spec` to completion. Returns stdout on a zero exit status; otherwise
/// a one-line, user-presentable error (the last non-empty stderr line when
/// there is one). On timeout the helper is killed with its process group, or
/// left to finish when the spec says `OnTimeout::Detach`.
pub(super) fn run(spec: &CommandSpec) -> Result<String, String> {
    let name = spec.name();
    let stdout = OutputFile::create()?;
    let stderr = OutputFile::create()?;
    let mut child = Command::new(&spec.program)
        .args(&spec.args)
        .envs(spec.envs.iter().copied())
        .stdin(Stdio::null())
        .stdout(stdout.for_child()?)
        .stderr(stderr.for_child()?)
        .process_group(NEW_PROCESS_GROUP)
        .spawn()
        .map_err(|e| format!("Failed to start {name}: {e}"))?;

    let deadline = Instant::now() + spec.timeout;
    let status = match wait_until(&mut child, deadline) {
        Ok(status) => status,
        Err(None) => return Err(on_timeout(spec, &name, child)),
        Err(Some(msg)) => {
            kill_group(&mut child);
            return Err(msg);
        }
    };

    if status.success() {
        return Ok(stdout.contents());
    }
    Err(last_line(&stderr.contents()).unwrap_or_else(|| format!("{name} exited with {status}")))
}

fn on_timeout(spec: &CommandSpec, name: &str, mut child: Child) -> String {
    let secs = spec.timeout.as_secs_f32();
    match spec.on_timeout {
        OnTimeout::Kill => {
            kill_group(&mut child);
            format!("{name} timed out after {secs}s")
        }
        OnTimeout::Detach => {
            log::warn!(
                "updater: {name} (pid {}) still running after {secs}s; leaving it to finish",
                child.id()
            );
            reap_in_background(child);
            format!(
                "{name} is still running after {secs}s \u{2014} it will finish in the background"
            )
        }
    }
}

/// Poll until the child exits. `Err(None)` = deadline passed,
/// `Err(Some(msg))` = waiting itself failed.
fn wait_until(child: &mut Child, deadline: Instant) -> Result<ExitStatus, Option<String>> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if Instant::now() >= deadline => return Err(None),
            Ok(None) => std::thread::sleep(POLL_INTERVAL),
            Err(e) => return Err(Some(format!("Failed to wait for process: {e}"))),
        }
    }
}

/// Wait for a detached child on its own thread so it is reaped when it
/// finishes instead of lingering as a zombie while the app runs.
fn reap_in_background(mut child: Child) {
    let spawned = std::thread::Builder::new()
        .name("updater-reaper".into())
        .spawn(move || {
            let _ = child.wait();
        });
    if let Err(e) = spawned {
        log::warn!("updater: could not start reaper thread: {e}");
    }
}

/// Kill the child's process group (it leads its own group, see `run`), so
/// helpers it spawned in that group die too, then reap the child.
fn kill_group(child: &mut Child) {
    let group = format!("-{}", child.id());
    let killed = Command::new(KILL_PROGRAM)
        .args(KILL_GROUP_ARGS)
        .arg(&group)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if !matches!(killed, Ok(s) if s.success()) {
        let _ = child.kill();
    }
    let _ = child.wait();
}

/// Whether process `pid` exists. Signal 0 checks without signalling; EPERM
/// means it exists but belongs to someone else.
pub(super) fn is_alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    // SAFETY: kill with signal 0 only performs the existence/permission check.
    let rc = unsafe { libc::kill(pid, 0) };
    rc == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// An output capture file, unlinked as soon as it is created: the child
/// writes through a duplicate handle and we read the same inode back, so
/// nothing is left in the temp dir even if the app quits mid-run.
struct OutputFile {
    file: File,
}

impl OutputFile {
    fn create() -> Result<Self, String> {
        let path = super::unique_temp_path(OUTPUT_FILE_PREFIX);
        let file = File::options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| format!("Failed to create an output file: {e}"))?;
        if let Err(e) = std::fs::remove_file(&path) {
            log::warn!("updater: could not unlink {}: {e}", path.display());
        }
        Ok(Self { file })
    }

    fn for_child(&self) -> Result<Stdio, String> {
        self.file
            .try_clone()
            .map(Stdio::from)
            .map_err(|e| format!("Failed to share an output file: {e}"))
    }

    fn contents(mut self) -> String {
        let mut buf = Vec::new();
        let read = self
            .file
            .seek(SeekFrom::Start(0))
            .and_then(|_| self.file.read_to_end(&mut buf));
        if let Err(e) = read {
            log::warn!("updater: could not read helper output: {e}");
        }
        String::from_utf8_lossy(&buf).into_owned()
    }
}

fn last_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    const SHORT: Duration = Duration::from_secs(10);

    #[test]
    fn run_returns_stdout() {
        let spec = CommandSpec::new("/bin/echo", SHORT).arg("hi");
        assert_eq!(run(&spec), Ok("hi\n".to_string()));
    }

    #[test]
    fn run_passes_env() {
        let spec = CommandSpec::new("/bin/sh", SHORT)
            .arg("-c")
            .arg("printf %s \"$TS_PROCESS_TEST\"")
            .env("TS_PROCESS_TEST", "value");
        assert_eq!(run(&spec), Ok("value".to_string()));
    }

    #[test]
    fn run_reports_last_stderr_line() {
        let spec = CommandSpec::new("/bin/sh", SHORT)
            .arg("-c")
            .arg("echo a >&2; echo boom >&2; echo >&2; exit 3");
        assert_eq!(run(&spec), Err("boom".to_string()));
    }

    #[test]
    fn run_reports_exit_code_without_stderr() {
        let spec = CommandSpec::new("/bin/sh", SHORT).arg("-c").arg("exit 4");
        let err = run(&spec).unwrap_err();
        assert!(err.contains("sh"), "{err}");
        assert!(err.contains('4'), "{err}");
    }

    #[test]
    fn run_reports_missing_program() {
        let spec = CommandSpec::new("/nonexistent/terminal-studio-helper", SHORT);
        let err = run(&spec).unwrap_err();
        assert!(err.contains("terminal-studio-helper"), "{err}");
    }

    #[test]
    fn run_times_out() {
        let spec = CommandSpec::new("/bin/sleep", Duration::from_millis(200)).arg("5");
        let started = Instant::now();
        let err = run(&spec).unwrap_err();
        assert!(err.contains("timed out"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn run_output_is_not_a_pipe() {
        // If the app quits mid-run, a pipe's read end closes and brew dies
        // on EPIPE part-way through moving the app. Files never do that.
        let spec = CommandSpec::new("/bin/sh", SHORT)
            .arg("-c")
            .arg("if [ -p /dev/fd/1 ] || [ -p /dev/fd/2 ]; then echo pipe; else echo file; fi");
        assert_eq!(run(&spec), Ok("file\n".to_string()));
    }

    #[test]
    fn run_detached_timeout_leaves_the_process_running() {
        // brew must not be SIGKILLed mid-upgrade; it is left to finish.
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("finished");
        let script = format!("/bin/sleep 1; : > '{}'", marker.display());
        let spec = CommandSpec::new("/bin/sh", Duration::from_millis(200))
            .arg("-c")
            .arg(script)
            .detach_on_timeout();

        let err = run(&spec).unwrap_err();
        assert!(err.contains("still running"), "{err}");

        let deadline = Instant::now() + Duration::from_secs(5);
        while !marker.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(marker.exists(), "detached process was killed");
    }

    #[test]
    fn run_detached_child_is_reaped_when_it_finishes() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        let script = format!("echo $$ > '{}'; /bin/sleep 0.5", pid_file.display());
        let spec = CommandSpec::new("/bin/sh", Duration::from_millis(100))
            .arg("-c")
            .arg(script)
            .detach_on_timeout();
        assert!(run(&spec).is_err());

        let pid = std::fs::read_to_string(&pid_file).unwrap();
        let pid = pid.trim();
        // A zombie still answers `kill -0`; a reaped child does not.
        let deadline = Instant::now() + Duration::from_secs(5);
        while process_alive(pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!process_alive(pid), "detached child {pid} was never reaped");
    }

    fn process_alive(pid: &str) -> bool {
        std::process::Command::new("/bin/kill")
            .args(["-0", pid])
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    #[test]
    fn is_alive_tells_running_from_reaped_processes() {
        assert!(is_alive(std::process::id()));
        // launchd exists but is not ours to signal (EPERM): still alive.
        assert!(is_alive(1));
        let mut child = std::process::Command::new("/usr/bin/true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        assert!(!is_alive(pid));
    }

    #[test]
    fn run_timeout_kills_grandchildren() {
        // brew spawns helpers; a timeout must not leave them running.
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("grandchild.pid");
        let script = format!("/bin/sleep 30 & echo $! > '{}'; wait", pid_file.display());
        let spec = CommandSpec::new("/bin/sh", Duration::from_millis(300))
            .arg("-c")
            .arg(script);

        let err = run(&spec).unwrap_err();
        assert!(err.contains("timed out"), "{err}");

        let pid = std::fs::read_to_string(&pid_file).unwrap();
        let pid = pid.trim();
        let deadline = Instant::now() + Duration::from_secs(2);
        while process_alive(pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!process_alive(pid), "grandchild {pid} survived the timeout");
    }
}
