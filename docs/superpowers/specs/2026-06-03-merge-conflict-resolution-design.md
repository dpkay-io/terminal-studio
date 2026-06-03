# Merge Conflict Resolution — Design Spec

## Overview

A new `PaneContent::ConflictResolver` pane type that detects conflict markers (`<<<<<<<` / `=======` / `>>>>>>>`) in files, renders them as color-coded inline blocks with floating action bars, and writes resolutions back to disk immediately per-hunk. User stages resolved files manually via the existing git status UI.

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Conflict detection | Parse markers from file content | Simpler than git merge machinery; handles partial resolutions; works regardless of git state |
| Resolution granularity | Per-hunk inline | Each conflict block gets its own Ours/Theirs/Both controls (VS Code style) |
| Write behavior | Immediate write, manual stage | Instant feedback per-hunk; no surprise auto-staging; existing stage UI handles that |
| Pane type | Dedicated `ConflictResolver` | Keeps diff parser clean; full control over conflict UI; follows existing `PaneContent` pattern |
| Layout | Inline with floating actions | Single column; action bar floats above each conflict block; matches code-reading flow |
| Status panel | Separate "Conflicts" section at top | Conflicts are urgent; front-and-center visibility; hides when empty |

## 1. Conflict Detection

### Git Status Parsing

Extend `src/git/parser.rs`:

- Recognize unmerged status codes from `git status --porcelain`: `UU` (both modified), `AA` (both added), `DU`/`UD` (delete/modify conflicts)
- Add `FileChangeKind::Conflicted` variant
- Conflicted files appear in the new "Conflicts" section of the git status panel

### Conflict Marker Parsing

New module: `src/app/conflict_parser.rs`

**Input:** Raw file content read from disk.

**Parsing logic:**
1. Scan line-by-line for `<<<<<<<` markers
2. Lines between `<<<<<<<` and `=======` → ours
3. Lines between `=======` and `>>>>>>>` → theirs
4. Everything outside conflict blocks → context
5. Produce ordered `Vec<ConflictBlock>`

**Types:**

```rust
pub struct ConflictFile {
    pub path: PathBuf,
    pub blocks: Vec<ConflictBlock>,
    pub total_conflicts: usize,
}

pub enum ConflictBlock {
    Context {
        lines: Vec<String>,
    },
    Conflict {
        index: usize, // 0-based conflict number
        ours_lines: Vec<String>,
        theirs_lines: Vec<String>,
        ours_label: String,   // text after <<<<<<< (e.g., "HEAD")
        theirs_label: String, // text after >>>>>>> (e.g., "feature-branch")
        resolved: Option<Resolution>,
    },
}

pub enum Resolution {
    Ours,
    Theirs,
    Both, // ours lines followed by theirs lines
}
```

**Edge cases:**
- Nested conflict markers (shouldn't happen in practice — treat as malformed, show raw lines as context)
- Missing `=======` or `>>>>>>>` — treat as malformed, show raw lines
- Empty ours or theirs section — valid, render empty block

## 2. Git Status Panel — Conflicts Section

**Location:** `src/app/git_diff.rs`, rendered at the top of the git status panel, above staged files.

**Rendering:**
- Section header: warning-colored background using `theme.warning`, text: "Conflicts — N files"
- Each file listed with a conflict icon/indicator
- File entries are clickable

**Behavior:**
- Click a conflicted file → open `ConflictResolver` pane (not `FileDiff`)
- If a `ConflictResolver` pane already exists for that file, focus it instead of creating a duplicate
- When a file has all conflicts resolved (written to disk, markers gone), it disappears from this section and appears in the unstaged section on next git status refresh
- When no conflicted files remain, the section hides entirely

## 3. ConflictResolver Pane

### Pane Content Variant

Add to `PaneContent` enum in `src/app/pane.rs`:

```rust
ConflictResolver(ConflictResolverState)
```

**State struct:**

```rust
pub struct ConflictResolverState {
    pub path: PathBuf,
    pub content: ConflictFile, // parsed conflict blocks
    pub resolved_count: usize,
    pub scroll_offset: f32,
}
```

### Toolbar

Rendered at the top of the pane:

- **Left:** File path (relative to workspace root)
- **Center:** Progress indicator — "2/5 resolved"
- **Right:** "Accept All Ours" and "Accept All Theirs" bulk action buttons

### Rendering

New UI file: `src/app/ui/conflict_resolver.rs`

**Context lines:**
- Normal dark background
- Line numbers in gutter
- Standard text color

**Unresolved conflict block:**
- **Floating action bar** at the top of the block:
  - Left: "CONFLICT 1/5" label (bold, using theme text color)
  - Right: three buttons — "Ours" (green bg), "Theirs" (red bg), "Both" (blue/accent bg)
- **Ours section:** green-tinted background (`theme.success` at `BLEND_SUBTLE`), 3px green left border
- **Separator:** styled horizontal rule representing the `=======` marker
- **Theirs section:** red-tinted background (`theme.error` at `BLEND_SUBTLE`), 3px red left border

**Resolved conflict block:**
- Shows only the chosen lines
- Muted styling — slightly dimmed background to indicate "handled"
- Small label: "Resolved: Ours" / "Resolved: Theirs" / "Resolved: Both" in subtle text

**Colors (from theme.rs tokens):**
- Ours background: `theme.success` blended at `BLEND_SUBTLE` (0.15) with surface
- Theirs background: `theme.error` blended at `BLEND_SUBTLE` (0.15) with surface
- Ours border/text: `theme.success`
- Theirs border/text: `theme.error`
- Action bar buttons: `theme.success` (Ours), `theme.error` (Theirs), `theme.accent` (Both)
- Floating bar background: surface with slight elevation

## 4. Resolution Flow

1. User clicks "Ours" / "Theirs" / "Both" on a conflict hunk's floating action bar
2. `ConflictResolverState` updates: marks that `ConflictBlock::Conflict` as `resolved: Some(Resolution::Xxx)`
3. `resolved_count` increments, toolbar progress updates
4. File is rewritten to disk immediately:
   - Reconstruct full file content from blocks
   - Resolved conflicts: substitute with chosen lines (no markers)
   - Unresolved conflicts: preserve original markers
   - Use `atomic_write()` from `src/util.rs`
5. Re-parse the file from disk to ensure state matches actual file content
6. UI re-renders showing the resolved block in muted style
7. When all conflicts resolved:
   - Trigger `FlashManager` success flash on the pane
   - File watcher picks up the change → git status refreshes → file moves from Conflicts section to Unstaged section
8. User stages the file manually via the existing stage button in the git status panel

### Bulk Actions

"Accept All Ours" / "Accept All Theirs":
- Iterate all unresolved `ConflictBlock::Conflict` entries
- Set `resolved` to the chosen `Resolution`
- Write file once (not per-hunk)
- Update `resolved_count` to `total_conflicts`

## 5. File Writing

In `src/app/conflict_parser.rs`:

```rust
pub fn write_resolved_file(path: &Path, blocks: &[ConflictBlock]) -> anyhow::Result<()>
```

**Logic:**
1. Build a `String` from blocks:
   - `Context { lines }` → append lines as-is
   - `Conflict { resolved: Some(Ours), .. }` → append `ours_lines` only
   - `Conflict { resolved: Some(Theirs), .. }` → append `theirs_lines` only
   - `Conflict { resolved: Some(Both), .. }` → append `ours_lines` then `theirs_lines`
   - `Conflict { resolved: None, .. }` → re-emit full conflict markers (`<<<<<<<`, ours, `=======`, theirs, `>>>>>>>`)
2. Write via `atomic_write()` for crash-safety
3. File watcher detects the change and triggers git status refresh

## 6. Integration Points

| File | Change |
|------|--------|
| `src/git/parser.rs` | Add `FileChangeKind::Conflicted`; parse `UU`/`AA`/`DU`/`UD` status codes |
| `src/app/conflict_parser.rs` | **New file** — conflict marker parser, `ConflictFile`/`ConflictBlock`/`Resolution` types, `write_resolved_file()` |
| `src/app/pane.rs` | Add `PaneContent::ConflictResolver(ConflictResolverState)` variant |
| `src/app/git_diff.rs` | Add "Conflicts" section at top of status panel; route clicks to `ConflictResolver` pane |
| `src/app/ui/conflict_resolver.rs` | **New file** — conflict resolver pane rendering (toolbar, conflict blocks, action buttons) |
| `src/app/ui/mod.rs` | Register `conflict_resolver` module |
| `src/app/ui/pane_renderer.rs` | Dispatch `PaneContent::ConflictResolver` to `conflict_resolver::render()` |
| `src/app.rs` | Handle conflict pane creation from git status panel clicks |
| `src/app/persistence.rs` | Skip serialization of `ConflictResolver` panes (conflicts are transient) |
| `src/shortcuts.rs` | No new shortcuts needed (actions are mouse-driven within the pane) |

## 7. Error Handling

| Scenario | Behavior |
|----------|----------|
| File deleted externally while resolving | Re-read fails → flash error "File no longer exists", close pane |
| File modified externally (markers changed) | Re-parse after write detects mismatch → flash warning "File changed externally", re-parse and reset unresolved blocks |
| No conflict markers in a `UU` file | Show "No conflict markers detected" message in pane body |
| Write failure (permissions, disk full) | Flash error, don't update resolved state, leave markers in place |
| Malformed markers (missing `=======` or `>>>>>>>`) | Treat the malformed region as context lines, show as-is |

## 8. Out of Scope

- Manual text editing within the conflict view (user opens file in external editor for that)
- Three-way merge base view
- Undo/redo of individual resolutions
- Persisting `ConflictResolver` panes across app restart (conflicts are transient state)
- Syntax highlighting within conflict blocks (plain text only for v1)

## 9. Test Coverage

| Module | Tests |
|--------|-------|
| `conflict_parser.rs` | Parse simple conflict, multi-hunk conflicts, nested/malformed markers, empty ours/theirs, no conflicts in file, labels extraction, `write_resolved_file` for each `Resolution` variant, partial resolution (some resolved + some not), bulk resolve |
| `git/parser.rs` | Parse `UU`, `AA`, `DU`, `UD` status codes → `Conflicted` kind |
| `conflict_resolver.rs` (UI) | State transitions: resolve single hunk, resolve all, progress counting |
