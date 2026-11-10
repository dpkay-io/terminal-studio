# UI Cleanup: Consistent Fonts, Colors and Selected States — Design

Issue: https://github.com/dpkay-io/terminal-studio/issues/36
Branch: `feat/ui-cleanup-36` (from `origin/master`)
Mockups: the five images attached to the issue (main window, Settings, Quick Switcher, Keyboard Shortcuts, Quit dialog).

## Intent

The app works but feels "off": mismatched fonts, ~6 text sizes, three competing
background colors, a loud saturated-blue title bar, and four different "selected"
styles. The goal is to make the app read as one coherent product **without
changing the layout** — same panels, same positions, same features. The mockups
are the visual target and should be matched as closely as egui allows.

## Goals

- Two font families, four text sizes, used consistently.
- Every theme (15 existing + new GitHub Light) expressed through one small set of surface/text/accent roles.
- One selected style for list items, one for tabs.
- Craft fixes: tree chevrons/icons, ellipsis + tooltip on overflow, title-bar
  system monitor not clipped, Quick Switcher, Settings, Shortcuts and Quit dialog
  restyled per mockups, shortcut conflict fixed.

## Non-goals

- No panel moves, no new panels, no layout restructuring.
- No new user-facing features beyond the small mockup details listed under "Mockup extras".

## Anti-goals (a failure even if it "works")

- Text that becomes unreadable on any theme (dark or light).
- Any regression in snappiness — fonts/icons/glyphs must not add per-frame cost
  (no per-frame font loading, no per-frame theme recomputation).
- Hardcoded colors or pixel sizes in UI modules (all through `theme.rs` tokens).
- Breaking users' saved custom keybindings.

## Decisions (made with the user)

### Fonts — bundled
- Bundle **Inter** (UI) and **JetBrains Mono** (terminal + kbd chips) into the
  binary via `include_bytes!` (both OFL; include license files in repo).
  Same look on every platform. Registered once at startup in `FontDefinitions`.
- Existing OS symbol-font fallback stays as the last fallback in each family.
- **Mono only for the terminal and kbd chips.** Paths, breadcrumb, file tree,
  Git Diff file list and Notes switch to the UI font.
- Weights: the builder decides which weights to bundle (at minimum Regular, plus
  a SemiBold/Bold for titles and mono bold for the terminal) — keep total added
  size around 1 MB; subsetting is allowed.

### Icons — Lucide font
- Bundle a **subset of the Lucide icon font** (ISC license) containing only the
  glyphs used (~20–30): chevron-right/down, folder, file, search, git-branch,
  columns/rows (split), x, power, terminal, menu, arrow-left-right (switch), sun,
  settings, plus, etc. — the builder finalises the list from the mockups.
- Icons are rendered as text in a dedicated `FontFamily::Name("icons")`, so they
  take theme colors and scale cleanly.
- Replace Unicode glyph icons (`▶ ▼ · ✕ ☰ ⎇` …) across `app.rs`,
  `file_browser.rs`, `dialogs.rs`, `info_bar.rs`, `titlebar.rs` and anywhere else found.
- Icon codepoints live in one module (e.g. `src/ui_kit/icons.rs`) as named constants.

### Type scale
- Four sizes: **11 / 12 / 13 / 14**.
  - 11 — meta: branch subtitles, counts, hints, system monitor
  - 12 — tree, list items, kbd chips
  - 13 — tabs, buttons, body
  - 14 — modal titles, terminal default
- Collapse existing `FONT_*` constants onto these; `FONT_UI_XS` (10),
  `FONT_SYS_SM` (9), `FONT_SYS_XS` (8) go to 11. Larger headings remain only
  inside the markdown preview.
- Terminal: JetBrains Mono at the existing font-size setting (default 14),
  line height ~1.3 (within the issue's 1.25–1.5).

### Colors — applied to every theme
Each theme defines the same roles (names indicative; builder may map onto
existing `Theme` fields and remove ones that become redundant):

| Role | Use | Nord value |
|------|-----|-----------|
| bg_base | terminal / editor | `#2E3440` |
| bg_panel | sidebars, title bar, tab strip, pane header | `#292E39` |
| bg_raised | modals, hover, chips | `#3B4252` |
| border | dividers, outlines | `#3B4252` |
| fg | normal text | `#D8DEE9` |
| fg_muted | secondary text | `#959EB1` |
| fg_strong | titles, selected text | `#ECEFF4` |
| fg_muted_raised | muted text inside modals (on bg_raised) | `#A7AFBF` (added after review: `#959EB1` is only 3.74:1 on `#3B4252`) |
| accent | the single accent | `#88C0D0` |
| accent_bg | selected fill | accent @ 14% alpha |
| danger | destructive buttons (white text) | `#A54B54` |

- Nord uses these exact values. Every other theme (including the light ones)
  is derived with the **same recipe** from its own palette in `ThemeDef`:

  | Role | Recipe (from existing `ThemeDef` fields) |
  |------|------------------------------------------|
  | bg_base | `base` |
  | bg_panel | `base` nudged ~50% toward `mantle` (slightly darker than base; on light themes slightly darker too) |
  | bg_raised | `surface0` |
  | border | `surface0` |
  | fg | `subtext1` (or `text` if subtext1 fails contrast) |
  | fg_strong | `text` |
  | fg_muted | `subtext0`/`overlay1`, darkened/lightened until it passes 4.5:1 on bg_base and bg_panel |
  | accent | the theme's signature accent (table below) — new `accent` field on `ThemeDef` |
  | accent_bg | accent @ 14% alpha |
  | danger | `red` adjusted until white (or bg_base on light themes) text passes 4.5:1 |

  Signature accents (match the swatch strips in the Settings mockup):

  | Theme | Accent |
  |-------|--------|
  | Catppuccin Mocha | blue `#89B4FA` |
  | Dracula | purple `#BD93F9` |
  | Nord | frost `#88C0D0` |
  | Solarized Dark | blue `#268BD2` |
  | Gruvbox Dark | aqua `#83A598` |
  | Tokyo Night | blue `#7AA2F7` |
  | Rosé Pine | iris `#C4A7E7` |
  | One Dark | blue `#61AFEF` |
  | Everforest Dark | aqua `#7FBBB3` |
  | Monokai | cyan `#66D9EF` |
  | GitHub Dark | blue `#58A6FF` |
  | Ayu Dark | blue `#59C2FF` |
  | Material Darker | blue `#82AAFF` |
  | Solarized Light | blue `#268BD2` |
  | Catppuccin Latte | blue `#1E66F5` |
  | GitHub Light (new, if approved) | blue `#0969DA` |

  The exact hex values these produce are left to the builder, bounded by the contrast test.
- **Fonts, icons, type scale and selected-state styles are global** — identical
  for every theme; only colors vary per theme.
- The title bar and pane header use `bg_panel`, never the saturated blue.
- Semantic colors (success/warning/error, git colors, markdown colors) stay but
  should be checked for harmony with the new surfaces.
- **Contrast test**: a unit test asserts, for every theme, `fg_muted` ≥ 4.5:1 on
  both `bg_base` and `bg_panel`, `fg` and `fg_muted_raised` ≥ 4.5:1 on `bg_raised`, and danger-button
  text ≥ 4.5:1 on `danger`. Reuse the existing WCAG helpers in `theme.rs`.

### Selected states — one style
- **List items** (sessions, workspaces, file tree, Quick Switcher rows, palette
  rows): `accent_bg` fill, rounded `R_MD`. No left bar, no outline.
- **Tabs** (terminal tabs, Directory / Git diff tabs): 2px `accent` bottom bar,
  text `fg_strong`. No box outline, no filled chip.
- Hover: `bg_raised` (or a light tint) — one hover style everywhere.
- Implement as shared helpers in `ui_kit` (e.g. `selectable_row`, `tab_item`)
  and use them in every panel, rather than per-panel painting.

### Craft fixes
- **Tree**: fixed 16px chevron column (Lucide chevron), small folder/file icon,
  no `·` prefix; file names align with folder names. Modified files show an
  `M` badge (warning color) at the right edge, using existing git status data.
- **Overflow**: any single-line label that may overflow (session names, theme
  names, branch names, "N changed", breadcrumb, tab titles) truncates with `…`
  and shows the full text in a hover tooltip. One shared helper in `ui_kit/text.rs`.
- **System monitor**: vertically centered in the title bar, not clipped;
  mockup style "CPU ▬ 14%  MEM ▬ 84%" with small bars.
- **Quick Switcher**: 640px-wide centered palette (not full-screen); grouped by
  workspace with a numbered header; rows show a terminal icon, shell **name**
  (e.g. "PowerShell", not the full exe path) and a short cwd; see "Quick
  Switcher behavior" for hotkeys and footer.
- **Settings**: toggles instead of checkboxes, each setting with a readable
  muted hint line beneath; themes in a 4-column grid of cards (palette strip +
  full name, selected card = accent outline + raised fill); segmented control
  for cursor style; section headers in small caps muted.
- **Keyboard Shortcuts dialog**: two columns of grouped sections, each binding
  shown as kbd chips; scrolls so the last row is never cut off.
- **Dialogs (all modal dialogs)**: footer separated by a border, 34px-high
  buttons right-aligned, order **Cancel, then primary/destructive**. Destructive
  button: `danger` fill, white text.
- **Quit dialog**: per mockup — icon tile with power icon, bold title, body text
  summarising open tabs ("3 open tabs: 2 terminals and 1 file…"), chips listing
  the open tabs, Cancel + Quit footer.

### Shortcut conflict
- Today `ReopenClosedSession` and `RevealInExplorer` both default to
  `Ctrl+Shift+R`, and `Ctrl+Shift+T` is `SearchAllSessions`.
- New defaults: "Reveal file in explorer" keeps `Ctrl+Shift+R`; "Reopen closed
  session" → `Ctrl+Shift+T` (browser convention); "Search all sessions" →
  `Ctrl+Shift+A` (builder verifies it is free; pick another free chord if not).
- Update all default tables (`shortcuts.rs`, `keybindings.rs`) and the
  hardcoded key handling in `app.rs` (~line 2983) so they agree.
- Defaults only; user overrides in `keybindings.json` are preserved. Add a test
  that no two default bindings collide.
- Settings hint text ("Reopen them later with Ctrl+Shift+T") and the Shortcuts
  dialog read from the registry, not hardcoded strings.

### Workspace colors
- The title bar no longer fills with the workspace color; it always uses `bg_panel`.
- Workspace colors are kept as small accents only: a color dot on workspace
  cards and session rows, and a thin (`TAB_COLOR_STRIP_W`) strip on tabs.
- The workspace color picker stays and keeps working.

### Title bar
- Keep every existing control (left/right panel toggles, Switcher, Recently
  Closed, Keyboard Shortcuts, Settings, system monitor, Update button), restyled
  with Lucide icons and `bg_panel`. No sun/theme toggle is added.

### Quick Switcher behavior
- Key handling is unchanged: typing filters immediately; `1`–`9` picks a
  workspace, then letters jump to a session.
- Row letter hints are shown only after a workspace is picked.
- Footer hints: `1-9` workspace, `Enter` open, `Esc` close (no `/` search hint).

### New theme: GitHub Light
- Add `ThemeId::GitHubLight` (16th theme) with a GitHub Light palette, serde
  name, ordering after Catppuccin Latte, and coverage in the contrast test —
  fills the Settings grid as 4×4.

### Mockup extras (user approved matching these)
- "Autosaved" label in the Notes header.
- `M` badge on modified files in the tree.
- Power icon + open-tab chips in the Quit dialog.
- Letter hotkeys on Quick Switcher rows (shown after a workspace is picked).
- GitHub Light theme.

## Constraints

- Rust, egui/eframe 0.28 — work within its styling model (`Style`, `Visuals`,
  painter). Set global `Visuals` from the active theme once per theme change.
- Follow `CLAUDE.md` conventions: all colors/sizes via `theme.rs` tokens,
  reusable components in `ui_kit`, small single-responsibility files, no
  monoliths, nothing on the UI thread that could block, no panics.
- Tests for every new helper/token (contrast, keybinding uniqueness,
  truncation helper, icon constant table, theme role completeness).
- Theme switching at runtime must still work instantly.

## Left to the builder

- Exact per-theme hex values for the 14 non-Nord themes (subject to the contrast test).
- Which font weights/subsets to bundle, within ~1 MB total.
- Final Lucide glyph list and the subsetting tool/process (document it so the
  subset can be regenerated).
- Mapping/renaming of existing `Theme` fields onto the roles above.
- Order of work and how to split it into reviewable steps.
- Spacing values where mockups and existing `SP_*` tokens differ slightly — prefer existing tokens.
