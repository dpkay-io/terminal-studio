use std::fs;
use std::path::PathBuf;

const ZSH_ENV: &str = r#"# Terminal Studio — zsh shell integration
_ts_orig_zdotdir="${_TS_ORIG_ZDOTDIR:-$HOME}"
[[ -f "$_ts_orig_zdotdir/.zshenv" ]] && builtin source "$_ts_orig_zdotdir/.zshenv"
"#;

const ZSH_PROFILE: &str = r#"[[ -f "${_ts_orig_zdotdir:-$HOME}/.zprofile" ]] && builtin source "${_ts_orig_zdotdir:-$HOME}/.zprofile"
"#;

const ZSH_RC: &str = r#"builtin export ZDOTDIR="${_ts_orig_zdotdir:-$HOME}"
[[ -f "$ZDOTDIR/.zshrc" ]] && builtin source "$ZDOTDIR/.zshrc"
unset _ts_orig_zdotdir

_terminal_studio_osc7() {
  builtin printf '\e]7;file://%s%s\a' "$HOST" "$PWD"
}
precmd_functions+=(_terminal_studio_osc7)
"#;

const ZSH_LOGIN: &str = r#"[[ -f "${_ts_orig_zdotdir:-$HOME}/.zlogin" ]] && builtin source "${_ts_orig_zdotdir:-$HOME}/.zlogin"
"#;

fn integration_base() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var("APPDATA").ok().map(|base| {
            PathBuf::from(base)
                .join("terminal-studio")
                .join("shell-integration")
        })
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var("HOME").ok().map(|base| {
            PathBuf::from(base)
                .join(".config")
                .join("terminal-studio")
                .join("shell-integration")
        })
    }
}

pub fn ensure_zsh_integration() -> Option<PathBuf> {
    let dir = integration_base()?.join("zsh");
    if let Err(e) = fs::create_dir_all(&dir) {
        log::warn!("Failed to create zsh integration dir: {e}");
        return None;
    }

    let files: &[(&str, &str)] = &[
        (".zshenv", ZSH_ENV),
        (".zprofile", ZSH_PROFILE),
        (".zshrc", ZSH_RC),
        (".zlogin", ZSH_LOGIN),
    ];

    for &(name, content) in files {
        let path = dir.join(name);
        if let Err(e) = fs::write(&path, content) {
            log::warn!("Failed to write {name}: {e}");
            return None;
        }
    }

    Some(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ensure_zsh_integration_creates_files() {
        let dir = std::env::temp_dir().join("ts-test-zsh-integration");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let zsh_dir = dir.join("zsh");
        fs::create_dir_all(&zsh_dir).unwrap();

        for &(name, content) in &[
            (".zshenv", ZSH_ENV),
            (".zprofile", ZSH_PROFILE),
            (".zshrc", ZSH_RC),
            (".zlogin", ZSH_LOGIN),
        ] {
            fs::write(zsh_dir.join(name), content).unwrap();
        }

        assert!(zsh_dir.join(".zshenv").exists());
        assert!(zsh_dir.join(".zprofile").exists());
        assert!(zsh_dir.join(".zshrc").exists());
        assert!(zsh_dir.join(".zlogin").exists());

        let env_content = fs::read_to_string(zsh_dir.join(".zshenv")).unwrap();
        assert!(env_content.contains("_ts_orig_zdotdir"));

        let rc_content = fs::read_to_string(zsh_dir.join(".zshrc")).unwrap();
        assert!(rc_content.contains("precmd_functions"));
        assert!(rc_content.contains("_terminal_studio_osc7"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_ensure_zsh_integration_idempotent() {
        let result1 = ensure_zsh_integration();
        let result2 = ensure_zsh_integration();
        assert_eq!(result1, result2);
    }

    #[test]
    fn test_zsh_rc_sources_user_config_before_precmd() {
        let rc = ZSH_RC;
        let source_pos = rc.find("builtin source").unwrap_or(usize::MAX);
        let precmd_pos = rc.find("precmd_functions").unwrap_or(0);
        assert!(
            source_pos < precmd_pos,
            "user .zshrc must be sourced before precmd hook is added"
        );
    }

    #[test]
    fn test_zsh_env_restores_zdotdir() {
        assert!(ZSH_ENV.contains("_TS_ORIG_ZDOTDIR"));
        assert!(ZSH_ENV.contains(".zshenv"));
    }

    #[test]
    fn test_integration_base_returns_some() {
        let base = integration_base();
        assert!(base.is_some(), "integration_base should return a path");
    }
}
