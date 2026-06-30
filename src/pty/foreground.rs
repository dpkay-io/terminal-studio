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

/// Returns all descendant PIDs of `pid` by walking the process tree.
/// Used as a fallback when a detected Claude process is a wrapper whose PID
/// doesn't match the inner Node.js process that wrote the session file.
pub fn find_descendant_pids(pid: u32) -> Vec<u32> {
    platform::find_descendant_pids(pid)
}

// ── Windows ───────────────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
mod platform {
    use super::ForegroundProcess;
    use std::collections::HashMap;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    const SKIP: &[&str] = &[
        "conhost.exe",
        "powershell.exe",
        "pwsh.exe",
        "cmd.exe",
        "wsl.exe",
    ];

    const MAX_TREE_DEPTH: u32 = 5;

    struct ProcessSnap {
        pid: u32,
        parent_pid: u32,
        name: String,
    }

    fn snapshot_processes() -> Vec<ProcessSnap> {
        let mut result = Vec::new();
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap == INVALID_HANDLE_VALUE {
                return result;
            }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            if Process32FirstW(snap, &mut entry) != 0 {
                loop {
                    result.push(ProcessSnap {
                        pid: entry.th32ProcessID,
                        parent_pid: entry.th32ParentProcessID,
                        name: wide_to_string(&entry.szExeFile),
                    });
                    if Process32NextW(snap, &mut entry) == 0 {
                        break;
                    }
                }
            }
            CloseHandle(snap);
        }
        result
    }

    fn build_children_map(procs: &[ProcessSnap]) -> HashMap<u32, Vec<usize>> {
        let mut map: HashMap<u32, Vec<usize>> = HashMap::new();
        for (i, p) in procs.iter().enumerate() {
            map.entry(p.parent_pid).or_default().push(i);
        }
        map
    }

    fn find_non_skipped_child(
        procs: &[ProcessSnap],
        children_map: &HashMap<u32, Vec<usize>>,
        parent_pid: u32,
        depth: u32,
    ) -> Option<(u32, String)> {
        if depth > MAX_TREE_DEPTH {
            return None;
        }
        let indices = children_map.get(&parent_pid)?;
        let mut best: Option<(u32, String)> = None;
        for &idx in indices {
            let p = &procs[idx];
            if SKIP.iter().any(|&s| p.name.eq_ignore_ascii_case(s)) {
                if let Some(desc) = find_non_skipped_child(procs, children_map, p.pid, depth + 1) {
                    best = Some(desc);
                }
            } else {
                best = Some((p.pid, p.name.clone()));
            }
        }
        best
    }

    pub fn detect_child(shell_pid: u32) -> Option<ForegroundProcess> {
        let procs = snapshot_processes();
        let children_map = build_children_map(&procs);
        let (child_pid, name) = find_non_skipped_child(&procs, &children_map, shell_pid, 0)?;
        let cmdline = get_process_cmdline(child_pid).unwrap_or_else(|| vec![name.clone()]);
        Some(ForegroundProcess {
            name,
            cmdline,
            pid: Some(child_pid),
        })
    }

    pub fn find_descendant_pids(pid: u32) -> Vec<u32> {
        let procs = snapshot_processes();
        let children_map = build_children_map(&procs);
        let mut result = Vec::new();
        let mut stack = vec![pid];
        while let Some(current) = stack.pop() {
            if let Some(indices) = children_map.get(&current) {
                for &idx in indices {
                    let child_pid = procs[idx].pid;
                    result.push(child_pid);
                    stack.push(child_pid);
                }
            }
        }
        result
    }

    fn get_process_cmdline(pid: u32) -> Option<Vec<String>> {
        use std::mem;
        use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
        };

        #[repr(C)]
        struct ProcessBasicInfo {
            exit_status: i32,
            peb_base_address: usize,
            affinity_mask: usize,
            base_priority: i32,
            unique_process_id: usize,
            inherited_from: usize,
        }

        #[link(name = "ntdll")]
        extern "system" {
            fn NtQueryInformationProcess(
                handle: isize,
                class: u32,
                info: *mut std::ffi::c_void,
                info_len: u32,
                ret_len: *mut u32,
            ) -> i32;
        }

        const PEB_PARAMS_OFFSET: usize = if mem::size_of::<usize>() == 8 {
            0x20
        } else {
            0x10
        };
        const CMDLINE_OFFSET: usize = if mem::size_of::<usize>() == 8 {
            0x70
        } else {
            0x40
        };
        const US_SIZE: usize = if mem::size_of::<usize>() == 8 { 16 } else { 8 };
        const PTR_OFFSET_IN_US: usize = if mem::size_of::<usize>() == 8 { 8 } else { 4 };

        unsafe fn read_mem<T>(handle: isize, addr: usize, buf: *mut T, len: usize) -> bool {
            let mut read: usize = 0;
            ReadProcessMemory(handle, addr as *const _, buf as *mut _, len, &mut read) != 0
                && read == len
        }

        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid);
            if handle == 0 {
                return None;
            }

            let result = (|| -> Option<Vec<String>> {
                let mut pbi: ProcessBasicInfo = mem::zeroed();
                if NtQueryInformationProcess(
                    handle,
                    0,
                    &mut pbi as *mut _ as *mut _,
                    mem::size_of::<ProcessBasicInfo>() as u32,
                    std::ptr::null_mut(),
                ) != 0
                {
                    return None;
                }

                let mut params_ptr: usize = 0;
                if !read_mem(
                    handle,
                    pbi.peb_base_address + PEB_PARAMS_OFFSET,
                    &mut params_ptr,
                    mem::size_of::<usize>(),
                ) {
                    return None;
                }

                let mut us_buf = [0u8; 16];
                if !read_mem(
                    handle,
                    params_ptr + CMDLINE_OFFSET,
                    us_buf.as_mut_ptr(),
                    US_SIZE,
                ) {
                    return None;
                }

                let length = u16::from_le_bytes([us_buf[0], us_buf[1]]) as usize;
                let buf_addr = if mem::size_of::<usize>() == 8 {
                    usize::from_le_bytes(
                        us_buf[PTR_OFFSET_IN_US..PTR_OFFSET_IN_US + 8]
                            .try_into()
                            .ok()?,
                    )
                } else {
                    u32::from_le_bytes(
                        us_buf[PTR_OFFSET_IN_US..PTR_OFFSET_IN_US + 4]
                            .try_into()
                            .ok()?,
                    ) as usize
                };

                if length == 0 || buf_addr == 0 {
                    return None;
                }

                let mut wchars = vec![0u16; length / 2];
                if !read_mem(handle, buf_addr, wchars.as_mut_ptr(), length) {
                    return None;
                }

                let raw = String::from_utf16_lossy(&wchars);
                let raw = raw.trim();
                if raw.is_empty() {
                    return None;
                }
                Some(parse_cmdline(raw))
            })();

            CloseHandle(handle);
            result
        }
    }

    /// Parses a Windows command line using the same rules as `CommandLineToArgvW`:
    /// - 2n backslashes + `"` → n backslashes, quote toggles
    /// - 2n+1 backslashes + `"` → n backslashes, literal `"`
    /// - n backslashes not followed by `"` → n literal backslashes
    pub(super) fn parse_cmdline(raw: &str) -> Vec<String> {
        let mut args = Vec::new();
        let mut current = String::new();
        let mut in_quotes = false;
        let chars: Vec<char> = raw.chars().collect();
        let len = chars.len();
        let mut i = 0;

        while i < len {
            let ch = chars[i];
            if ch == '\\' {
                let mut num_backslashes = 0;
                while i < len && chars[i] == '\\' {
                    num_backslashes += 1;
                    i += 1;
                }
                if i < len && chars[i] == '"' {
                    for _ in 0..num_backslashes / 2 {
                        current.push('\\');
                    }
                    if num_backslashes % 2 == 1 {
                        current.push('"');
                    } else {
                        in_quotes = !in_quotes;
                    }
                    i += 1;
                } else {
                    for _ in 0..num_backslashes {
                        current.push('\\');
                    }
                }
            } else if ch == '"' {
                in_quotes = !in_quotes;
                i += 1;
            } else if ch == ' ' && !in_quotes {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
                i += 1;
            } else {
                current.push(ch);
                i += 1;
            }
        }
        if !current.is_empty() {
            args.push(current);
        }
        args
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

    pub fn find_descendant_pids(pid: u32) -> Vec<u32> {
        let mut result = Vec::new();
        let mut stack = vec![pid];
        while let Some(current) = stack.pop() {
            let entries = match std::fs::read_dir("/proc") {
                Ok(e) => e,
                Err(_) => continue,
            };
            let current_str = current.to_string();
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                let Ok(child_pid) = name_str.parse::<u32>() else {
                    continue;
                };
                if child_pid == current {
                    continue;
                }
                let Ok(stat) = std::fs::read_to_string(format!("/proc/{child_pid}/stat")) else {
                    continue;
                };
                if ppid_from_stat(&stat) == Some(current_str.as_str()) {
                    result.push(child_pid);
                    stack.push(child_pid);
                }
            }
        }
        result
    }

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
    fn ppid_from_stat(stat: &str) -> Option<&str> {
        let after = stat.rfind(')')?.checked_add(2)?;
        let rest = stat.get(after..)?;
        let mut parts = rest.split_whitespace();
        parts.next()?; // state
        parts.next() // ppid
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

    pub fn find_descendant_pids(_pid: u32) -> Vec<u32> {
        Vec::new()
    }

    pub fn detect_child(shell_pid: u32) -> Option<ForegroundProcess> {
        let out = std::process::Command::new("ps")
            .args(["-eo", "ppid=,pid=,command="])
            .output()
            .ok()?;
        if !out.status.success() {
            log::warn!("ps exited with status {}", out.status);
            return None;
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let shell_pid_str = shell_pid.to_string();
        for line in text.lines() {
            let trimmed = line.trim_start();
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
                    // ps merges argv with spaces — we can't reconstruct
                    // original boundaries. Split into executable + rest.
                    let (exe, remainder) = match cmd.split_once(' ') {
                        Some((e, r)) => (e, Some(r.trim())),
                        None => (cmd, None),
                    };
                    let name = exe.rsplit('/').next().unwrap_or(exe).to_string();
                    let mut cmdline = vec![exe.to_string()];
                    if let Some(r) = remainder {
                        if !r.is_empty() {
                            cmdline.push(r.to_string());
                        }
                    }
                    return Some(ForegroundProcess {
                        name,
                        cmdline,
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
    pub fn find_descendant_pids(_pid: u32) -> Vec<u32> {
        Vec::new()
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

    #[cfg(target_os = "windows")]
    mod windows_cmdline {
        use super::super::platform::parse_cmdline;

        #[test]
        fn simple_args() {
            assert_eq!(parse_cmdline("node.exe app.js"), vec!["node.exe", "app.js"]);
        }

        #[test]
        fn quoted_args() {
            assert_eq!(
                parse_cmdline(r#""C:\Program Files\node.exe" "C:\app dir\cli.js""#),
                vec![r"C:\Program Files\node.exe", r"C:\app dir\cli.js"]
            );
        }

        #[test]
        fn mixed_quoted_and_plain() {
            assert_eq!(
                parse_cmdline(r#"node.exe "C:\Users\me\.npm\claude\cli.js" --resume"#),
                vec!["node.exe", r"C:\Users\me\.npm\claude\cli.js", "--resume"]
            );
        }

        #[test]
        fn empty_string() {
            let result: Vec<String> = parse_cmdline("");
            assert!(result.is_empty());
        }

        #[test]
        fn single_arg() {
            assert_eq!(parse_cmdline("claude.exe"), vec!["claude.exe"]);
        }

        #[test]
        fn escaped_quote_in_arg() {
            assert_eq!(
                parse_cmdline(r#"app.exe "say \"hello\"""#),
                vec!["app.exe", r#"say "hello""#]
            );
        }

        #[test]
        fn backslashes_before_quote() {
            // 2 backslashes + quote → 1 literal backslash, quote toggles
            assert_eq!(
                parse_cmdline(r#"app.exe "path\\" next"#),
                vec!["app.exe", r"path\", "next"]
            );
        }

        #[test]
        fn backslashes_not_before_quote() {
            // Backslashes not preceding a quote are literal
            assert_eq!(
                parse_cmdline(r"C:\Users\dpk\app.exe"),
                vec![r"C:\Users\dpk\app.exe"]
            );
        }

        #[test]
        fn triple_backslash_before_quote() {
            // 3 backslashes + quote → 1 backslash + literal quote
            assert_eq!(
                parse_cmdline(r#"app.exe "a\\\"b""#),
                vec!["app.exe", r#"a\"b"#]
            );
        }
    }
}
