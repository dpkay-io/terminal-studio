# UI Cleanup (Issue #36) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Terminal Studio read as one coherent product — two bundled font families, four text sizes, one color-role set per theme, one selected style, Lucide icons, and restyled Quick Switcher / Settings / Shortcuts / dialogs — without moving any panel.

**Architecture:** Foundations first (theme roles, type scale, fonts, icons, `ui_kit` primitives), each with unit tests; then each screen is migrated onto those primitives in its own task. All visual values flow from `crate::theme`; all reusable widgets live in `crate::ui_kit`.

**Tech Stack:** Rust, egui/eframe 0.28, `include_bytes!` fonts (Inter, JetBrains Mono, Lucide subset), `ttf-parser` (dev-dependency, tests only).

**Spec:** `docs/superpowers/specs/2026-11-10-ui-cleanup-design.md` (read it alongside this plan; mockups are attached to issue #36).

## Global Constraints

- No panel moves, no new panels; every existing title-bar control stays.
- All colors and sizes come from `theme.rs` tokens — no `Color32::from_rgb` or literal px in UI modules.
- Type scale is exactly 11 / 12 / 13 / 14; nothing in UI chrome below 11. Larger sizes only in the markdown preview.
- Mono (JetBrains Mono) only for terminal and kbd chips; everything else (paths, tree, Git Diff list, Notes, breadcrumb) uses Inter.
- Nord roles use the spec's exact hex values; every other theme uses the spec's recipe and signature-accent table.
- Contrast: `fg_muted` ≥ 4.5:1 on `bg_base` and `bg_panel`; `fg` ≥ 4.5:1 on `bg_raised`; `danger_text` ≥ 4.5:1 on `danger`; `fg_muted_raised` ≥ 4.5:1 on `bg_raised` (muted text inside modals) — for all 16 themes.
- Selected list item = `accent_bg` fill, radius `R_MD`, no bar/outline. Selected tab = 2px `accent` bottom bar, `fg_strong` text, no outline/fill.
- Dialog buttons 34px high, right-aligned in a bordered footer, order Cancel → primary/destructive.
- Bundled font payload ≈ 1 MB total. Fonts registered once at startup; nothing font- or theme-related recomputed per frame.
- No panics, no blocking work on the UI thread (CLAUDE.md). Files stay single-responsibility.
- Per the user's global rules: **do not commit without explicit user approval**, never commit to `master`, and no AI attribution in commits or code.
- `cargo test`, `cargo clippy --all-targets -- -D warnings`, and `cargo fmt --check` pass at the end of every task.

## Review Focus

1. **Light themes (Solarized Light, Catppuccin Latte, GitHub Light):** panel/raised/accent_bg must still be distinguishable and text readable — pinned by Task 1's contrast test and Task 1's `light_theme_panel_differs_from_base` test.
2. **Very long names** (session, workspace, branch, theme, path) at minimum window width (640px): must truncate with `…` and tooltip, never overlap neighbours or wrap — pinned by Task 5's `truncate_*` tests.
3. **User keybinding overrides** in an existing `keybindings.json` (e.g. user bound `Ctrl+Shift+R` to reopen): must survive the default changes — pinned by Task 10's `user_override_survives_default_change`.
4. **Glyph not in the Lucide subset** (someone adds an icon constant but forgets to regenerate the font): must be caught in CI, not render as tofu — pinned by Task 4's `every_icon_constant_exists_in_font`.
5. **Theme switched at runtime** while dialogs are open: visuals and roles update next frame without restart — pinned by Task 1's `visuals_for` test being the only path to `set_visuals` (both existing call sites in `app/state.rs` replaced).

## Performance Requirements

Verified against the code during plan review; each is owned by a task below.

- **P1 (Task 3) One cell-metrics source.** `app.rs:2581-2589` (`cached_cell_size`, PTY resize) and `terminal_pass.rs:123` (render) compute cell height independently — changing only one desyncs PTY rows from rendered rows. Add `renderer::terminal_cell_metrics(fonts: &Fonts, font_size: f32) -> CellMetrics { w: f32, h: f32, glyph_y_off: f32 }` with `h = (font_size * TERM_LINE_HEIGHT).round().max(row_height)` (not `row_height × 1.3` — JetBrains Mono's `row_height` is already ≈1.32× size). Both sites use it; `App.cached_cell_size` keyed by `(font_size, pixels_per_point)`. Tests: `pty_rows_equal_render_rows`, `cell_h_is_font_size_times_line_height`.
- **P2 (Task 9) Git status parsed once, on the worker.** Today `git_diff.rs:112` parses `git status` every frame, `app.rs:1438` clones the status String and unpushed Vec every frame, `app.rs:1666` counts lines every frame. Add `GitStatusSnapshot { entries: Vec<GitFileStatus>, modified: HashSet<PathBuf>, change_count: usize }` built in `run_git_info` on the git worker; `DirData.git_status: Arc<GitStatusSnapshot>`; `PanelSnap` does `Arc::clone`; `render_git_diff` takes `&GitStatusSnapshot`; optimistic overrides applied at lookup. Wire every copy site: `watcher.rs:122` (DirAdded), `watcher.rs:190`, `apply_git_result`, `PanelSnap` (`app.rs:1411-1497`). Absolute paths built from `util::find_git_root(dir)` (same string form as `list_dir_entries`) — never `canonicalize` on the UI thread. Checks: `rg "parse_git_status\(" src/app/ui src/app/git_diff.rs` → none; test `modified_paths_match_list_dir_entries_paths` (temp repo).
- **P3 (Task 9) File tree culls off-screen rows.** `render_dir_tree` paints every expanded row every frame with a `format!` per row. Each row: `allocate_exact_size`, paint only if `ui.is_rect_visible(rect)` (reveal-target logic still runs for all); chevron/icon via `painter.text` with `&'static str`; name = one galley with `TextWrapping::truncate_at_width`; badge lookup `!modified.is_empty() && modified.contains(&entry.path)`; no per-row `format!`/`to_string()`. Test: `tree_lays_out_only_visible_rows` (5 000 entries, 600px viewport, galley-cache count < 4 × (visible + 10)).
- **P4 (Task 5) Truncation is one layout.** Do not binary-search prefixes. `truncate_label` wraps `egui::Label::new(..).truncate()` (egui 0.28 elides and shows full-text tooltip only when elided); painter paths use `truncate_galley(painter, text, font, color, max_w) -> Arc<Galley>` built from a `LayoutJob` with `TextWrapping::truncate_at_width(max_w)`. `truncate_to_width` is dropped. Tooltip text never built eagerly. Tests: `truncate_label_single_layout`, `truncate_sets_elided_flag`.
- **P5 (Tasks 3–4) Font chains.** Lucide uses Private Use Area codepoints that collide with Powerline / Nerd Font prompt glyphs, so it is **not** in Proportional/Monospace. `Proportional = [Inter, symbol_fallback, egui defaults…]`, `Monospace = [JetBrains Mono, symbol_fallback, egui defaults…]`, `mono_bold = [JetBrains Mono Bold, …same fallbacks]`, `icons = [lucide, Inter]`. Fonts use `FontData::from_static(include_bytes!(…))`; `install_fonts` called only from `App::new` (grep: one `set_fonts` site). Do not subset JetBrains Mono (keep Braille, arrows, geometric shapes, Greek/Cyrillic); Inter may be subset. Icons only at 11/12/13/14.
- **P6 (Task 3) Bold and box drawing.** `terminal_pass.rs` draws bold twice offset 1px — replace with the `mono_bold` family and delete the double draw. At 1.3 line height, box-drawing glyphs won't join: draw U+2500–257F and U+2580–259F as line/rect shapes in `terminal_pass` (`fn draw_box_char(painter, ch, cell: Rect, color) -> bool`). Tests: `box_char_vertical_spans_full_cell`, `non_box_char_returns_false`.
- **P7 (Task 11) Font-size control.** `settings_overlay.rs:279` `DragValue.speed(0.1)` over 8–28 creates up to 200 font sizes (each fills the glyph atlas; >80% full triggers a full atlas rebuild) and saves to disk every tick on the UI thread. Change to whole-point steps (value rounded), save via the existing `ui/debounce.rs` `Debouncer` or on `drag_stopped`.
- **P8 (Task 9) Notes save off the UI thread.** `NoteStore::set` does `atomic_write` on the UI thread on every changed frame (`app.rs:2336`, `app.rs:4412`, `workspace.rs:136`). Notes save via `Debouncer` + background writer that reports completion; the "Saving…/Autosaved" label reads that state with no repaint loop.
- **P9 (Task 12) Quick Switcher model.** `dialogs.rs:328-420` rebuilds every frame with `format!` + fuzzy match per pane, and `switcher_collect_entries` takes a blocking `session.read()` per pane. Build the model only on open / query change, use `try_read`. Row titles use existing `ShellKind::display_name()` (`pty/mod.rs:49`) via `SessionEntry.shell`.
- **P10 (Task 1) Visuals set once per theme change.** Startup visuals (`state.rs:66-101`) run before `AppSettings::load()` (`state.rs:169`), so a saved non-default theme gets Mocha's visuals — move after load. `App.applied_theme: Option<ThemeId>`; `set_visuals`/`set_style` only when it changes. Grep: exactly one `set_visuals` site. `derive_roles` private, only called from `ThemeDef::build` (themes built once in `OnceLock`).
- **P11a (Task 5) One list-row widget.** Existing `ui_kit::list_item` (`lists.rs:5`, used by `command_palette.rs:182,654`) is reimplemented on top of `selectable_row` so the command palette gets the same selected style.
- **P11 (Task 5) No per-row animations.** egui's `AnimationManager` never frees entries; `ui_kit::list_item` (`lists.rs:21-26`) animates per row ID. `selectable_row` uses instant hover/selected fills. Toggle/segmented may animate (they stop repainting when done).
- **P12 (Tasks 8, 10, 12) No per-frame contrast fixing.** Uncached `ensure_readable` calls in `dialogs.rs`, `session_list.rs`, `settings_overlay.rs`, `app.rs:1908` are replaced by role colors (guaranteed by the contrast test); user-chosen label colors get the same per-frame cache `ensure_term_contrast` has. Shortcut groups and chip strings cached in `ShortcutRegistry` (like `labels`); title-bar chip reads `label_for(OpenQuickSwitcher)`.

**Before/after measurement (Task 0 and Task 13):**
- Add opt-in `src/app/perf.rs` (`TS_PERF=1`): records `frame.info().cpu_usage` per frame; every 5s logs p50/p95/max CPU ms, fps, `font_atlas_fill_ratio`, `font_image_size`, `num_galleys_in_cache`; logs time-to-first-frame from a `START: OnceLock<Instant>` set in `main`.
- Scenarios, run on `master` and the branch: 30s idle (fps must not rise); `dir /s C:\Windows` for 10s; right panel on this repo with `src/` expanded + ~200 untracked files; Git Diff tab open; cycle all 16 themes; drag font size; Quick Switcher open while a terminal streams output.
- System numbers: RSS (`Get-Process terminal-studio | select WorkingSet64,PrivateMemorySize64`), release binary size, median of 5 time-to-first-frame. Branch must be ≤ master on frame time p95 and idle fps; binary ≤ master + 1.5 MB.
- Deterministic tests (Task 5 harness): `atlas_stable_after_warmup`, `install_fonts_idempotent`, plus P1/P3/P4 tests.

## Existing Tests

**Baseline (2026-11-10, `feat/ui-cleanup-36` @ 52a11d6):** `cargo test` → **671 passed, 0 failed**. Every task must end with ≥ 671 + (new tests added so far) − (tests explicitly removed below) passing. No existing test may be deleted, `#[ignore]`d or have its assertions weakened unless it is listed here.

| Existing tests | Task | What happens |
|---|---|---|
| `theme.rs` tests (28: tinted, contrast, ensure_readable, srgb LUT, lerp, theme switching, all-theme validity, ANSI contrast) | 1 | Move with the code into `src/theme/mod.rs` (or the submodule owning the function) **unchanged**. `test_all_theme_ids_produce_valid_themes` and `test_ensure_term_contrast_all_themes_ansi_on_same` automatically cover GitHub Light — it must pass them, not be excluded. |
| `test_header_bg_active_with_color`, `test_header_bg_no_color`, `test_header_bg_midpoint_no_overflow` | 6 | `header_bg()` (workspace-tinted header) has no production callers today and the spec drops workspace-tinted headers → **delete `header_bg` and these 3 tests** in Task 6, noted in the commit message. |
| `shortcuts.rs` tests (`labels exist for main actions`, `groups_cover_main_actions`, parsing tests) | 10 | Keep unchanged; they must still pass after the default rebinding. `SearchAllSessions` stays in the label list. |
| `keybindings.rs` — **no tests today** (CLAUDE.md's 6 is stale) | 10 | Task 10 creates the module. Effective defaults are `KeybindingsConfig::default_bindings` (`keybindings.rs:48-453`); `shortcuts.rs:512-659` is only a fallback (and lacks `RevealInExplorer`). Tests call the two `default_bindings()` directly — never `ShortcutRegistry::new()`, which reads the developer's real `keybindings.json`. Add `KeybindingsConfig::from_json(&str) -> Self` for the override test. |
| `renderer/terminal_pass.rs` tests (geometry `to_cell_*`, selection, ANSI) | 3 | Keep unchanged — they construct `TerminalGeometry` with an explicit `cell_h`, so line height doesn't affect them. Add `cell_height_applies_line_height` (cell_h == round(row_height × 1.3)) and `to_cell_uses_scaled_cell_h`. |
| `terminal/tests.rs` (11 emulator tests) | 3 | Untouched; must pass. |
| `app/file_browser.rs` — **no tests today** (CLAUDE.md's 10 is stale) | 9 | Task 9 creates the `#[cfg(test)]` module. |
| `app/watcher.rs` tests (9) | 9 | Keep. Tests at ~650/666/691/714 set `git_status` directly — update their literals to the new `Arc<GitStatusSnapshot>` type only. Add the snapshot-propagation assertions to `apply_git_result_populates_data` (~733) and `dir_added_after_apply_preserves_git_data` (~760). Update the `DirData` literal in the test helper (~633). |
| `git/parser.rs` tests (17) | 9 | Keep unchanged; new `modified_paths_*` tests are added next to them. |
| `app/settings.rs` tests (6, incl. theme persistence) | 1, 11 | Keep. Add `unknown_theme_id_falls_back_to_default` and `github_light_roundtrips` in Task 1 (key is `theme_id`; serde_json round-trip only). |
| `app/ui/command_palette.rs` tests (6) | 10, 12 | Keep; if a test asserts a palette label/shortcut string that changes (Reopen/Search all sessions), update only that expected string. |
| `app/feedback.rs` tests (7) | 1 | Keep; `flash_bg` etc. still exist (now derived from roles). |
| `app/markdown.rs` tests (15) | 2 | Keep; markdown preview retains `FONT_HEADING_1/2`. |

**UI files with zero tests today** — `dialogs.rs`, `settings_overlay.rs`, `titlebar.rs`, `tab_bar.rs`, `left_panel/*`, all of `ui_kit/*`, `git_diff.rs`. This plan adds tests to each by extracting pure helpers (`titlebar_fill`, `sysmon_rect`, `tab_visual`, `session_row_style`, `tree_row_layout`, `switcher_row_title`, `quit_summary`, `row_hint`, `grid_columns`, `truncate_galley`, …) and by headless `egui::Context::run` widget tests in `ui_kit`. Painting code itself stays manually verified via `cargo run`.

### Test infrastructure (built in Tasks 1 and 5, used by everything after)

1. **Global theme race.** `theme::set_theme` mutates a process-wide `ACTIVE_IDX`; `cargo test` runs tests in parallel, and existing `test_theme_switching` / `test_all_theme_ids_produce_valid_themes` already flip it. Any new test that reads `theme::active()` would be flaky. Fix (Task 1): add `#[cfg(test)] pub(crate) mod test_support` in `src/theme/` with `pub fn lock_theme(id: ThemeId) -> ThemeGuard` (holds a `static parking_lot::Mutex<()>`, sets the theme, restores `CatppuccinMocha` on drop). **Every** existing test that calls `set_theme` takes the guard as its first line (only change to them): `theme.rs` tests at ~1489, 1496, 1504, 1518, 1531, 1540, 1552, 1587, 1596, 1616, 1627, 1654, 1699, 1720 and `app/feedback.rs:134` — grep `rg "set_theme\(" src` inside `#[cfg(test)]` to catch all. All `ui_kit` widget tests take it too (widgets read `theme::active()`). Every new test that touches the active theme uses it. Prefer helpers that take `t: &Theme` explicitly so most tests need no global at all.
2. **No headless egui harness exists** (no test in the repo builds a `RawInput`). Add (Task 5) `#[cfg(test)] src/ui_kit/test_harness.rs`:
   - `pub fn ctx_with_fonts() -> egui::Context` — calls `fonts::install_fonts` and runs one warm-up frame so text measuring works.
   - `pub fn run_ui(ctx: &Context, input: RawInput, f: impl FnMut(&mut Ui)) -> FullOutput` — renders `f` inside a fixed 800×600 `CentralPanel`.
   - `pub fn click_at(ctx: &Context, pos: Pos2, f: impl FnMut(&mut Ui)) -> FullOutput` — frame 1 `PointerMoved`, frame 2 `PointerButton{pressed:true}`, frame 3 `pressed:false`.
   - `pub fn filled_rects(out: &FullOutput) -> Vec<(Rect, Color32)>` — walks `out.shapes` (including nested `Shape::Vec`) for `Shape::Rect` fills.
   All `ui_kit` widget tests in Task 5 and the screen tests that need a frame use only these four functions.
3. **Test helpers named in this plan** live in `theme::test_support`: `fn theme_by_id(id: ThemeId) -> &'static Theme` (indexes `all_themes()`, no global mutation), `fn hex(rgb: u32) -> Color32`, `fn ratio(a: Color32, b: Color32) -> f32` (wraps the existing private `contrast_ratio([u8;3],[u8;3])`; alpha ignored — `accent_bg` contrast is checked after compositing onto `bg_panel`).
4. **Binary assets in git.** Add `.gitattributes`: `*.ttf binary`. Fonts are loaded with `include_bytes!`, so CI (`.github/workflows/ci.yml` runs `cargo test`, `cargo clippy --all-targets -D warnings`, `cargo fmt --check` on push) fails to compile if a font file isn't committed — the font files must be added in the same commit as the code that includes them.
5. **Coverage.** The repo has no coverage tool. Task 13 runs `cargo llvm-cov --bin terminal-studio --summary-only` (no `src/lib.rs` exists; install `cargo-llvm-cov` locally; not added to CI) and reports line coverage for every new/changed non-painting module (`theme/*`, `fonts.rs`, `ui_kit/*`, `git/parser.rs`, extracted helpers). Target 100% for those; any uncovered line gets a test or is listed in the PR description with the reason (e.g. GPU-only paint path).

**Each task's "Run" step means the full `cargo test`**, not just the new filter — the filtered run is for the red/green cycle; the full run is the gate.

---

## File Map

| File | Change |
|------|--------|
| `src/theme.rs` → `src/theme/mod.rs` | becomes a module dir; keeps public API (`active`, `set_theme`, tokens) |
| `src/theme/palettes.rs` (new) | `all_defs()` palette data moved out of mod.rs, + GitHub Light, + `accent` per def |
| `src/theme/roles.rs` (new) | role derivation recipe (`derive_roles`) + contrast adjustment |
| `src/theme/visuals.rs` (new) | `visuals_for(&Theme) -> egui::Visuals` (single source; replaces duplicated code in `app/state.rs`) |
| `src/fonts.rs` (new) | `install_fonts(ctx)`; family name constants |
| `assets/fonts/` (new) | `Inter-Regular.ttf`, `Inter-SemiBold.ttf`, `JetBrainsMono-Regular.ttf`, `JetBrainsMono-Bold.ttf`, `lucide-subset.ttf`, `LICENSE-*.txt` |
| `scripts/subset-lucide.py` (new) | regenerates `lucide-subset.ttf` from `src/ui_kit/icons.rs` |
| `src/ui_kit/icons.rs` (new) | named Lucide codepoints + `icon(…) -> RichText` |
| `src/ui_kit/text.rs` | `truncate_label`, `kbd_chip` |
| `src/ui_kit/lists.rs` | `selectable_row` |
| `src/ui_kit/tabs.rs` (new) | `tab_item` |
| `src/ui_kit/controls.rs` (new) | `toggle_switch`, `segmented` |
| `src/ui_kit/containers.rs` | `dialog_footer`, `DialogButton` |
| screen files | `titlebar.rs`, `tab_bar.rs`, `pane_renderer.rs`, `left_panel/*`, `file_browser.rs`, `git_diff.rs`, `info_bar.rs`, `settings_overlay.rs`, `dialogs.rs`, `app.rs` |
| `src/shortcuts.rs`, `src/keybindings.rs`, `src/app.rs` | default binding changes |
| `CLAUDE.md` | update module map, tokens, test counts |

---

### Task 1: Theme roles, GitHub Light, single visuals path

**Files:**
- Move: `src/theme.rs` → `src/theme/mod.rs` (git mv; no behavior change in that commit step)
- Create: `src/theme/palettes.rs`, `src/theme/roles.rs`, `src/theme/visuals.rs`
- Modify: `src/app/state.rs:65-100` and `src/app/state.rs:1968-2000` (replace both visuals blocks with `theme::visuals_for`)
- Test: `#[cfg(test)]` modules in `roles.rs`, `visuals.rs`, existing tests in `mod.rs`

**Interfaces:**
- Produces on `Theme` (new pub fields, `Color32`): `bg_base`, `bg_panel`, `bg_raised`, `border`, `fg`, `fg_muted`, `fg_strong`, `accent` (re-pointed to signature accent), `accent_bg`, `danger`, `danger_text`.
- Produces: `ThemeId::GitHubLight` (name `"GitHub Light"`, serde/str id `"github_light"`, last in `ThemeId::ALL`).
- Produces: `pub fn visuals_for(t: &Theme) -> egui::Visuals` in `theme::visuals`, re-exported from `theme`.
- Produces: `fn derive_roles(def: &ThemeDef) -> Roles` in `roles.rs`; `ThemeDef` gains `accent: [u8; 3]`.
- `fg_muted` already exists (= `overlay0`); it is **redefined in place** to the new role, which intentionally lightens every current caller. `fg_muted_raised` is a new role for muted text on `bg_raised` (modals).
- Existing fields (`bg_panel_fill`, `bg_row_active`, `fg_primary`, `fg_muted`, `border_panel`, …) stay for now as aliases computed from the roles; later tasks migrate callers; Task 13 removes unused ones.

- [ ] **Step 1: Move `theme.rs` into `src/theme/mod.rs` and split palette data into `palettes.rs`**; run `cargo test theme` — all 29 existing theme tests pass unchanged.
- [ ] **Step 1b: Add `theme::test_support`** (`lock_theme`, `ThemeGuard`, `theme_by_id`, `hex`, `ratio` — see "Test infrastructure") and make every existing test that calls `set_theme` take the guard (≈15 tests incl. `feedback.rs:134`). Run `cargo test` 5× (`for i in 1 2 3 4 5; do cargo test -q || break; done`) — no flakes.

- [ ] **Step 2: Write failing tests** in `src/theme/roles.rs`:

```rust
#[test] fn nord_roles_match_spec() {
    let t = theme_by_id(ThemeId::Nord);
    assert_eq!(t.bg_base,   hex(0x2E3440)); assert_eq!(t.bg_panel, hex(0x292E39));
    assert_eq!(t.bg_raised, hex(0x3B4252)); assert_eq!(t.border,   hex(0x3B4252));
    assert_eq!(t.fg,        hex(0xD8DEE9)); assert_eq!(t.fg_muted, hex(0x959EB1));
    assert_eq!(t.fg_strong, hex(0xECEFF4)); assert_eq!(t.accent,   hex(0x88C0D0));
    assert_eq!(t.danger,    hex(0xA54B54));
    assert_eq!(t.fg_muted_raised, hex(0xA7AFBF)); // 4.56:1 on #3B4252; spec's #959EB1 is 3.74:1 there
    assert_eq!(t.accent_bg, Color32::from_rgba_unmultiplied(0x88,0xC0,0xD0, 36)); // 14%
}
#[test] fn every_theme_meets_contrast() { /* for all 16: ratio(fg_muted,bg_base)>=4.5, ratio(fg_muted,bg_panel)>=4.5, ratio(fg,bg_raised)>=4.5, ratio(danger_text,danger)>=4.5, ratio(fg_muted_raised,bg_raised)>=4.5 */ }
#[test] fn signature_accents_match_spec_table() { /* 16 asserts, values from spec table */ }
#[test] fn light_theme_panel_differs_from_base() { /* for is_light themes: bg_panel != bg_base && bg_raised != bg_base */ }
#[test] fn github_light_is_registered() { assert_eq!(ThemeId::ALL.len(), 16); assert_eq!(ThemeId::GitHubLight.name(), "GitHub Light"); }
// in app/settings.rs tests:
#[test] fn github_light_roundtrips() { /* serde_json only — never save()/load(), they touch real APPDATA: AppSettings{theme_id: GitHubLight,..} -> to_string -> from_str, same id; serialized value is "GitHubLight" */ }
#[test] fn unknown_theme_id_falls_back_to_default() { /* from_str(r#"{"theme_id":"Nope","font_size":16.0}"#) -> theme_id == CatppuccinMocha AND font_size == 16.0 (needs a lenient deserialize_with on theme_id; today an unknown value fails the whole parse and safe_json_load resets every setting) */ }
```
In `visuals.rs`: `visuals_for_uses_roles` — `panel_fill == bg_panel`, `window_fill == bg_raised`, `selection.bg_fill == accent_bg`, `widgets.hovered.weak_bg_fill == bg_raised`.

- [ ] **Step 3: Run** `cargo test theme` — new tests FAIL (fields / variant missing).

- [ ] **Step 4: Implement.** Nord gets an override so its roles are the spec's exact values; all others use the spec recipe: `bg_panel = blend(base, mantle, 0.5)`, `bg_raised = border = surface0`, `fg = subtext1`, falling back to `text` if subtext1 < 4.5:1 on `bg_raised` (needed for Solarized Light 4.39 and Catppuccin Latte 4.05), `fg_strong = text`, `fg_muted` = `subtext0` stepped **toward `text` for both light and dark themes** in 2% blends (max 50 steps, `text` as last resort) until both contrast checks pass, `fg_muted_raised` (new role, see below) stepped the same way until ≥ 4.5:1 on `bg_raised`, `danger` = `red` darkened (dark themes) until white passes 4.5:1, `danger_text` = white (dark) / `bg_base`-contrast-safe (light). GitHub Light palette: base `#FFFFFF`, mantle `#F6F8FA`, surface0 `#EAEEF2`, text `#1F2328`, subtext `#59636E`, blue `#0969DA`, green `#1A7F37`, red `#CF222E`, yellow `#9A6700`, mauve `#8250DF`, teal `#1B7C83`, ANSI from GitHub's light terminal palette. `visuals_for` absorbs the two duplicated blocks in `state.rs`.

- [ ] **Step 5: Run** `cargo test` — all pass. Launch `cargo run`, switch through all 16 themes in Settings: no crash, colors change instantly.

- [ ] **Step 6: Commit (after user approval)** — `refactor(theme): split theme module, add color roles and GitHub Light`

---

### Task 2: Type scale tokens

**Files:** Modify `src/theme/mod.rs` (FONT_* block ~line 546), all callers of removed constants.

**Interfaces:**
- Produces: `FONT_XS = 11.0`, `FONT_SM = 12.0`, `FONT_MD = 13.0`, `FONT_LG = 14.0`; `FONT_TERM = 14.0` (default terminal size); `TERM_LINE_HEIGHT = 1.3`. `FONT_HEADING_1/2` kept but documented "markdown preview only".
- Also: `GIT_FONT_SZ` (10.0, `git_diff.rs:263,443,563`) → `FONT_XS`; `FONT_HEADING_2` use at `session_list.rs:148` → `FONT_LG`. Spinner sizes (`git_diff.rs:88,311`) are not text and are exempt.
- Removes: `FONT_UI_XS`, `FONT_SYS_SM`, `FONT_SYS_XS`, `FONT_STATUS`, `FONT_UI_SM/MD/LG`, `FONT_TERM_BOLD` (callers mapped: XS/SYS_*/UI_SM→`FONT_XS`, UI_MD→`FONT_SM`, UI_LG→`FONT_MD`, STATUS→`FONT_LG`).

- [ ] **Step 1: Failing test** `type_scale_is_four_sizes`: asserts the four values and `FONT_XS >= 11.0`.
- [ ] **Step 2: Run** `cargo test type_scale` → FAIL.
- [ ] **Step 3: Add constants, delete old ones, fix all compile errors with the mapping above.** Grep check: `rg "FONT_(UI|SYS|STATUS|TERM_BOLD)" src` returns nothing; `rg "RichText.*\.size\([0-9]|FontId::(new|proportional|monospace)\([0-9]" src/app src/ui_kit` returns nothing.
- [ ] **Step 4: Run** `cargo test && cargo clippy --all-targets -- -D warnings` → PASS.
- [ ] **Step 5: Commit (after user approval)** — `refactor(theme): collapse type scale to 11/12/13/14`

---

### Task 3: Bundled fonts (Inter + JetBrains Mono) and terminal line height

**Files:**
- Create: `src/fonts.rs`, `assets/fonts/{Inter-Regular,Inter-SemiBold,JetBrainsMono-Regular,JetBrainsMono-Bold}.ttf`, `assets/fonts/LICENSE-Inter.txt`, `assets/fonts/LICENSE-JetBrainsMono.txt`
- Modify: `src/main.rs` (mod decl), `src/app/state.rs:104-166` (font block moves into `fonts.rs`), `src/renderer/terminal_pass.rs:123` (cell height), every non-terminal/non-kbd `FontFamily::Monospace` / `FontId::monospace` use (`git_diff.rs`, `pane_renderer.rs` breadcrumb/notes, `session_list.rs`, `command_palette.rs`, `app.rs`; keep `markdown.rs` code blocks and `syntax.rs` editor mono).

**Interfaces:**
- Produces: `pub fn install_fonts(ctx: &egui::Context)`; `pub const UI_BOLD: &str = "ui_bold"`, `pub fn ui_bold() -> egui::FontFamily` (Inter SemiBold, for titles); families per P5 (egui defaults kept after the bundled font so emoji like 📌 🔖 📝 still render; Lucide **only** in `icons`; `symbol_fallback` added only if it loaded, otherwise epaint panics). `fn font_definitions(symbol: Option<Vec<u8>>) -> egui::FontDefinitions` is pure; `install_fonts` reads the OS symbol font and passes it in. `FontFamily::Name("mono_bold")` = JetBrains Mono Bold + Monospace fallbacks, used by the terminal for bold cells (P6).
- Produces: `fn font_definitions() -> egui::FontDefinitions` (pure, testable).
- Terminal: `cell_h = (row_height(font) * theme::TERM_LINE_HEIGHT).round()`, glyphs drawn vertically centred within the cell (selection, cursor, underline, strike use `cell_h`).

- [ ] **Step 0: Add `.gitattributes`** with `*.ttf binary`.
- [ ] **Step 1: Download fonts** from official releases (Inter v4.x `extras/ttf`, JetBrains Mono v2.304 `fonts/ttf`), copy the OFL texts. Check combined size ≤ ~1 MB (`du -ch assets/fonts/*.ttf`); if larger, subset to Latin + Latin-1 + General Punctuation + Box Drawing + Block Elements (mono) with `pyftsubset` and record the command in `scripts/subset-lucide.py`'s sibling README section of `assets/fonts/README.md`.
- [ ] **Step 2: Failing tests** in `fonts.rs`: `proportional_starts_with_inter`, `monospace_starts_with_jetbrains`, `symbol_fallback_is_last_when_present`, `no_symbol_font_does_not_register_symbol_fallback`, `egui_default_fonts_kept_as_fallback`, `every_named_family_is_registered` (ui_bold, mono_bold, icons), `font_bytes_are_valid_ttf` (parse each with `ttf_parser::Face::parse`). Add `ttf-parser = "0.25"` (already in Cargo.lock via owned_ttf_parser) under `[dev-dependencies]`.
- [ ] **Step 3: Run** `cargo test fonts` → FAIL.
- [ ] **Step 4: Implement** `font_definitions`/`install_fonts`; call once from `App::new` (replacing the inline block). Apply `TERM_LINE_HEIGHT` in `terminal_pass.rs`; update `TerminalGeometry` tests that assume `cell_h == row_height`. Switch non-terminal mono uses to proportional.
- [ ] **Step 5: Run** `cargo test`; `cargo run` → UI is Inter, terminal is JetBrains Mono with visibly looser lines, `vim`/`htop` box drawing still joins, mouse click lands on the right cell row.
- [ ] **Step 6: Commit (after user approval)** — `feat(fonts): bundle Inter and JetBrains Mono, terminal line height 1.3`

---

### Task 4: Lucide icon font and icon module

**Files:**
- Create: `src/ui_kit/icons.rs`, `assets/fonts/lucide-subset.ttf`, `assets/fonts/LICENSE-Lucide.txt`, `scripts/subset-lucide.py`
- Modify: `src/ui_kit/mod.rs`, `src/fonts.rs` (register `"lucide"` **only** in `FontFamily::Name("icons")` = [lucide, Inter] — not in Proportional/Monospace, P5)

**Interfaces:**
- Produces: `pub mod icon { pub const CHEVRON_RIGHT: char; CHEVRON_DOWN; CHEVRON_UP; FOLDER; FOLDER_OPEN; FILE; FILE_TEXT; SEARCH; GIT_BRANCH; COLUMNS_2; ROWS_2; X; POWER; TERMINAL; MENU; ARROW_LEFT_RIGHT; SETTINGS; PLUS; KEYBOARD; HISTORY; PANEL_LEFT; PANEL_RIGHT; DOWNLOAD; REFRESH_CW; COPY; TRASH; PENCIL; MAXIMIZE; MINIMIZE; SQUARE; }` (codepoints from Lucide's `info.json` for the pinned release).
- Produces: `pub const ALL: &[(&str, char)]` (name, codepoint) and `pub fn icon(ch: char, size: f32, color: Color32) -> egui::RichText` (family `icons`).

- [ ] **Step 1: Write `scripts/subset-lucide.py`**: reads `ALL` names from `icons.rs` via regex, downloads pinned `lucide-font` release TTF, runs `fontTools.subset` keeping only those codepoints, writes `assets/fonts/lucide-subset.ttf`. Run it; file should be < 20 KB.
- [ ] **Step 2: Failing tests** in `icons.rs`: `every_icon_constant_exists_in_font` (each `ALL` codepoint has a glyph via `ttf_parser::Face::glyph_index`), `icon_names_are_unique`, `icon_uses_icons_family`.
- [ ] **Step 3: Run** `cargo test icons` → FAIL before font/constants exist; implement; → PASS.
- [ ] **Step 4: Replace Unicode glyph icons** everywhere (`app.rs:2272-2274`, `file_browser.rs:228,446-456`, `info_bar.rs:121`, `git_diff.rs:97` `↻`, `titlebar.rs`, `dialogs.rs`, `buttons.rs` callers). Check: `rg "[▶▼▲·✕×☰⟳⎇↻]" src --glob '!**/tests*'` returns only non-icon text uses (e.g. `·` as a separator in labels).
- [ ] **Step 5: Run** `cargo test`; `cargo run` → icons render crisp at 10–18px in every theme.
- [ ] **Step 6: Commit (after user approval)** — `feat(ui_kit): bundle Lucide icon subset, replace unicode glyph icons`

---

### Task 5: `ui_kit` primitives — truncation, kbd chips, selectable row, tab item, toggle, segmented, dialog footer

**Files:**
- Modify: `src/ui_kit/text.rs`, `src/ui_kit/lists.rs`, `src/ui_kit/containers.rs`, `src/ui_kit/mod.rs`
- Create: `src/ui_kit/tabs.rs`, `src/ui_kit/controls.rs`

**Interfaces (all produced; later tasks consume):**
- `pub fn truncate_galley(painter: &egui::Painter, text: &str, font: FontId, color: Color32, max_w: f32) -> Arc<Galley>` — one `LayoutJob` with `TextWrapping::truncate_at_width(max_w)` (see P4).
- `pub fn truncate_label(ui: &mut Ui, text: &str, font: FontId, color: Color32, max_w: f32) -> Response` — shows full text as tooltip only when truncated.
- `pub fn kbd_chip(ui: &mut Ui, key: &str) -> Response` — mono `FONT_SM`, `bg_raised` fill, `R_SM`, `SP_2` x-pad.
- `pub fn kbd_combo(ui: &mut Ui, shortcut: &crate::shortcuts::Shortcut)` — one chip per modifier + key.
- `pub fn selectable_row(ui: &mut Ui, selected: bool, height: f32, add_contents: impl FnOnce(&mut Ui)) -> Response` — `accent_bg` fill when selected, `bg_raised` on hover, `R_MD`.
- `pub fn tab_item(ui: &mut Ui, label: &str, selected: bool, badge: Option<&str>) -> Response` — 2px accent bottom bar + `fg_strong` when selected, `fg_muted` otherwise; `FONT_MD`.
- `pub fn toggle_switch(ui: &mut Ui, on: &mut bool) -> Response` — 36×20 pill, `accent` when on.
- `pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) -> Response`.
- `pub enum DialogButton { Secondary(&'static str), Primary(&'static str), Destructive(&'static str) }`; `pub fn dialog_footer(ui: &mut Ui, buttons: &[DialogButton]) -> Option<usize>` — top border `border`, 34px-high buttons right-aligned in given order, returns clicked index. Constant `DIALOG_BTN_H: f32 = 34.0` in theme.

- [ ] **Step 0: Create `src/ui_kit/test_harness.rs`** (see "Test infrastructure" item 2) with a self-test `harness_click_registers` (a `ui.button` clicked via `click_at` reports `clicked()`).
- [ ] **Step 1: Failing tests** (via `test_harness` for widgets, plain calls for pure functions):
  - `truncate_short_text_unchanged`, `truncate_long_text_ends_with_ellipsis_and_fits`, `truncate_zero_width_returns_ellipsis_only`, `truncate_handles_multibyte` (`"Rosé Pine ✓ 日本語"` never splits a char).
  - `selectable_row_selected_paints_accent_bg` (inspect shapes in output), `tab_item_selected_has_bottom_bar`, `toggle_switch_click_flips_value`, `segmented_click_sets_value`, `dialog_footer_orders_buttons_right_aligned` (rects sorted by x match input order and last button's max.x == footer max.x − SP_6), `dialog_buttons_are_34px`.
- [ ] **Step 2: Run** `cargo test ui_kit` → FAIL.
- [ ] **Step 3: Implement.** All colors from `theme::active()` roles; spacing from `SP_*`.
- [ ] **Step 4: Run** `cargo test` → PASS.
- [ ] **Step 5: Commit (after user approval)** — `feat(ui_kit): add shared selection, tab, toggle, segmented, truncation and dialog footer widgets`

---

### Task 6: Title bar

**Files:** Modify `src/app/ui/titlebar.rs` (fill ~194-209, sysmon, buttons). Split out `src/app/ui/titlebar/sysmon.rs` if titlebar.rs stays > ~800 lines after edits.

**Interfaces:** Consumes Task 1 roles, Task 4 icons, Task 5 `truncate_label`, `kbd_chip`.

- [ ] **Step 1: Failing test** `titlebar_fill_ignores_workspace_color` — extract `fn titlebar_fill(t: &Theme, _ws_color: Option<Color32>) -> Color32` and assert it returns `t.bg_panel` for `Some(blue)` and `None`.
- [ ] **Step 2: Failing test** `sysmon_rect_is_vertically_centered` — extract `fn sysmon_rect(bar: Rect, w: f32) -> Rect`; assert `center().y == bar.center().y` and contained in `bar`.
- [ ] **Step 3: Implement**: delete unused `theme::header_bg` and its 3 tests (see Existing Tests); `bg_panel` fill + bottom `border` line; all buttons Lucide icons (`MENU`, `ARROW_LEFT_RIGHT`, `HISTORY`, `KEYBOARD`, `PANEL_LEFT/RIGHT`, `SETTINGS`, `DOWNLOAD`); "Switch" + `kbd_combo` of `shortcut_registry.find_shortcut(OpenQuickSwitcher)`; title = session `fg_strong` `FONT_MD` · workspace `fg` · branch `fg_muted`, each truncated; sysmon "CPU ▬ 14%  MEM ▬ 84%" at `FONT_XS` with 40×4 bars (`accent` for CPU, `warning` for MEM). No sun icon. `SYSMON_W` (100, `theme.rs:475`) increases to fit "CPU ▬ 14%  MEM ▬ 84%" at `FONT_XS` with 40px bars (measure; ~190).
- [ ] **Step 4: Run** tests → PASS; `cargo run` → title bar never blue, sysmon not clipped at 1× and 1.5× scaling.
- [ ] **Step 5: Commit (after user approval)** — `style(titlebar): panel background, Lucide icons, centred system monitor`

---

### Task 7: Tab bar and pane header

**Files:** Modify `src/app/ui/tab_bar.rs` (~174 workspace tint, active tab painting), `src/app/ui/pane_renderer.rs` (pane header strip / breadcrumb).

**Interfaces:** Consumes `tab_item`, `truncate_label`, icons, roles.

- [ ] **Step 1: Failing test** `active_tab_style` — extract `fn tab_visual(selected: bool, t: &Theme) -> TabVisual { text: Color32, bar: Option<Color32>, fill: Option<Color32>, outline: bool }`; assert selected → `bar == Some(accent)`, `fill == None`, `!outline`, `text == fg_strong`; unselected → `bar == None`, `text == fg_muted`.
- [ ] **Step 2: Failing test** `workspace_strip_is_thin` — tab workspace color appears only as a `TAB_COLOR_STRIP_W` rect (assert helper returns rect width == `TAB_COLOR_STRIP_W`), never as tab fill.
- [ ] **Step 3: Implement**: tab strip `bg_panel`; tabs via `tab_item` semantics; titles truncated; status dot (`success` alive / `fg_muted` dead); split/close buttons Lucide `COLUMNS_2`, `ROWS_2`, `X`. Pane header: `bg_panel` (spec: title bar and pane header use bg_panel) with bottom `border`, breadcrumb "workspace  ⎇-icon branch" in Inter `FONT_SM`, `fg` / `fg_muted`, truncated.
- [ ] **Step 4: Run** tests → PASS; `cargo run` with 12 tabs at 800px width → truncation, no overlap.
- [ ] **Step 5: Commit (after user approval)** — `style(tabs): single selected style, thin workspace strip, Inter breadcrumb`

---

### Task 8: Left panel — sessions and workspaces

**Files:** Modify `src/app/ui/left_panel/session_list.rs` (~877 row tint), `src/app/ui/left_panel/workspace_section.rs`, `src/app/ui/left_panel/mod.rs`.

**Interfaces:** Consumes `selectable_row`, `truncate_label`, icons, roles.

- [ ] **Step 1: Failing test** `session_row_uses_shared_selection` — extract `fn session_row_style(selected: bool, ws_color: Option<Color32>, t: &Theme) -> RowStyle { fill: Option<Color32>, dot: Color32, left_bar: bool }`; selected → `fill == Some(accent_bg)`, `!left_bar`; workspace color only in `dot`.
- [ ] **Step 2: Failing test** `workspace_card_changed_count_truncates` — `"feature/very-long-branch-name-here"` + `"12 changed"` in 160px: branch truncated, count shown whole.
- [ ] **Step 3: Implement** per mockup: header "SESSIONS · 3" (`FONT_XS`, `fg_muted`, letter-spaced caps), "Close all" text button, "+ New" raised button; search inputs `bg_base` fill + `border`, Lucide `SEARCH`; rows: title `FONT_MD` `fg`/`fg_strong` selected, subtitle `FONT_XS` `fg_muted`; close `X` on hover/selected only. Workspaces: "WORKSPACES · 2" + chevron; card name `FONT_MD`, branch `FONT_XS` `fg_muted`, "N changed" `FONT_XS` `warning`; workspace color dot 8px.
- [ ] **Step 4: Run** tests → PASS; `cargo run` → visually compare with mockup m2 left column.
- [ ] **Step 5: Commit (after user approval)** — `style(left-panel): shared selection, truncation, workspace color dots`

---

### Task 9: Right panel — file tree with `M` badge, Directory/Git diff tabs, Git Diff list font, Notes "Autosaved"

**Files:**
- Modify: `src/git/parser.rs` (add `modified_paths`), `src/app/file_browser.rs` (`DirData`, `render_dir_tree`, `render_flat_file_list`, `file_icon`), `src/app/watcher.rs:~190` (populate set where status is stored), `src/app/git_diff.rs` (font + `↻` icon), `src/app.rs` / `pane_renderer.rs` (right-panel tab header, notes header)

**Interfaces:**
- Produces: `pub fn modified_paths(status: &str, repo_root: &Path) -> HashSet<PathBuf>` in `git/parser.rs` (absolute paths; includes modified/added/renamed/untracked/conflicted; renames use new path).
- Produces: `DirData.git_modified: Arc<HashSet<PathBuf>>`, set only when `git_status` changes (never recomputed per frame). Computed on the worker side if the status arrives from a worker result; otherwise at assignment in `watcher.rs` (once per status change, not per frame).
- `render_dir_tree` gains `modified: &HashSet<PathBuf>` param.

- [ ] **Step 1: Failing tests** in `git/parser.rs`: `modified_paths_includes_worktree_and_index_changes`, `modified_paths_uses_rename_target`, `modified_paths_handles_quoted_paths` (`"\"a b.rs\""`), `modified_paths_empty_status_is_empty`.
- [ ] **Step 2: Failing tests** in `file_browser.rs`: `file_icon_returns_lucide_glyph` (no `·`), `tree_row_layout_aligns_names` — extract `fn tree_row_layout(depth: usize, is_dir: bool) -> TreeRowLayout { chevron_x, icon_x, name_x }`; assert `name_x` equal for a dir and a file at the same depth, chevron column width 16.
- [ ] **Step 3: Run** → FAIL. **Implement**: fixed 16px chevron column (`CHEVRON_RIGHT/DOWN`, `FONT_XS`, `fg_muted`; empty for files), `FOLDER`/`FILE` icon `FONT_SM` `fg_muted`, name Inter `FONT_SM` `fg`, truncated; `M` badge `FONT_XS` bold `warning` right-aligned for paths in `git_modified`. Directory/Git diff header via `tab_item` with count badge. Git Diff file list → Inter. Notes header "NOTES" left, "Autosaved" `FONT_XS` `fg_muted` right (shown after the note store's save completes; "Saving…" while dirty), notes body Inter.
- [ ] **Step 4: Run** `cargo test` → PASS; `cargo run` on this repo with a modified file → `M` badge appears within the 500ms refresh, no `·`.
- [ ] **Step 5: Commit (after user approval)** — `style(right-panel): Lucide tree with modified badges, Inter for paths and notes`

---

### Task 10: Shortcut defaults and Keyboard Shortcuts dialog

**Files:** Modify `src/shortcuts.rs:~580` (defaults), `src/keybindings.rs:~343,~363`, `src/app.rs:2983-2993` (hardcoded handling), `src/app/ui/settings_overlay.rs:~583` (dialog body; extract to `src/app/ui/shortcuts_dialog.rs`).

**Interfaces:** Consumes `kbd_combo`, roles. Produces `pub fn render_shortcuts_dialog(app: &mut App, ctx: &egui::Context)` in `shortcuts_dialog.rs`.

- [ ] **Step 1: Failing tests:**
  - `default_bindings_have_no_collisions` (shortcuts.rs and keybindings.rs: every chord maps to exactly one action).
  - `reopen_closed_session_is_ctrl_shift_t`, `reveal_in_explorer_is_ctrl_shift_r`, `search_all_sessions_is_ctrl_shift_a`.
  - `user_override_survives_default_change` — `KeybindingsConfig::from_json` on a string binding `reopen_closed_session` to `Ctrl+Shift+R`; resolved binding is `Ctrl+Shift+R`.
- [ ] **Step 2: Run** → FAIL. **Implement** defaults in both tables; **keep** the global-shortcut block at `app.rs:2963-2998` and its handlers (`app.rs:3066`, `3089`) — the registry path (`app.rs:3175`) routes both actions to `_ => {}` and is skipped while modals/editors have focus. Replace each hardcoded `consume_key(cs, Key::X)` in that block with the chord from `self.shortcut_registry.find_shortcut(action)` (Reopen → T, Search all → A) and remove the `R` branch so `Ctrl+Shift+R` reaches `RevealInExplorer` (`app.rs:3248`). Test: `global_block_reopen_uses_registry_chord`. Settings hint text uses the registry's display string.
- [ ] **Step 3: Restyle the dialog** per mockup m1: ~800px wide, title `FONT_LG` `ui_bold`, two columns of groups (header `FONT_XS` caps `accent`), rows `FONT_MD` label + right-aligned `kbd_combo`, row separators `border`, wrapped in `ScrollArea` so the last row is reachable at 400px window height.
- [ ] **Step 4: Run** `cargo test` → PASS; `cargo run` → `Ctrl+Shift+T` reopens, `Ctrl+Shift+A` opens global search, `Ctrl+Shift+R` reveals.
- [ ] **Step 5: Commit (after user approval)** — `fix(shortcuts): resolve Ctrl+Shift+R clash, restyle shortcuts dialog`

---

### Task 11: Settings overlay

**Files:** Modify `src/app/ui/settings_overlay.rs` (6 checkboxes, theme picker, cursor picker). Extract `src/app/ui/settings/theme_grid.rs` if the file exceeds ~600 lines.

**Interfaces:** Consumes `toggle_switch`, `segmented`, roles. Produces `fn theme_card(ui, theme: &Theme, selected: bool) -> Response`; `fn grid_columns(n: usize) -> usize` (always 4).

- [ ] **Step 1: Failing tests:** `theme_grid_is_four_columns_for_sixteen` (rows == 4), `theme_card_shows_full_name` (card width fits `"Catppuccin Mocha"` at `FONT_MD`; no truncation for any of the 16 names at the dialog's min width), `settings_setting_rows_have_hints` (each toggle row descriptor has non-empty hint).
- [ ] **Step 2: Run** → FAIL. **Implement** per mockup m5: section headers `FONT_XS` caps `fg_muted`; each boolean as label `FONT_MD` + hint `FONT_SM` `fg_muted` + right-aligned `toggle_switch`; numeric inputs right-aligned `bg_base` field; theme grid 4 columns of cards (palette strip base|accent|green, name below; selected: `accent` 1.5px outline + `bg_raised` fill); cursor style via `segmented` (Block / Beam / Underline).
- [ ] **Step 3: Run** `cargo test` → PASS; `cargo run` → toggles, grid, segmented control all change settings and persist across restart.
- [ ] **Step 4: Commit (after user approval)** — `style(settings): toggles, hints, 4-column theme grid, segmented cursor picker`

---

### Task 12: Quick Switcher, Quit dialog, and all other dialogs

**Files:** Modify `src/app/ui/dialogs.rs` (`render_quick_switcher` 63-~640, `render_quit_confirm` 1280, plus workspace save/edit, close-all, commit, push, stage-all, revert, delete, open-folder). Split into `src/app/ui/dialogs/{quick_switcher.rs, quit.rs, git.rs, workspace.rs, confirm.rs}` (dialogs.rs is 2054 lines).

**Interfaces:**
- Consumes `dialog_footer`, `DialogButton`, `selectable_row`, `kbd_chip`, icons, roles.
- Consumes existing `ShellKind::display_name()` (`pty/mod.rs:49`) via `SessionEntry.shell` (`pane.rs:122`) — no new shell-name parser. Produces: `fn switcher_row_title(entry: &SessionEntry) -> Cow<str>` (custom title if renamed, else display_name); `fn short_cwd(path: &str, max_w_chars: usize) -> String`; `fn quit_summary(terminals: usize, files: usize) -> String`.

- [ ] **Step 1: Failing tests:** `switcher_row_title_uses_display_name_when_not_renamed`, `switcher_row_title_keeps_user_rename`, `short_cwd_keeps_last_components`, `quit_summary_text` (`(2,1)` → `"You have 3 open tabs: 2 terminals and 1 file. Running processes in those terminals will stop."`; `(1,0)` → singular forms), `switcher_width_is_640` (`QUICK_SWITCHER_W == 640.0` in theme), `switcher_row_letters_hidden_until_workspace_picked` (extract `fn row_hint(picked_ws: Option<usize>, idx: usize) -> Option<char>`).
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement Quick Switcher** per m3: 640px centered window (`bg_raised`, `R_LG`, shadow), search row with `SEARCH` icon + `kbd_combo` of the registry's `OpenQuickSwitcher` chord; numbered workspace group headers (`kbd_chip("1")` + caps name); rows via `selectable_row`: `TERMINAL` icon tile, `switcher_row_title` `FONT_MD`, `short_cwd` `FONT_SM` `fg_muted`, letter chip on the right only when `row_hint` is `Some`; footer chips `1-9` workspace, `/` search, `Enter` open, `Esc` close (today `/` enters search mode, `dialogs.rs:202-217`, so the hint stays). Key handling unchanged. Row title stays the session's title when the user renamed it; otherwise `ShellKind::display_name()`.
- [ ] **Step 4: Implement Quit dialog** per m4: `POWER` icon in a `danger`-tinted tile, title `FONT_LG` `ui_bold`, `quit_summary` body `FONT_MD`, open-tab chips (`bg_raised`, `FONT_SM`, truncated, max 6 + "+N more"), `dialog_footer([Secondary("Cancel"), Destructive("Quit")])`.
- [ ] **Step 5: Migrate every other dialog** to `dialog_footer` (Cancel first, primary/destructive last; delete/revert/close-all use `Destructive`). Also migrate dialogs outside `dialogs.rs`: New Label window (`session_list.rs:130-168`), `closed_sessions_picker.rs`, `conflict_resolver.rs`, `left_panel/context_menu.rs` dialogs. Grep check: `rg '"Cancel"|ActionButtonStyle::Cancel' src/app` hits only `dialog_footer` call sites.
- [ ] **Step 6: Run** `cargo test` → PASS; `cargo run` → open each dialog once; Esc/Enter behave as before.
- [ ] **Step 7: Commit (after user approval)** — `style(dialogs): 640px quick switcher, quit dialog, shared dialog footer`

---

### Task 13: Cleanup, docs, full verification

**Files:** `src/theme/mod.rs` (remove now-unused legacy fields), `CLAUDE.md` (module map: `theme/`, `fonts.rs`, `ui_kit/icons.rs`, `tabs.rs`, `controls.rs`, dialogs split; tokens section: roles, type scale, `TERM_LINE_HEIGHT`; dependencies: bundled fonts; test counts), `assets/fonts/README.md` (sources, versions, licenses, subset commands).

- [ ] **Step 1:** Remove `Theme` fields with zero references (`rg` each); `cargo build` clean.
- [ ] **Step 2:** Hardcode audit — `rg "Color32::from_rgb" src/app src/ui_kit` and `rg "FontFamily::Monospace" src/app` return only allowed sites (terminal, kbd, markdown code, editor).
- [ ] **Step 3: Run** `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` → all PASS; record new test count in CLAUDE.md.
- [ ] **Step 3b: Coverage** — `cargo llvm-cov --bin terminal-studio --summary-only`; report per-file line coverage for new/changed non-painting modules; add tests for uncovered lines or list them with reasons for the PR description.
- [ ] **Step 4:** `cargo build --release`; compare binary size to `master` (expect ≈ +1 MB); run app, cycle all 16 themes, screenshot main window in Nord and compare against mockup m2.
- [ ] **Step 5: Commit (after user approval)** — `docs: update CLAUDE.md for UI cleanup`
