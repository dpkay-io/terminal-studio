//! Picking colours for new workspaces that stand apart from the ones in use.
//!
//! Every candidate (the presets first, then a wheel of hues in three tones)
//! is scored by its distance to the nearest colour already taken — the
//! store's and those picked earlier in the same call — measured in OKLab,
//! where equal distances look equally different. The farthest one wins, so
//! each new workspace gets the colour least like any other; presets win close
//! calls, being the familiar choices the dialogs show as swatches. Pure.

use std::sync::OnceLock;

/// The swatches the workspace dialogs offer, and the first candidates.
pub(super) const PRESET_COLORS: &[[u8; 3]] = &[
    [100, 140, 230], // blue
    [80, 200, 100],  // green
    [220, 120, 80],  // orange
    [200, 80, 160],  // pink
    [140, 100, 220], // purple
    [80, 200, 200],  // teal
    [220, 200, 60],  // yellow
    [200, 80, 80],   // red
];

/// Hues on the candidate wheel, evenly spaced (every 15°).
const WHEEL_HUES: usize = 24;

/// Saturation and brightness of each tone on the wheel: bright (like the
/// presets), deep and soft, so a hue already used can still give a clearly
/// different colour.
const WHEEL_TONES: [(f32, f32); 3] = [(0.62, 0.88), (0.72, 0.62), (0.38, 0.96)];

/// How much farther than the best wheel colour a preset may be beaten by and
/// still win: presets are the dialog's own swatches, so they are preferred
/// unless a wheel colour is clearly more distinct.
const PRESET_PREFERENCE: f32 = 1.15;

/// Hue step for the overflow colours used once every candidate is taken: the
/// golden ratio of a turn, so consecutive ones land far apart.
const OVERFLOW_HUE_STEP: f32 = 0.618_034;
const OVERFLOW_SATURATION: f32 = 0.6;
const OVERFLOW_VALUE: f32 = 0.85;

/// `count` colours for new workspaces, each as far as possible from `used`
/// and from the ones picked before it. Never returns a colour in `used`, nor
/// the same colour twice.
pub(super) fn pick_distinct_colors(used: &[[u8; 3]], count: usize) -> Vec<[u8; 3]> {
    let mut taken: Vec<Lab> = used.iter().map(|&c| Lab::from_rgb(c)).collect();
    let mut taken_rgb: Vec<[u8; 3]> = used.to_vec();
    let mut picked = Vec::with_capacity(count);
    for _ in 0..count {
        let colour = most_distinct(&taken, &taken_rgb);
        taken.push(Lab::from_rgb(colour));
        taken_rgb.push(colour);
        picked.push(colour);
    }
    picked
}

/// One colour for a new workspace, given the colours already in use.
pub(super) fn pick_distinct_color(used: &[[u8; 3]]) -> [u8; 3] {
    pick_distinct_colors(used, 1)
        .first()
        .copied()
        .unwrap_or(PRESET_COLORS[0])
}

/// The free candidate farthest from everything in `taken`; with nothing
/// taken, the first preset.
fn most_distinct(taken: &[Lab], taken_rgb: &[[u8; 3]]) -> [u8; 3] {
    let mut best: Option<([u8; 3], f32)> = None;
    for candidate in candidates() {
        if taken_rgb.contains(&candidate.rgb) {
            continue;
        }
        let nearest = taken
            .iter()
            .map(|t| t.distance(&candidate.lab))
            .fold(f32::INFINITY, f32::min);
        let score = if candidate.preset {
            nearest * PRESET_PREFERENCE
        } else {
            nearest
        };
        // Strictly greater: on a tie the earlier candidate (presets first,
        // then the wheel in order) keeps its place, so picks are stable.
        if best.is_none_or(|(_, s)| score > s) {
            best = Some((candidate.rgb, score));
        }
    }
    best.map(|(rgb, _)| rgb)
        .unwrap_or_else(|| overflow_color(taken_rgb))
}

/// Past every candidate (dozens of workspaces): the first overflow colour no
/// workspace uses, so a new one never starts with a colour the dialogs refuse.
fn overflow_color(taken_rgb: &[[u8; 3]]) -> [u8; 3] {
    (0..=taken_rgb.len())
        .map(|index| {
            let hue = (index as f32 * OVERFLOW_HUE_STEP).fract();
            hsv_to_rgb(hue, OVERFLOW_SATURATION, OVERFLOW_VALUE)
        })
        .find(|c| !taken_rgb.contains(c))
        .unwrap_or(PRESET_COLORS[0])
}

struct Candidate {
    rgb: [u8; 3],
    lab: Lab,
    preset: bool,
}

/// Presets, then the wheel tone by tone. Built once.
fn candidates() -> &'static [Candidate] {
    static CANDIDATES: OnceLock<Vec<Candidate>> = OnceLock::new();
    CANDIDATES.get_or_init(|| {
        let presets = PRESET_COLORS.iter().map(|&rgb| Candidate {
            rgb,
            lab: Lab::from_rgb(rgb),
            preset: true,
        });
        let wheel = WHEEL_TONES.iter().flat_map(|&(saturation, value)| {
            (0..WHEEL_HUES).map(move |i| {
                let rgb = hsv_to_rgb(i as f32 / WHEEL_HUES as f32, saturation, value);
                Candidate {
                    rgb,
                    lab: Lab::from_rgb(rgb),
                    preset: false,
                }
            })
        });
        presets.chain(wheel).collect()
    })
}

fn hsv_to_rgb(hue: f32, saturation: f32, value: f32) -> [u8; 3] {
    egui::ecolor::Hsva::new(hue, saturation, value, 1.0).to_srgb()
}

/// A colour in OKLab: lightness and two opponent axes, scaled so that
/// Euclidean distance tracks how different two colours look.
#[derive(Debug, Clone, Copy)]
struct Lab {
    l: f32,
    a: f32,
    b: f32,
}

impl Lab {
    fn from_rgb(rgb: [u8; 3]) -> Self {
        let [r, g, b] = rgb.map(srgb_to_linear);
        let l = 0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b;
        let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
        let s = 0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b;
        let (l, m, s) = (l.cbrt(), m.cbrt(), s.cbrt());
        Lab {
            l: 0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
            a: 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
            b: 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
        }
    }

    fn distance(&self, other: &Lab) -> f32 {
        let (dl, da, db) = (self.l - other.l, self.a - other.a, self.b - other.b);
        (dl * dl + da * da + db * db).sqrt()
    }
}

fn srgb_to_linear(channel: u8) -> f32 {
    let c = f32::from(channel) / 255.0;
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two colours closer than this in OKLab are easy to mistake for each
    /// other at a glance (the presets are all at least ~0.1 apart).
    const CLEARLY_DIFFERENT: f32 = 0.08;

    fn lab_distance(a: [u8; 3], b: [u8; 3]) -> f32 {
        Lab::from_rgb(a).distance(&Lab::from_rgb(b))
    }

    fn min_pairwise(colours: &[[u8; 3]]) -> f32 {
        let mut min = f32::INFINITY;
        for (i, a) in colours.iter().enumerate() {
            for b in &colours[i + 1..] {
                min = min.min(lab_distance(*a, *b));
            }
        }
        min
    }

    #[test]
    fn nothing_used_starts_with_the_first_preset() {
        assert_eq!(pick_distinct_color(&[]), PRESET_COLORS[0]);
    }

    #[test]
    fn a_batch_of_eight_is_clearly_different_from_each_other() {
        let picked = pick_distinct_colors(&[], 8);
        assert_eq!(picked.len(), 8);
        assert!(
            min_pairwise(&picked) >= CLEARLY_DIFFERENT,
            "closest pair only {} apart: {picked:?}",
            min_pairwise(&picked)
        );
    }

    #[test]
    fn picks_never_repeat_a_used_colour_or_each_other() {
        let used = vec![PRESET_COLORS[0], PRESET_COLORS[3], [10, 10, 10]];
        let picked = pick_distinct_colors(&used, 40);
        for (i, c) in picked.iter().enumerate() {
            assert!(!used.contains(c), "pick {i} is a used colour");
            assert!(!picked[i + 1..].contains(c), "pick {i} repeats");
        }
    }

    #[test]
    fn a_pick_stays_away_from_a_custom_colour_close_to_a_preset() {
        // A hand-picked blue a hair off the blue preset: the preset is free
        // but would look the same, so something else is chosen.
        let near_blue = [102, 142, 228];
        let picked = pick_distinct_color(&[near_blue]);
        assert!(lab_distance(picked, near_blue) >= CLEARLY_DIFFERENT);
    }

    #[test]
    fn each_pick_is_the_farthest_free_candidate() {
        let used = vec![PRESET_COLORS[1], PRESET_COLORS[6]];
        let picked = pick_distinct_color(&used);
        let nearest = |c: [u8; 3]| {
            used.iter()
                .map(|u| lab_distance(c, *u))
                .fold(f32::INFINITY, f32::min)
        };
        let best_wheel = candidates()
            .iter()
            .filter(|c| !c.preset && !used.contains(&c.rgb))
            .map(|c| nearest(c.rgb))
            .fold(0.0, f32::max);
        assert!(nearest(picked) * PRESET_PREFERENCE >= best_wheel);
    }

    #[test]
    fn every_preset_is_used_before_most_wheel_colours_when_starting_empty() {
        // Starting from nothing, the presets are distinct enough that most of
        // them come out first: familiar swatches before generated tones.
        let picked = pick_distinct_colors(&[], PRESET_COLORS.len());
        let presets = picked.iter().filter(|c| PRESET_COLORS.contains(c)).count();
        assert!(presets >= PRESET_COLORS.len() / 2, "{picked:?}");
    }

    #[test]
    fn picks_are_deterministic() {
        let used = vec![[30, 60, 90]];
        assert_eq!(
            pick_distinct_colors(&used, 5),
            pick_distinct_colors(&used, 5)
        );
    }

    #[test]
    fn past_every_candidate_still_gives_an_unused_colour() {
        let all: Vec<[u8; 3]> = candidates().iter().map(|c| c.rgb).collect();
        let picked = pick_distinct_colors(&all, 3);
        for (i, c) in picked.iter().enumerate() {
            assert!(!all.contains(c));
            assert!(!picked[i + 1..].contains(c));
        }
    }

    #[test]
    fn candidates_are_distinct() {
        let all: Vec<[u8; 3]> = candidates().iter().map(|c| c.rgb).collect();
        for (i, a) in all.iter().enumerate() {
            assert!(!all[i + 1..].contains(a), "candidate {i} repeats");
        }
    }

    #[test]
    fn oklab_matches_reference_values() {
        // White is L=1, a=b=0; black is all zero.
        let white = Lab::from_rgb([255, 255, 255]);
        assert!((white.l - 1.0).abs() < 1e-3);
        assert!(white.a.abs() < 1e-3 && white.b.abs() < 1e-3);
        let black = Lab::from_rgb([0, 0, 0]);
        assert!(black.l.abs() < 1e-6);
    }
}
