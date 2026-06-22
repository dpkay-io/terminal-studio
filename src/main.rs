#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod file_search_worker;
mod git;
mod keybindings;
mod md_detector;
mod pane_tree;
mod pty;
mod renderer;
mod search;
mod search_worker;
mod shortcuts;
mod single_instance;
mod syntax;
mod sys_monitor;
mod terminal;
mod theme;
mod ui_kit;
mod updater;
mod url_detector;
pub(crate) mod util;
mod workspace;

// Generates a 32×32 "TS" icon using Catppuccin Mocha base + blue.
fn make_icon() -> egui::IconData {
    const S: usize = 32;
    let bg = [30u8, 30, 46, 255];
    let fg = [137u8, 180, 250, 255];
    let mut buf = vec![bg; S * S];

    macro_rules! fill {
        ($x0:expr, $x1:expr, $y0:expr, $y1:expr) => {
            for y in $y0..$y1 {
                for x in $x0..$x1 {
                    buf[y * S + x] = fg;
                }
            }
        };
    }

    // T: horizontal bar then vertical stem
    fill!(2, 13, 5, 8);
    fill!(6, 9, 8, 27);

    // S: top bar / top-left / middle / bottom-right / bottom bar
    fill!(16, 28, 5, 8);
    fill!(16, 19, 8, 15);
    fill!(16, 28, 14, 17);
    fill!(25, 28, 17, 24);
    fill!(16, 28, 24, 27);

    egui::IconData {
        rgba: buf.iter().flat_map(|p| p.iter().copied()).collect(),
        width: S as u32,
        height: S as u32,
    }
}

fn force_x11_if_needed() {
    #[cfg(target_os = "linux")]
    {
        let args: Vec<String> = std::env::args().collect();
        let force_x11 = args.iter().any(|a| a == "--x11");
        let is_wsl = std::env::var("WSL_DISTRO_NAME").is_ok();

        if force_x11 || is_wsl {
            std::env::remove_var("WAYLAND_DISPLAY");
            if is_wsl {
                log::info!("WSL detected — forcing X11 backend");
            } else {
                log::info!("--x11 flag set — forcing X11 backend");
            }
        }
    }
}

fn crash_log_path() -> Option<std::path::PathBuf> {
    crate::util::data_dir().map(|d| d.join("crash.log"))
}

fn write_crash_log(message: &str) {
    if let Some(path) = crash_log_path() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let timestamp = chrono_lite_now();
        let entry = format!("[{timestamp}] {message}\n");
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut f| std::io::Write::write_all(&mut f, entry.as_bytes()));
    }
}

fn chrono_lite_now() -> String {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let total_secs = d.as_secs();
    let time_of_day = total_secs % 86_400;
    let hours = time_of_day / 3600;
    let mins = (time_of_day % 3600) / 60;
    let s = time_of_day % 60;

    let days = (total_secs / 86_400) as i64;
    let (y, m, day) = civil_from_days(days);
    format!("{y:04}-{m:02}-{day:02}T{hours:02}:{mins:02}:{s:02}Z")
}

// Howard Hinnant's civil_from_days algorithm (public domain).
fn civil_from_days(days_since_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}

fn parse_renderer_arg() -> Option<eframe::Renderer> {
    for arg in std::env::args() {
        if let Some(val) = arg.strip_prefix("--renderer=") {
            return match val.to_ascii_lowercase().as_str() {
                "glow" | "gl" | "opengl" => Some(eframe::Renderer::Glow),
                "wgpu" => Some(eframe::Renderer::Wgpu),
                _ => None,
            };
        }
    }
    None
}

fn main() -> eframe::Result<()> {
    env_logger::init();

    std::panic::set_hook(Box::new(|info| {
        let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown panic".to_string()
        };
        let location = info
            .location()
            .map(|l| format!(" at {}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_default();
        log::error!("PANIC{location}: {payload}");
        eprintln!("Terminal Studio encountered an error{location}: {payload}");
        write_crash_log(&format!("PANIC{location}: {payload}"));
    }));

    if updater::handle_apply_update_flag() {
        std::process::exit(0);
    }

    force_x11_if_needed();
    updater::cleanup_old_binary();

    // Single-instance enforcement: exit early if another instance is running.
    // Pass --no-singleton to bypass (useful for development).
    let _singleton_guard = match single_instance::SingleInstanceGuard::try_acquire() {
        Ok(guard) => guard,
        Err(()) => {
            log::warn!("Another instance of Terminal Studio is already running. Exiting.");
            eprintln!("Terminal Studio is already running. Pass --no-singleton to override.");
            std::process::exit(1);
        }
    };

    let forced_renderer = parse_renderer_arg();

    let viewport = egui::ViewportBuilder::default()
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([theme::MIN_WINDOW_W, theme::MIN_WINDOW_H])
        .with_resizable(true)
        .with_decorations(false)
        .with_icon(std::sync::Arc::new(make_icon()));

    let forced_array;
    let renderers_to_try: &[eframe::Renderer] = match forced_renderer {
        Some(r) => {
            write_crash_log(&format!("renderer forced via CLI: {r:?}"));
            forced_array = [r];
            &forced_array
        }
        None => &[eframe::Renderer::Wgpu, eframe::Renderer::Glow],
    };

    let mut last_err: Option<eframe::Result<()>> = None;
    let mut all_panicked = true;

    for &renderer in renderers_to_try {
        write_crash_log(&format!(
            "starting v{} with {renderer:?} renderer",
            env!("CARGO_PKG_VERSION")
        ));

        let options = eframe::NativeOptions {
            viewport: viewport.clone(),
            renderer,
            ..Default::default()
        };

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            eframe::run_native(
                "Terminal Studio",
                options,
                Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
            )
        }));

        match result {
            Ok(Ok(())) => return Ok(()),
            Ok(Err(e)) => {
                all_panicked = false;
                let msg = format!("{renderer:?} renderer failed: {e}");
                log::error!("{msg}");
                write_crash_log(&msg);
                last_err = Some(Err(e));
            }
            Err(panic_payload) => {
                let panic_msg = panic_payload
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| panic_payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "unknown panic".into());
                let msg = format!("{renderer:?} renderer panicked: {panic_msg}");
                log::error!("{msg}");
                write_crash_log(&msg);
            }
        }
    }

    if all_panicked {
        let log_path = crash_log_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<unavailable>".into());
        eprintln!("Terminal Studio failed to start. See crash log: {log_path}");
        std::process::exit(1);
    }

    last_err.unwrap_or(Ok(()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_from_days_epoch() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }

    #[test]
    fn civil_from_days_leap_day() {
        // 2024-02-29 = day 19782
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
    }

    #[test]
    fn civil_from_days_end_of_year() {
        // 2023-12-31 = day 19722
        assert_eq!(civil_from_days(19_722), (2023, 12, 31));
    }

    #[test]
    fn civil_from_days_start_of_year() {
        // 2024-01-01 = day 19723
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
    }

    #[test]
    fn civil_from_days_mid_year() {
        // 2026-06-22 = day 20626
        assert_eq!(civil_from_days(20_626), (2026, 6, 22));
    }

    #[test]
    fn chrono_lite_now_format() {
        let ts = chrono_lite_now();
        // ISO 8601 format: YYYY-MM-DDTHH:MM:SSZ
        assert_eq!(ts.len(), 20);
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[7..8], "-");
        assert_eq!(&ts[10..11], "T");
        assert_eq!(&ts[13..14], ":");
        assert_eq!(&ts[16..17], ":");
        assert_eq!(&ts[19..20], "Z");
    }
}
