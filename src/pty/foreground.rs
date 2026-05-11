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
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW,
        PROCESSENTRY32W, TH32CS_SNAPPROCESS,
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
        // Try to get the full command line; fall back to exe name only.
        let cmdline = get_cmdline(child_pid).unwrap_or_else(|| vec![name.clone()]);
        Some(ForegroundProcess { name, cmdline })
    }

    fn find_child(parent_pid: u32) -> Option<(u32, String)> {
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap == INVALID_HANDLE_VALUE {
                return None;
            }

            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

            let mut result: Option<(u32, String)> = None;

            if Process32FirstW(snap, &mut entry) != 0 {
                loop {
                    if entry.th32ParentProcessID == parent_pid {
                        let name = wide_to_string(&entry.szExeFile);
                        if !SKIP.iter().any(|&s| name.eq_ignore_ascii_case(s)) {
                            result = Some((entry.th32ProcessID, name));
                            break;
                        }
                    }
                    if Process32NextW(snap, &mut entry) == 0 {
                        break;
                    }
                }
            }

            CloseHandle(snap);
            result
        }
    }

    fn wide_to_string(buf: &[u16; 260]) -> String {
        let len = buf.iter().position(|&c| c == 0).unwrap_or(260);
        String::from_utf16_lossy(&buf[..len])
    }

    fn get_cmdline(pid: u32) -> Option<Vec<String>> {
        // wmic is deprecated on Windows 11 22H2+ but is still present on all current
        // systems.  This is a best-effort call: if it fails we fall back to the exe name.
        let output = std::process::Command::new("wmic")
            .args([
                "process",
                "where",
                &format!("ProcessId={}", pid),
                "get",
                "CommandLine",
                "/format:list",
            ])
            .output()
            .ok()?;

        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            if let Some(cmd) = line.strip_prefix("CommandLine=") {
                let cmd = cmd.trim();
                if !cmd.is_empty() {
                    return Some(parse_windows_cmdline(cmd));
                }
            }
        }
        None
    }

    fn parse_windows_cmdline(s: &str) -> Vec<String> {
        let mut args: Vec<String> = Vec::new();
        let mut current = String::new();
        let mut in_quotes = false;
        for c in s.chars() {
            match c {
                '"' => in_quotes = !in_quotes,
                ' ' | '\t' if !in_quotes => {
                    if !current.is_empty() {
                        args.push(std::mem::take(&mut current));
                    }
                }
                _ => current.push(c),
            }
        }
        if !current.is_empty() {
            args.push(current);
        }
        args
    }
}

// ── Linux ─────────────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
mod platform {
    use super::ForegroundProcess;

    pub fn detect_child(shell_pid: u32) -> Option<ForegroundProcess> {
        let child_pid = find_child_pid(shell_pid)?;
        let cmdline_bytes =
            std::fs::read(format!("/proc/{}/cmdline", child_pid)).ok()?;
        if cmdline_bytes.is_empty() {
            return None; // zombie
        }
        let args: Vec<String> = cmdline_bytes
            .split(|&b| b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect();
        if args.is_empty() {
            return None;
        }
        let name = args[0]
            .rsplit('/')
            .next()
            .unwrap_or(&args[0])
            .to_string();
        Some(ForegroundProcess { name, cmdline: args })
    }

    fn find_child_pid(shell_pid: u32) -> Option<u32> {
        // Walk all /proc/<N> entries and find a direct child of shell_pid.
        let proc = std::fs::read_dir("/proc").ok()?;
        for entry in proc.flatten() {
            let fname = entry.file_name();
            let Ok(child_pid): Result<u32, _> = fname.to_string_lossy().parse() else {
                continue;
            };
            if child_pid == shell_pid {
                continue;
            }
            let stat =
                match std::fs::read_to_string(format!("/proc/{}/stat", child_pid)) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
            if ppid_from_stat(&stat) == Some(shell_pid) {
                return Some(child_pid);
            }
        }
        None
    }

    /// Parse the ppid field from `/proc/<pid>/stat`.
    /// Format: `pid (comm) state ppid ...`
    /// The comm field may contain spaces, so we find the last `)` first.
    fn ppid_from_stat(stat: &str) -> Option<u32> {
        let after = stat.rfind(')')?.checked_add(2)?;
        let rest = stat.get(after..)?;
        // rest = "state ppid ..."
        let mut parts = rest.split_whitespace();
        parts.next()?; // state
        parts.next()?.parse().ok()
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
        Some(ForegroundProcess { name, cmdline: args })
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
