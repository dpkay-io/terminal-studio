mod app;
mod keybindings;
mod pane_tree;
mod pty;
mod renderer;
mod search;
mod shortcuts;
mod single_instance;
mod sys_monitor;
mod terminal;
mod theme;
mod updater;
mod url_detector;
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

fn main() -> eframe::Result<()> {
    env_logger::init();
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

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_resizable(true)
            .with_decorations(false)
            .with_icon(std::sync::Arc::new(make_icon())),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        "Terminal Studio",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
