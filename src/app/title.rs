use crate::pty::foreground::ForegroundProcess;
use crate::pty::ShellKind;

pub(super) fn display_title(title: &str) -> String {
    let t = title.trim();
    let looks_like_path = t.starts_with('/')
        || t.starts_with('~')
        || (t.len() >= 3
            && t.chars()
                .next()
                .map(|c| c.is_ascii_alphabetic())
                .unwrap_or(false)
            && &t[1..3] == ":\\");
    if looks_like_path {
        t.split(['/', '\\'])
            .rfind(|s| !s.is_empty())
            .unwrap_or(t)
            .to_string()
    } else {
        t.to_string()
    }
}

pub(super) fn shell_escape_arg(s: &str) -> String {
    let safe = !s.is_empty()
        && s.chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':' | '@' | '='));
    if safe {
        return s.to_string();
    }
    #[cfg(target_os = "windows")]
    {
        format!("\"{}\"", s.replace('"', "\"\""))
    }
    #[cfg(not(target_os = "windows"))]
    {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

pub(super) fn effective_title(
    title: &str,
    cwd: &std::path::Path,
    fg: Option<&ForegroundProcess>,
    shell: Option<&ShellKind>,
    workspace_name: Option<&str>,
) -> String {
    if let Some(fp) = fg {
        let name = fp.name.strip_suffix(".exe").unwrap_or(&fp.name);
        if fp.cmdline.len() > 1 {
            let arg = fp.cmdline[1..].iter().find(|a| !a.starts_with('-'));
            if let Some(a) = arg {
                let short = std::path::Path::new(a.as_str())
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(a);
                return format!("{name} {short}");
            }
        }
        return name.to_string();
    }

    let t = title.trim();
    let tl = t.to_lowercase();
    let is_shell_default = t.is_empty()
        || tl.starts_with("session ")
        || tl == "powershell.exe"
        || tl == "windows powershell"
        || tl == "cmd.exe"
        || tl == "bash"
        || tl == "zsh"
        || tl == "sh"
        || tl == "fish";

    if !is_shell_default {
        return display_title(title);
    }

    if let Some(ws) = workspace_name.filter(|s| !s.is_empty()) {
        return ws.to_string();
    }

    if let Some(dir) = cwd
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|s| !s.is_empty())
    {
        return dir.to_string();
    }

    if let Some(sk) = shell {
        return sk.display_name().to_string();
    }

    t.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn display_title_plain_text_unchanged() {
        assert_eq!(display_title("  vim  "), "vim");
    }

    #[test]
    fn display_title_unix_path_returns_last_segment() {
        assert_eq!(display_title("/home/user/projects/myapp"), "myapp");
    }

    #[test]
    fn display_title_tilde_path_returns_last_segment() {
        assert_eq!(display_title("~/projects/myapp"), "myapp");
    }

    #[test]
    fn display_title_windows_path_returns_last_segment() {
        assert_eq!(display_title("C:\\Users\\testuser\\proj"), "proj");
    }

    #[test]
    fn effective_title_shell_default_uses_cwd() {
        let cwd = Path::new("/home/user/myproject");
        assert_eq!(effective_title("bash", cwd, None, None, None), "myproject");
        assert_eq!(
            effective_title("Session 1", cwd, None, None, None),
            "myproject"
        );
        assert_eq!(
            effective_title("PowerShell.exe", cwd, None, None, None),
            "myproject"
        );
        assert_eq!(effective_title("", cwd, None, None, None), "myproject");
    }

    #[test]
    fn effective_title_real_title_uses_display_title() {
        let cwd = Path::new("/home/user/myproject");
        assert_eq!(
            effective_title("vim README.md", cwd, None, None, None),
            "vim README.md"
        );
    }

    #[test]
    fn effective_title_real_title_strips_path() {
        let cwd = Path::new("/home/user");
        assert_eq!(
            effective_title("/home/user/projects/src", cwd, None, None, None),
            "src"
        );
    }

    #[test]
    fn effective_title_empty_cwd_falls_back_to_title() {
        let cwd = Path::new("");
        let result = effective_title("Session 1", cwd, None, None, None);
        assert_eq!(result, "Session 1");
    }

    #[test]
    fn effective_title_empty_cwd_falls_back_to_shell_name() {
        let cwd = Path::new("");
        let result = effective_title("", cwd, None, Some(&ShellKind::PowerShell), None);
        assert_eq!(result, "PowerShell");
    }

    #[test]
    fn effective_title_foreground_process_wins() {
        let cwd = Path::new("/home/user/myproject");
        let fg = ForegroundProcess {
            name: "vim".to_string(),
            cmdline: vec!["vim".to_string(), "README.md".to_string()],
        };
        assert_eq!(
            effective_title("bash", cwd, Some(&fg), None, None),
            "vim README.md"
        );
    }

    #[test]
    fn effective_title_foreground_process_no_args() {
        let cwd = Path::new("/home/user");
        let fg = ForegroundProcess {
            name: "htop".to_string(),
            cmdline: vec!["htop".to_string()],
        };
        assert_eq!(effective_title("bash", cwd, Some(&fg), None, None), "htop");
    }

    #[test]
    fn effective_title_foreground_strips_exe_suffix() {
        let cwd = Path::new("/home/user");
        let fg = ForegroundProcess {
            name: "node.exe".to_string(),
            cmdline: vec!["node.exe".to_string(), "server.js".to_string()],
        };
        assert_eq!(
            effective_title("bash", cwd, Some(&fg), None, None),
            "node server.js"
        );
    }

    #[test]
    fn effective_title_foreground_shortens_path_arg() {
        let cwd = Path::new("/home/user");
        let fg = ForegroundProcess {
            name: "vim".to_string(),
            cmdline: vec![
                "vim".to_string(),
                "/home/user/projects/README.md".to_string(),
            ],
        };
        assert_eq!(
            effective_title("bash", cwd, Some(&fg), None, None),
            "vim README.md"
        );
    }

    #[test]
    fn effective_title_workspace_name_overrides_cwd() {
        let cwd = Path::new("/home/user/myproject");
        assert_eq!(
            effective_title("bash", cwd, None, None, Some("My Project")),
            "My Project"
        );
    }

    #[test]
    fn effective_title_workspace_name_ignored_when_real_title() {
        let cwd = Path::new("/home/user/myproject");
        assert_eq!(
            effective_title("vim README.md", cwd, None, None, Some("My Project")),
            "vim README.md"
        );
    }

    #[test]
    fn effective_title_workspace_name_ignored_when_foreground() {
        let cwd = Path::new("/home/user/myproject");
        let fg = ForegroundProcess {
            name: "cargo".to_string(),
            cmdline: vec!["cargo".to_string(), "build".to_string()],
        };
        assert_eq!(
            effective_title("bash", cwd, Some(&fg), None, Some("My Project")),
            "cargo build"
        );
    }

    #[test]
    fn effective_title_empty_workspace_name_falls_through_to_cwd() {
        let cwd = Path::new("/home/user/myproject");
        assert_eq!(
            effective_title("bash", cwd, None, None, Some("")),
            "myproject"
        );
    }
}
