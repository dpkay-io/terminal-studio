use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use std::time::Duration;

use sysinfo::{Networks, System};

#[derive(Clone, Default)]
pub struct SystemStats {
    pub cpu_percent: f32,
    pub ram_percent: f32,
    /// Bytes received per second (summed across all interfaces).
    pub net_rx_per_sec: u64,
    /// Bytes transmitted per second (summed across all interfaces).
    pub net_tx_per_sec: u64,
}

pub struct SysMonitor {
    stats: Arc<Mutex<SystemStats>>,
    alive: Arc<AtomicBool>,
}

impl SysMonitor {
    pub fn spawn(ctx: egui::Context, interval: Duration) -> Option<Self> {
        let stats: Arc<Mutex<SystemStats>> = Arc::default();
        let shared = stats.clone();
        let alive = Arc::new(AtomicBool::new(true));
        let alive_bg = alive.clone();

        if let Err(e) = std::thread::Builder::new()
            .name("sys-monitor".into())
            .spawn(move || poll_loop(shared, ctx, interval, alive_bg))
        {
            log::error!("failed to spawn sys-monitor thread: {e}");
            return None;
        }

        Some(Self { stats, alive })
    }

    pub fn stats(&self) -> SystemStats {
        self.stats.lock().clone()
    }
}

impl Drop for SysMonitor {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Relaxed);
    }
}

fn poll_loop(
    shared: Arc<Mutex<SystemStats>>,
    ctx: egui::Context,
    interval: Duration,
    alive: Arc<AtomicBool>,
) {
    let mut sys = System::new();
    let mut nets = Networks::new_with_refreshed_list();

    // Initial CPU sample — sysinfo needs two calls to compute delta.
    sys.refresh_cpu_usage();
    if !sleep_interruptible(interval, &alive) {
        return;
    }

    while alive.load(Ordering::Relaxed) {
        sys.refresh_cpu_usage();
        sys.refresh_memory();

        let prev_rx: u64 = nets.values().map(|n| n.total_received()).sum();
        let prev_tx: u64 = nets.values().map(|n| n.total_transmitted()).sum();

        nets.refresh(false);

        let cur_rx: u64 = nets.values().map(|n| n.total_received()).sum();
        let cur_tx: u64 = nets.values().map(|n| n.total_transmitted()).sum();

        let secs = interval.as_secs_f64();
        let rx_per_sec = ((cur_rx.saturating_sub(prev_rx)) as f64 / secs) as u64;
        let tx_per_sec = ((cur_tx.saturating_sub(prev_tx)) as f64 / secs) as u64;

        let cpu = sys.global_cpu_usage();
        let ram = if sys.total_memory() > 0 {
            (sys.used_memory() as f64 / sys.total_memory() as f64 * 100.0) as f32
        } else {
            0.0
        };

        {
            let mut s = shared.lock();
            s.cpu_percent = cpu;
            s.ram_percent = ram;
            s.net_rx_per_sec = rx_per_sec;
            s.net_tx_per_sec = tx_per_sec;
        }

        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        if !sleep_interruptible(interval, &alive) {
            break;
        }
    }
}

fn sleep_interruptible(duration: Duration, alive: &AtomicBool) -> bool {
    let step = Duration::from_millis(250);
    let mut remaining = duration;
    while remaining > Duration::ZERO {
        if !alive.load(Ordering::Relaxed) {
            return false;
        }
        let sleep_time = remaining.min(step);
        std::thread::sleep(sleep_time);
        remaining = remaining.saturating_sub(sleep_time);
    }
    alive.load(Ordering::Relaxed)
}

pub fn format_bytes_rate(bps: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * 1024;
    if bps >= MB {
        format!("{:.1}M", bps as f64 / MB as f64)
    } else if bps >= KB {
        format!("{:.0}K", bps as f64 / KB as f64)
    } else {
        format!("{}B", bps)
    }
}
