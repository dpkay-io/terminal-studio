//! Raw-binary self-update: swap the running executable in place, with
//! elevation when its directory is not writable. Used on Windows, Linux and
//! for standalone macOS binaries.

use super::APPLY_UPDATE_FLAG;

pub(super) fn apply_update_elevated(
    target_exe: &std::path::Path,
    new_bytes: &[u8],
) -> Result<(), String> {
    let temp_dir = std::env::temp_dir();
    let temp_name = if cfg!(target_os = "windows") {
        "terminal-studio-update.exe"
    } else {
        "terminal-studio-update"
    };
    let temp_path = temp_dir.join(temp_name);
    std::fs::write(&temp_path, new_bytes)
        .map_err(|e| format!("Failed to write update to temp: {e}"))?;

    let current_exe =
        std::env::current_exe().map_err(|e| format!("Cannot locate current exe: {e}"))?;

    let args = [
        APPLY_UPDATE_FLAG.to_string(),
        temp_path.display().to_string(),
        target_exe.display().to_string(),
    ];

    let result = run_elevated(&current_exe, &args);
    let _ = std::fs::remove_file(&temp_path);
    result
}

pub(super) fn preflight_checks(binary_path: &std::path::Path) -> Result<(), String> {
    let dir = binary_path
        .parent()
        .ok_or_else(|| "Cannot determine binary directory".to_string())?;

    let test_file = dir.join(".ts_update_preflight");
    std::fs::write(&test_file, b"test")
        .map_err(|e| format!("No write permission to install directory: {e}"))?;
    let _ = std::fs::remove_file(&test_file);

    Ok(())
}

pub(super) fn apply_update(_current_exe: &std::path::Path, new_bytes: &[u8]) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let dir = _current_exe
            .parent()
            .ok_or_else(|| "Cannot determine binary directory".to_string())?;
        let temp_path = dir.join("terminal-studio.update");
        std::fs::write(&temp_path, new_bytes)
            .map_err(|e| format!("Failed to write update file: {e}"))?;
        if let Err(e) = self_replace::self_replace(&temp_path) {
            let _ = std::fs::remove_file(&temp_path);
            return Err(format!("Failed to replace binary: {e}"));
        }
        let _ = std::fs::remove_file(&temp_path);
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    {
        let dir = _current_exe
            .parent()
            .ok_or_else(|| "Cannot determine binary directory".to_string())?;

        let temp_path = dir.join("terminal-studio.update");
        std::fs::write(&temp_path, new_bytes)
            .map_err(|e| format!("Failed to write update file: {e}"))?;

        let backup_path = backup_path_for(_current_exe);

        if let Err(e) = std::fs::rename(_current_exe, &backup_path) {
            let _ = std::fs::remove_file(&temp_path);
            return Err(format!("Failed to backup current binary: {e}"));
        }

        if let Err(e) = std::fs::rename(&temp_path, _current_exe) {
            let _ = std::fs::rename(&backup_path, _current_exe);
            return Err(format!("Failed to install new binary (rolled back): {e}"));
        }

        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(_current_exe, std::fs::Permissions::from_mode(0o755));

        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
fn backup_path_for(exe: &std::path::Path) -> std::path::PathBuf {
    exe.with_extension("old")
}

pub(super) fn platform_asset_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "terminal-studio-windows.exe"
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            "terminal-studio-macos-arm"
        } else {
            "terminal-studio-macos-intel"
        }
    } else {
        "terminal-studio-linux"
    }
}

pub fn cleanup_old_binary() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let ext = if cfg!(target_os = "windows") {
        "exe.old"
    } else {
        "old"
    };
    let old = exe.with_extension(ext);
    if old.exists() {
        let _ = std::fs::remove_file(&old);
    }
    // Also clean up leftover .update file from interrupted downloads
    if let Some(dir) = exe.parent() {
        let update_file = dir.join("terminal-studio.update");
        if update_file.exists() {
            let _ = std::fs::remove_file(&update_file);
        }
    }
}

pub fn handle_apply_update_flag() -> bool {
    let args: Vec<String> = std::env::args().collect();
    let flag_pos = args.iter().position(|a| a == APPLY_UPDATE_FLAG);
    let Some(pos) = flag_pos else {
        return false;
    };

    if args.len() < pos + 3 {
        eprintln!("Usage: --apply-update <source> <target>");
        return true;
    }

    let source = std::path::Path::new(&args[pos + 1]);
    let target = std::path::Path::new(&args[pos + 2]);

    if !source.exists() {
        eprintln!("Source file not found: {}", source.display());
        return true;
    }

    let bytes = match std::fs::read(source) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Failed to read source: {e}");
            return true;
        }
    };

    if let Err(e) = apply_update(target, &bytes) {
        eprintln!("Apply update failed: {e}");
    }

    true
}

#[cfg(target_os = "windows")]
fn run_elevated(exe: &std::path::Path, args: &[String]) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{WaitForSingleObject, INFINITE};
    use windows_sys::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let params = args
        .iter()
        .map(|a| {
            if a.contains(' ') {
                format!("\"{a}\"")
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    let verb: Vec<u16> = "runas\0".encode_utf16().collect();
    let file: Vec<u16> = exe
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let parameters: Vec<u16> = params.encode_utf16().chain(std::iter::once(0)).collect();

    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = parameters.as_ptr();
    info.nShow = SW_HIDE;

    let success = unsafe { ShellExecuteExW(&mut info) };
    if success == 0 {
        return Err("UAC elevation was denied or failed".to_string());
    }

    if info.hProcess != 0 {
        unsafe {
            WaitForSingleObject(info.hProcess, INFINITE);
            CloseHandle(info.hProcess);
        }
    }

    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn run_elevated(exe: &std::path::Path, args: &[String]) -> Result<(), String> {
    let status = std::process::Command::new("sudo")
        .arg(exe)
        .args(args)
        .status()
        .map_err(|e| format!("Failed to run sudo: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err("Elevated update failed".to_string())
    }
}
