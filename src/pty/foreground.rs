/// A foreground process (non-shell child) detected under a given shell PID.
#[derive(Clone, Debug)]
pub struct ForegroundProcess {
    /// Short executable name, e.g. "ssh" or "vim".
    pub name: String,
    /// Full command-line arguments as parsed from the OS.  May be just [name]
    /// on platforms where arg retrieval is unavailable.
    pub cmdline: Vec<String>,
    /// OS process ID of the foreground process, if available.
    pub pid: Option<u32>,
}

/// Detect the first non-shell foreground child of `shell_pid`.
/// Returns `None` when the shell is at a prompt (no children) or detection fails.
/// This is called on a ~500 ms cached basis, so it may do file-system reads.
pub fn detect_child(shell_pid: u32) -> Option<ForegroundProcess> {
    platform::detect_child(shell_pid)
}

// ── Windows ───────────────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
mod platform {
    use super::ForegroundProcess;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    // Process names that are infrastructure, not user commands.
    const SKIP: &[&str] = &[
        "conhost.exe",
        "powershell.exe",
        "pwsh.exe",
        "cmd.exe",
        "wsl.exe",
    ];

    pub fn detect_child(shell_pid: u32) -> Option<ForegroundProcess> {
        let (child_pid, name) = find_child(shell_pid)?;
        Some(ForegroundProcess {
            cmdline: vec![name.clone()],
            name,
            pid: Some(child_pid),
        })
    }

    fn find_child(parent_pid: u32) -> Option<(u32, String)> {
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap == INVALID_HANDLE_VALUE {
                return None;
            }

            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

            let mut best: Option<(u32, String)> = None;

            if Process32FirstW(snap, &mut entry) != 0 {
                loop {
                    if entry.th32ParentProcessID == parent_pid {
                        let name = wide_to_string(&entry.szExeFile);
                        if !SKIP.iter().any(|&s| name.eq_ignore_ascii_case(s)) {
                            best = Some((entry.th32ProcessID, name));
                        }
                    }
                    if Process32NextW(snap, &mut entry) == 0 {
                        break;
                    }
                }
            }

            CloseHandle(snap);
            best
        }
    }

    fn wide_to_string(buf: &[u16; 260]) -> String {
        let len = buf.iter().position(|&c| c == 0).unwrap_or(260);
        String::from_utf16_lossy(&buf[..len])
    }
}

// ── Linux ─────────────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
mod platform {
    use super::ForegroundProcess;

    const SHELL_NAMES: &[&str] = &["bash", "sh", "zsh", "fish", "dash", "ksh", "csh", "tcsh"];

    pub fn detect_child(shell_pid: u32) -> Option<ForegroundProcess> {
        if let Some(proc) = detect_via_tpgid(shell_pid) {
            return Some(proc);
        }
        // WSL interop processes don't update the terminal's tpgid.
        // Fall back to scanning /proc for direct children of the shell.
        detect_via_child_scan(shell_pid)
    }

    fn detect_via_tpgid(shell_pid: u32) -> Option<ForegroundProcess> {
        let fg_pid = find_foreground_pid(shell_pid)?;
        proc_from_pid(fg_pid)
    }

    fn proc_from_pid(pid: u32) -> Option<ForegroundProcess> {
        let cmdline_bytes = std::fs::read(format!("/proc/{}/cmdline", pid)).ok()?;
        if cmdline_bytes.is_empty() {
            return None;
        }
        let args: Vec<String> = cmdline_bytes
            .split(|&b| b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect();
        if args.is_empty() {
            return None;
        }
        let name = args[0].rsplit('/').next().unwrap_or(&args[0]).to_string();
        Some(ForegroundProcess {
            name,
            cmdline: args,
            pid: Some(pid),
        })
    }

    /// Scan /proc for direct children of `shell_pid`, skipping known shells
    /// and infrastructure processes. Returns the first non-shell child found.
    fn detect_via_child_scan(shell_pid: u32) -> Option<ForegroundProcess> {
        let entries = std::fs::read_dir("/proc").ok()?;
        let shell_pid_str = shell_pid.to_string();
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            let Ok(pid) = name_str.parse::<u32>() else {
                continue;
            };
            if pid == shell_pid {
                continue;
            }
            let Ok(stat) = std::fs::read_to_string(format!("/proc/{}/stat", pid)) else {
                continue;
            };
            if ppid_from_stat(&stat) != Some(shell_pid_str.as_str()) {
                continue;
            }
            let comm = std::fs::read_to_string(format!("/proc/{}/comm", pid))
                .unwrap_or_default()
                .trim()
                .to_string();
            if SHELL_NAMES.iter().any(|&s| comm == s) || comm == "init" {
                continue;
            }
            if let Some(proc) = proc_from_pid(pid) {
                return Some(proc);
            }
        }
        None
    }

    /// Extract ppid (as string) from a /proc/PID/stat line.
    fn ppid_from_stat<'a>(stat: &'a str) -> Option<&'a str> {
        let after = stat.rfind(')')?.checked_add(2)?;
        let rest = stat.get(after..)?;
        let mut parts = rest.split_whitespace();
        parts.next()?; // state
        Some(parts.next()?) // ppid
    }

    fn find_foreground_pid(shell_pid: u32) -> Option<u32> {
        let stat = std::fs::read_to_string(format!("/proc/{}/stat", shell_pid)).ok()?;
        let tpgid = tpgid_from_stat(&stat)?;
        if tpgid == shell_pid || tpgid == 0 {
            return None;
        }
        Some(tpgid)
    }

    /// Parse the tpgid (foreground process group ID) from `/proc/<pid>/stat`.
    /// Format: `pid (comm) state ppid pgrp session tty_nr tpgid ...`
    /// Fields after `)`: state(0) ppid(1) pgrp(2) session(3) tty_nr(4) tpgid(5)
    fn tpgid_from_stat(stat: &str) -> Option<u32> {
        let after = stat.rfind(')')?.checked_add(2)?;
        let rest = stat.get(after..)?;
        let mut parts = rest.split_whitespace();
        parts.next()?; // state
        parts.next()?; // ppid
        parts.next()?; // pgrp
        parts.next()?; // session
        parts.next()?; // tty_nr
        parts.next()?.parse().ok() // tpgid
    }
}

// ── macOS ─────────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
mod platform {
    use super::ForegroundProcess;

    pub fn detect_child(shell_pid: u32) -> Option<ForegroundProcess> {
        // Single `ps` call: list all processes with parent PID, PID, and full command.
        // `-o ppid=,pid=,command=` gives parent PID, PID, and full command, no header.
        let out = std::process::Command::new("ps")
            .args(["-eo", "ppid=,pid=,command="])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let shell_pid_str = shell_pid.to_string();
        for line in text.lines() {
            let trimmed = line.trim_start();
            // Check if this process is a child of our shell
            if let Some(rest) = trimmed.strip_prefix(&shell_pid_str) {
                if rest.starts_with(' ') {
                    let rest = rest.trim_start();
                    let Some((pid_str, cmd)) = rest.split_once(' ') else {
                        continue;
                    };
                    let Ok(child_pid) = pid_str.trim().parse::<u32>() else {
                        continue;
                    };
                    let cmd = cmd.trim();
                    if cmd.is_empty() {
                        continue;
                    }
                    let args: Vec<String> = cmd.split_whitespace().map(str::to_string).collect();
                    let Some(first) = args.first() else {
                        continue;
                    };
                    let name = first.rsplit('/').next().unwrap_or(first).to_string();
                    return Some(ForegroundProcess {
                        name,
                        cmdline: args,
                        pid: Some(child_pid),
                    });
                }
            }
        }
        None
    }
}

// ── Fallback (other platforms) ────────────────────────────────────────────────

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
mod platform {
    use super::ForegroundProcess;
    pub fn detect_child(_shell_pid: u32) -> Option<ForegroundProcess> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_foreground_process_with_pid() {
        let proc = ForegroundProcess {
            name: "claude".into(),
            cmdline: vec!["claude".into()],
            pid: Some(12345),
        };
        assert_eq!(proc.pid, Some(12345));
        assert_eq!(proc.name, "claude");
    }

    #[test]
    fn test_foreground_process_without_pid() {
        let proc = ForegroundProcess {
            name: "vim".into(),
            cmdline: vec!["vim".into()],
            pid: None,
        };
        assert_eq!(proc.pid, None);
    }

    #[test]
    fn test_foreground_process_clone() {
        let original = ForegroundProcess {
            name: "ssh".into(),
            cmdline: vec!["ssh".into(), "user@host".into()],
            pid: Some(42),
        };
        let cloned = original.clone();
        assert_eq!(cloned.name, original.name);
        assert_eq!(cloned.cmdline, original.cmdline);
        assert_eq!(cloned.pid, original.pid);
    }
}
