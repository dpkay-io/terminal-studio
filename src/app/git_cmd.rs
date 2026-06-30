use std::io::Read;
use std::path::Path;
use std::process::{Command, ExitStatus, Output, Stdio};
use std::time::{Duration, Instant};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) fn git_output(args: &[&str], dir: &Path) -> Option<String> {
    git_output_with_timeout(args, dir, DEFAULT_TIMEOUT)
}

pub(super) fn git_output_with_timeout(
    args: &[&str],
    dir: &Path,
    timeout: Duration,
) -> Option<String> {
    let output = run_git(args, dir, timeout)?;
    if output.status.success() {
        String::from_utf8(output.stdout).ok()
    } else {
        None
    }
}

pub(super) fn git_status_ok(args: &[&str], dir: &Path) -> bool {
    run_git(args, dir, DEFAULT_TIMEOUT)
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub(super) fn git_stderr_on_fail(args: &[&str], dir: &Path) -> Result<Output, String> {
    match run_git(args, dir, Duration::from_secs(30)) {
        Some(o) if o.status.success() => Ok(o),
        Some(o) => Err(String::from_utf8_lossy(&o.stderr).into_owned()),
        None => Err("git command timed out".into()),
    }
}

fn run_git(args: &[&str], dir: &Path, timeout: Duration) -> Option<Output> {
    let mut cmd = Command::new("git");
    cmd.args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("GIT_EDITOR", "true")
        .env("GIT_TERMINAL_PROMPT", "0");

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = cmd.spawn().ok()?;

    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();

    let stdout_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = stdout_pipe {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });

    let stderr_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = stderr_pipe {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });

    let start = Instant::now();
    let status: Option<ExitStatus> = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    log::warn!("git timed out after {timeout:?}: git {args:?}");
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                log::warn!("git try_wait error: {e}");
                break None;
            }
        }
    };

    let status = status?;
    let stdout = stdout_thread.join().unwrap_or_default();
    let stderr = stderr_thread.join().unwrap_or_default();

    Some(Output {
        status,
        stdout,
        stderr,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn git_output_returns_branch() {
        let cwd = std::env::current_dir().unwrap();
        let result = git_output(&["rev-parse", "--abbrev-ref", "HEAD"], &cwd);
        assert!(result.is_some());
        assert!(!result.unwrap().trim().is_empty());
    }

    #[test]
    fn git_output_non_git_dir() {
        let dir = if cfg!(windows) {
            PathBuf::from("C:\\Windows")
        } else {
            PathBuf::from("/tmp")
        };
        assert!(git_output(&["rev-parse", "--abbrev-ref", "HEAD"], &dir).is_none());
    }

    #[test]
    fn git_status_ok_succeeds() {
        let cwd = std::env::current_dir().unwrap();
        assert!(git_status_ok(&["status"], &cwd));
    }

    #[test]
    fn git_status_ok_fails_for_bad_dir() {
        let dir = if cfg!(windows) {
            PathBuf::from("C:\\Windows")
        } else {
            PathBuf::from("/tmp")
        };
        assert!(!git_status_ok(&["status"], &dir));
    }

    #[test]
    fn git_stderr_on_fail_returns_error() {
        let dir = if cfg!(windows) {
            PathBuf::from("C:\\Windows")
        } else {
            PathBuf::from("/tmp")
        };
        let result = git_stderr_on_fail(&["status"], &dir);
        assert!(result.is_err());
    }

    #[test]
    fn timeout_kills_slow_command() {
        let cwd = std::env::current_dir().unwrap();
        let very_short = Duration::from_millis(1);
        let result = run_git(&["log", "--all", "--oneline"], &cwd, very_short);
        // May or may not timeout depending on how fast git responds,
        // but should not hang or panic either way.
        let _ = result;
    }
}
