//! Run a helper program (brew, hdiutil, ditto, plutil) with a hard timeout,
//! capturing its output without blocking on full pipes.

use std::ffi::OsString;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// How often the child's exit status is polled while waiting.
const POLL_INTERVAL: Duration = Duration::from_millis(50);
/// Extra time allowed for the output readers after the child exits.
const DRAIN_GRACE: Duration = Duration::from_secs(2);
/// Used to kill the child's whole process group on timeout (no libc dep).
const KILL_PROGRAM: &str = "/bin/kill";
/// Process group id 0 = put the child in a new group led by itself.
const NEW_PROCESS_GROUP: i32 = 0;

/// A helper program invocation. Built with a small builder so call sites
/// read like the command line they produce.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CommandSpec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub envs: Vec<(&'static str, &'static str)>,
    pub timeout: Duration,
}

impl CommandSpec {
    pub fn new(program: impl Into<PathBuf>, timeout: Duration) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            envs: Vec::new(),
            timeout,
        }
    }

    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn env(mut self, key: &'static str, value: &'static str) -> Self {
        self.envs.push((key, value));
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
/// there is one). The child and everything it spawned are killed when the
/// timeout elapses.
pub(super) fn run(spec: &CommandSpec) -> Result<String, String> {
    let name = spec.name();
    let mut child = Command::new(&spec.program)
        .args(&spec.args)
        .envs(spec.envs.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(NEW_PROCESS_GROUP)
        .spawn()
        .map_err(|e| format!("Failed to start {name}: {e}"))?;

    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let deadline = Instant::now() + spec.timeout;

    let status = wait_until(&mut child, deadline).map_err(|e| {
        kill_group(&mut child);
        e.unwrap_or_else(|| format!("{name} timed out after {}s", spec.timeout.as_secs_f32()))
    })?;

    let stdout = collect(&stdout);
    let stderr = collect(&stderr);
    if status.success() {
        return Ok(stdout);
    }
    Err(last_line(&stderr).unwrap_or_else(|| format!("{name} exited with {status}")))
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

/// Kill the child's process group (it leads its own group, see `run`), so
/// grandchildren holding our pipes die too, then reap the child.
fn kill_group(child: &mut Child) {
    let group = format!("-{}", child.id());
    let killed = Command::new(KILL_PROGRAM)
        .args(["-KILL", "--", &group])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if !matches!(killed, Ok(s) if s.success()) {
        let _ = child.kill();
    }
    let _ = child.wait();
}

/// Read a pipe to the end on its own thread so a chatty child never blocks
/// on a full pipe while we poll for its exit.
fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    if let Some(mut pipe) = pipe {
        let spawned = std::thread::Builder::new()
            .name("updater-pipe".into())
            .spawn(move || {
                let mut buf = Vec::new();
                let _ = pipe.read_to_end(&mut buf);
                let _ = tx.send(String::from_utf8_lossy(&buf).into_owned());
            });
        if let Err(e) = spawned {
            log::warn!("updater: failed to spawn pipe reader: {e}");
        }
    }
    rx
}

/// Output gathered by a `drain` thread, or empty if a grandchild still holds
/// the pipe open past the grace period.
fn collect(rx: &mpsc::Receiver<String>) -> String {
    rx.recv_timeout(DRAIN_GRACE).unwrap_or_default()
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

    fn process_alive(pid: &str) -> bool {
        std::process::Command::new("/bin/kill")
            .args(["-0", pid])
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
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
