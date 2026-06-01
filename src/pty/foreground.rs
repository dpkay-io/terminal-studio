/// A foreground process (non-shell child) detected under a given shell PID.
#[derive(Clone, Debug)]
pub struct ForegroundProcess {
    /// Short executable name, e.g. "ssh" or "vim".
    pub name: String,
    /// Full command-line arguments as parsed from the OS.  May be just [name]
    /// on platforms where arg retrieval is unavailable.
    pub cmdline: Vec<String>,
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
        let (_child_pid, name) = find_child(shell_pid)?;
        Some(ForegroundProcess {
            cmdline: vec![name.clone()],
            name,
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

    pub fn detect_child(shell_pid: u32) -> Option<ForegroundProcess> {
        let fg_pid = find_foreground_pid(shell_pid)?;
        let cmdline_bytes = std::fs::read(format!("/proc/{}/cmdline", fg_pid)).ok()?;
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
        })
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
        // pgrep -P <pid> lists direct children by PID
        let out = std::process::Command::new("pgrep")
            .args(["-P", &shell_pid.to_string()])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let child_pid: u32 = text.lines().next()?.trim().parse().ok()?;

        // ps -o command= gives the full command string without a header
        let ps = std::process::Command::new("ps")
            .args(["-o", "command=", "-p", &child_pid.to_string()])
            .output()
            .ok()?;
        let cmd = String::from_utf8_lossy(&ps.stdout);
        let cmd = cmd.trim();
        if cmd.is_empty() {
            return None;
        }
        let args: Vec<String> = cmd.split_whitespace().map(str::to_string).collect();
        let name = args.first()?.rsplit('/').next()?.to_string();
        Some(ForegroundProcess {
            name,
            cmdline: args,
        })
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
