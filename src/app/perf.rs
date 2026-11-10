use std::sync::OnceLock;
use std::time::{Duration, Instant};

const REPORT_INTERVAL: Duration = Duration::from_secs(5);

static START: OnceLock<Instant> = OnceLock::new();

pub(crate) fn mark_start() {
    let _ = START.set(Instant::now());
}

pub(super) struct PerfMonitor {
    report_started: Instant,
    frames: u64,
    cpu_ms: Vec<f32>,
    first_frame_logged: bool,
}

impl PerfMonitor {
    pub(super) fn from_env() -> Option<Self> {
        enabled(std::env::var("TS_PERF").ok().as_deref()).then(|| Self {
            report_started: Instant::now(),
            frames: 0,
            cpu_ms: Vec::with_capacity(1_024),
            first_frame_logged: false,
        })
    }

    pub(super) fn record(&mut self, ctx: &egui::Context, frame: &eframe::Frame, now: Instant) {
        if !self.first_frame_logged {
            let started = START.get_or_init(Instant::now);
            log::info!(
                "TS_PERF time_to_first_frame_ms={:.2}",
                now.checked_duration_since(*started)
                    .unwrap_or_default()
                    .as_secs_f64()
                    * 1_000.0
            );
            self.first_frame_logged = true;
        }

        self.frames += 1;
        if let Some(cpu_seconds) = frame.info().cpu_usage {
            self.cpu_ms.push(cpu_seconds * 1_000.0);
        }

        let elapsed = now.duration_since(self.report_started);
        if elapsed < REPORT_INTERVAL {
            return;
        }

        let summary = summarize(&self.cpu_ms);
        let fps = self.frames as f64 / elapsed.as_secs_f64();
        let (atlas_fill, image_size, galley_count) = ctx.fonts(|fonts| {
            (
                fonts.font_atlas_fill_ratio(),
                fonts.font_image_size(),
                fonts.num_galleys_in_cache(),
            )
        });
        log::info!(
            "TS_PERF cpu_ms_p50={:.2} cpu_ms_p95={:.2} cpu_ms_max={:.2} fps={:.2} \
             font_atlas_fill_ratio={:.3} font_image_size={}x{} num_galleys_in_cache={}",
            summary.p50,
            summary.p95,
            summary.max,
            fps,
            atlas_fill,
            image_size[0],
            image_size[1],
            galley_count
        );

        self.report_started = now;
        self.frames = 0;
        self.cpu_ms.clear();
    }
}

#[derive(Debug, Default, PartialEq)]
struct Summary {
    p50: f32,
    p95: f32,
    max: f32,
}

fn summarize(samples: &[f32]) -> Summary {
    if samples.is_empty() {
        return Summary::default();
    }

    let mut sorted = samples.to_vec();
    sorted.sort_by(f32::total_cmp);
    Summary {
        p50: percentile(&sorted, 0.50),
        p95: percentile(&sorted, 0.95),
        max: *sorted.last().unwrap_or(&0.0),
    }
}

fn percentile(sorted: &[f32], quantile: f32) -> f32 {
    let index = ((sorted.len() as f32 * quantile).ceil() as usize)
        .saturating_sub(1)
        .min(sorted.len().saturating_sub(1));
    sorted.get(index).copied().unwrap_or_default()
}

fn enabled(value: Option<&str>) -> bool {
    value == Some("1")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perf_is_opt_in() {
        assert!(enabled(Some("1")));
        assert!(!enabled(None));
        assert!(!enabled(Some("0")));
        assert!(!enabled(Some("true")));
    }

    #[test]
    fn summary_handles_empty_samples() {
        assert_eq!(summarize(&[]), Summary::default());
    }

    #[test]
    fn summary_reports_nearest_rank_percentiles() {
        let samples: Vec<f32> = (1..=20).rev().map(|value| value as f32).collect();
        assert_eq!(
            summarize(&samples),
            Summary {
                p50: 10.0,
                p95: 19.0,
                max: 20.0,
            }
        );
    }
}
