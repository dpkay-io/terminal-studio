use egui::Color32;
use serde::{Deserialize, Serialize};

// ── Type alias ───────────────────────────────────────────────────────────────

/// Unique identifier for a label definition.
pub(super) type LabelId = u32;

// ── Constants ────────────────────────────────────────────────────────────────

/// All custom label IDs start at this value. Built-in IDs are 0–99.
pub(super) const FIRST_CUSTOM_ID: LabelId = 100;

/// Icon options available when creating a custom label.
pub(super) const CUSTOM_ICON_PALETTE: &[&str] = &[
    "\u{25CF}",  // ● filled circle
    "\u{25A0}",  // ■ filled square
    "\u{25B2}",  // ▲ filled triangle
    "\u{2605}",  // ★ star
    "\u{2764}",  // ❤ heart
    "\u{26A1}",  // ⚡ lightning
    "\u{1F4CC}", // 📌 pin
    "\u{1F516}", // 🔖 bookmark
];

// ── Enums ────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) enum LabelCategory {
    Status,
    Intent,
    Priority,
    Custom,
}

// ── Structs ──────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct LabelDef {
    pub(super) id: LabelId,
    pub(super) name: String,
    pub(super) icon: String,
    pub(super) category: LabelCategory,
    /// Stored RGBA components (premultiplied) so we can round-trip through JSON
    /// without pulling in egui's serde feature.
    pub(super) color_rgba: [u8; 4],
    pub(super) builtin: bool,
}

impl LabelDef {
    pub(super) fn color(&self) -> Color32 {
        Color32::from_rgba_premultiplied(
            self.color_rgba[0],
            self.color_rgba[1],
            self.color_rgba[2],
            self.color_rgba[3],
        )
    }
}

// ── Registry ─────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct LabelRegistry {
    pub(super) labels: Vec<LabelDef>,
    pub(super) next_custom_id: LabelId,
}

impl Default for LabelRegistry {
    fn default() -> Self {
        Self {
            labels: builtin_labels(),
            next_custom_id: FIRST_CUSTOM_ID,
        }
    }
}

impl LabelRegistry {
    /// Look up a label by its ID.
    pub(super) fn get(&self, id: LabelId) -> Option<&LabelDef> {
        self.labels.iter().find(|l| l.id == id)
    }

    /// Return a reference to every registered label.
    pub(super) fn all(&self) -> &[LabelDef] {
        &self.labels
    }

    /// Add a new custom label. Returns the newly assigned ID.
    pub(super) fn add_custom(&mut self, name: String, icon: String, color: Color32) -> LabelId {
        let id = self.next_custom_id;
        self.next_custom_id += 1;
        let [r, g, b, a] = color.to_array();
        self.labels.push(LabelDef {
            id,
            name,
            icon,
            category: LabelCategory::Custom,
            color_rgba: [r, g, b, a],
            builtin: false,
        });
        id
    }

    #[allow(dead_code)]
    pub(super) fn remove_custom(&mut self, id: LabelId) -> bool {
        if let Some(pos) = self.labels.iter().position(|l| l.id == id && !l.builtin) {
            self.labels.remove(pos);
            true
        } else {
            false
        }
    }

    /// Remove every label ID from `pane_labels` that no longer exists in the
    /// registry. Call this after removing a custom label to keep pane state
    /// consistent.
    pub(super) fn strip_orphans(&self, pane_labels: &mut Vec<LabelId>) {
        pane_labels.retain(|id| self.get(*id).is_some());
    }

    /// Load from `labels.json` in the data directory, falling back to defaults.
    pub(super) fn load() -> Self {
        let Some(path) = crate::util::data_file("labels.json") else {
            return Self::default();
        };
        crate::util::safe_json_load::<Self>(&path)
            .map(|mut r| {
                // Ensure all built-in labels are present (handles upgrades).
                for builtin in builtin_labels() {
                    if r.get(builtin.id).is_none() {
                        r.labels.push(builtin);
                    }
                }
                r
            })
            .unwrap_or_default()
    }

    /// Persist to `labels.json` atomically.
    pub(super) fn save(&self) {
        let Some(path) = crate::util::data_file("labels.json") else {
            return;
        };
        match serde_json::to_string_pretty(self) {
            Ok(text) => {
                if let Err(e) = crate::util::atomic_write(&path, &text) {
                    log::error!("failed to save labels: {e}");
                }
            }
            Err(e) => log::error!("failed to serialize labels: {e}"),
        }
    }
}

// ── Built-in label definitions ────────────────────────────────────────────────

/// Returns the 12 built-in label definitions using semantic theme colors.
/// Colors are resolved at call-time from the active theme so they stay
/// consistent with the user's chosen palette.
pub(super) fn builtin_labels() -> Vec<LabelDef> {
    let t = crate::theme::active();

    let make = |id: LabelId,
                name: &str,
                icon: &str,
                category: LabelCategory,
                color: Color32|
     -> LabelDef {
        let [r, g, b, a] = color.to_array();
        LabelDef {
            id,
            name: name.to_owned(),
            icon: icon.to_owned(),
            category,
            color_rgba: [r, g, b, a],
            builtin: true,
        }
    };

    vec![
        // ── Status ───────────────────────────────────────────────────────────
        make(0, "Todo", "\u{2610}", LabelCategory::Status, t.text),
        make(
            1,
            "In Progress",
            "\u{25B6}",
            LabelCategory::Status,
            t.accent,
        ),
        make(2, "Done", "\u{2713}", LabelCategory::Status, t.success),
        make(3, "Blocked", "\u{2298}", LabelCategory::Status, t.error),
        // ── Intent ───────────────────────────────────────────────────────────
        make(
            4,
            "Read Later",
            "\u{21A9}",
            LabelCategory::Intent,
            t.accent_muted,
        ),
        make(
            5,
            "Reference",
            "\u{00A7}",
            LabelCategory::Intent,
            t.accent_muted,
        ),
        make(6, "Debug", "\u{1F527}", LabelCategory::Intent, t.warning),
        make(
            7,
            "Experiment",
            "\u{25C8}",
            LabelCategory::Intent,
            t.accent_strong,
        ),
        // ── Priority ─────────────────────────────────────────────────────────
        make(8, "Important", "\u{2691}", LabelCategory::Priority, t.error),
        make(
            9,
            "Confusing",
            "\u{2047}",
            LabelCategory::Priority,
            t.warning,
        ),
        make(10, "Watch", "\u{25C9}", LabelCategory::Priority, t.accent),
        make(
            11,
            "Fragile",
            "\u{25B3}",
            LabelCategory::Priority,
            t.warning,
        ),
    ]
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_have_twelve_builtin_labels() {
        let registry = LabelRegistry::default();
        assert_eq!(registry.labels.len(), 12);
        assert!(registry.labels.iter().all(|l| l.builtin));
    }

    #[test]
    fn default_next_custom_id_is_first_custom() {
        let registry = LabelRegistry::default();
        assert_eq!(registry.next_custom_id, FIRST_CUSTOM_ID);
    }

    #[test]
    fn get_returns_builtin_by_id() {
        let registry = LabelRegistry::default();
        let label = registry.get(0).expect("id 0 must exist");
        assert_eq!(label.name, "Todo");
        assert!(label.builtin);
    }

    #[test]
    fn get_returns_none_for_missing_id() {
        let registry = LabelRegistry::default();
        assert!(registry.get(FIRST_CUSTOM_ID).is_none());
    }

    #[test]
    fn add_custom_increments_id_and_stores_label() {
        let mut registry = LabelRegistry::default();
        let id = registry.add_custom(
            "My Label".to_owned(),
            "\u{25CF}".to_owned(),
            Color32::from_rgb(255, 0, 0),
        );
        assert_eq!(id, FIRST_CUSTOM_ID);
        assert_eq!(registry.next_custom_id, FIRST_CUSTOM_ID + 1);
        let label = registry.get(id).expect("custom label must exist");
        assert_eq!(label.name, "My Label");
        assert_eq!(label.category, LabelCategory::Custom);
        assert!(!label.builtin);
    }

    #[test]
    fn remove_custom_succeeds_and_returns_true() {
        let mut registry = LabelRegistry::default();
        let id = registry.add_custom(
            "Temp".to_owned(),
            "\u{25CF}".to_owned(),
            Color32::from_rgb(0, 255, 0),
        );
        assert!(registry.remove_custom(id));
        assert!(registry.get(id).is_none());
    }

    #[test]
    fn remove_builtin_returns_false() {
        let mut registry = LabelRegistry::default();
        assert!(!registry.remove_custom(0));
        assert!(registry.get(0).is_some());
    }

    #[test]
    fn strip_orphans_removes_deleted_ids() {
        let mut registry = LabelRegistry::default();
        let custom_id = registry.add_custom(
            "Gone".to_owned(),
            "\u{25CF}".to_owned(),
            Color32::from_rgb(0, 0, 255),
        );
        registry.remove_custom(custom_id);

        let mut pane_labels: Vec<LabelId> = vec![0, 2, custom_id];
        registry.strip_orphans(&mut pane_labels);
        assert_eq!(pane_labels, vec![0, 2]);
    }

    #[test]
    fn strip_orphans_keeps_valid_ids() {
        let registry = LabelRegistry::default();
        let mut pane_labels: Vec<LabelId> = vec![0, 1, 2, 3];
        registry.strip_orphans(&mut pane_labels);
        assert_eq!(pane_labels, vec![0, 1, 2, 3]);
    }

    #[test]
    fn serialization_roundtrip() {
        let mut registry = LabelRegistry::default();
        registry.add_custom(
            "Roundtrip".to_owned(),
            "\u{2605}".to_owned(),
            Color32::from_rgb(100, 150, 200),
        );
        let json = serde_json::to_string(&registry).unwrap();
        let restored: LabelRegistry = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.labels.len(), registry.labels.len());
        assert_eq!(restored.next_custom_id, registry.next_custom_id);
        let custom = restored.get(FIRST_CUSTOM_ID).unwrap();
        assert_eq!(custom.name, "Roundtrip");
    }

    #[test]
    fn custom_icon_palette_has_eight_entries() {
        assert_eq!(CUSTOM_ICON_PALETTE.len(), 8);
    }

    #[test]
    fn all_returns_all_labels() {
        let registry = LabelRegistry::default();
        assert_eq!(registry.all().len(), 12);
    }
}
