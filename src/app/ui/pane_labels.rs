use super::super::pane::PaneContent;
use super::super::title::effective_title;
use super::super::App;

impl App {
    /// Compute the display text for a tab at the given pane index.
    #[allow(dead_code)]
    pub(in crate::app) fn tab_display_text(&self, pane_index: usize) -> String {
        match &self.pane_state.panes[pane_index].content {
            PaneContent::Terminal(sid) => {
                let sid = *sid;
                self.session_state
                    .sessions
                    .iter()
                    .find(|e| e.id == sid)
                    .map(|e| {
                        let s = e.session.read();
                        let title = s.title();
                        let cwd = s.cwd.clone();
                        drop(s);
                        let fg = self.workers.foreground_worker.get(e.id);
                        let ws_name = if cwd.as_os_str().is_empty() {
                            None
                        } else {
                            self.workspace_store
                                .find_for_cwd(&cwd)
                                .map(|w| w.name.clone())
                        };
                        effective_title(
                            &title,
                            &cwd,
                            fg.as_ref(),
                            Some(&e.shell),
                            ws_name.as_deref(),
                        )
                    })
                    .unwrap_or_else(|| format!("Terminal {sid}"))
            }
            PaneContent::DeferredTerminal {
                cwd, saved_title, ..
            } => {
                if let Some(t) = saved_title.as_deref().filter(|s| !s.is_empty()) {
                    return t.to_string();
                }
                let cwd_path = cwd.clone().unwrap_or_default();
                let ws_name = if cwd_path.as_os_str().is_empty() {
                    None
                } else {
                    self.workspace_store
                        .find_for_cwd(&cwd_path)
                        .map(|w| w.name.clone())
                };
                effective_title("", &cwd_path, None, None, ws_name.as_deref())
            }
            PaneContent::FileEditor(ed) => {
                let fname = ed
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if ed.save_error {
                    format!("! {fname}")
                } else if ed.dirty {
                    format!("* {fname}")
                } else {
                    fname
                }
            }
            PaneContent::FileDiff(d) => {
                let fname = d
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                format!("\u{21c4} {fname}")
            }
            PaneContent::NoteEditor(_) => "Notes".to_string(),
            PaneContent::ConflictResolver(cr) => cr
                .path
                .file_name()
                .map(|n| format!("\u{26a0} {}", n.to_string_lossy()))
                .unwrap_or_else(|| "Conflicts".to_string()),
        }
    }

    /// Compute the tooltip text for a tab: full title + CWD + workspace name.
    pub(in crate::app) fn tab_tooltip_text(&self, pane_index: usize) -> String {
        match &self.pane_state.panes[pane_index].content {
            PaneContent::Terminal(sid) => {
                let sid = *sid;
                let Some(e) = self.session_state.sessions.iter().find(|e| e.id == sid) else {
                    return format!("Terminal {sid}");
                };
                let s = e.session.read();
                let title = s.title();
                let cwd = s.cwd.clone();
                drop(s);
                let ws_name = if cwd.as_os_str().is_empty() {
                    None
                } else {
                    self.workspace_store
                        .find_for_cwd(&cwd)
                        .map(|w| w.name.clone())
                };
                let fg = self.workers.foreground_worker.get(e.id);
                let display = effective_title(
                    &title,
                    &cwd,
                    fg.as_ref(),
                    Some(&e.shell),
                    ws_name.as_deref(),
                );
                let mut tip = display;
                let cwd_str = cwd.to_string_lossy();
                if !cwd_str.is_empty() {
                    tip.push('\n');
                    tip.push_str(&cwd_str);
                }
                if let Some(name) = &ws_name {
                    tip.push('\n');
                    tip.push_str(name);
                }
                tip
            }
            PaneContent::DeferredTerminal {
                cwd, saved_title, ..
            } => {
                let mut tip = saved_title
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .unwrap_or("Terminal")
                    .to_string();
                if let Some(c) = cwd {
                    let s = c.to_string_lossy();
                    if !s.is_empty() {
                        tip.push('\n');
                        tip.push_str(&s);
                    }
                }
                tip
            }
            PaneContent::FileEditor(ed) => ed.path.to_string_lossy().into_owned(),
            PaneContent::FileDiff(d) => format!("Diff: {}", d.path.to_string_lossy()),
            PaneContent::NoteEditor(_) => "Notes".to_string(),
            PaneContent::ConflictResolver(cr) => {
                format!("Conflicts: {}", cr.path.to_string_lossy())
            }
        }
    }

    /// Workspace color for a pane (by index), if the pane belongs to a workspace.
    pub(in crate::app) fn ws_color_for_pane(&self, pane_index: usize) -> Option<[u8; 3]> {
        match &self.pane_state.panes[pane_index].content {
            PaneContent::Terminal(sid) => self.session_state.find(*sid).and_then(|e| {
                let cwd = e.session.read().cwd.clone();
                if cwd.as_os_str().is_empty() {
                    return None;
                }
                self.workspace_store.find_for_cwd(&cwd).map(|w| w.color)
            }),
            PaneContent::DeferredTerminal { cwd, .. } => cwd
                .as_ref()
                .and_then(|c| self.workspace_store.find_for_cwd(c).map(|w| w.color)),
            PaneContent::FileEditor(ed) => ed.workspace_id.and_then(|id| {
                self.workspace_store
                    .workspaces
                    .iter()
                    .find(|w| w.id == id)
                    .map(|w| w.color)
            }),
            PaneContent::NoteEditor(ne) => ne.workspace_id.and_then(|id| {
                self.workspace_store
                    .workspaces
                    .iter()
                    .find(|w| w.id == id)
                    .map(|w| w.color)
            }),
            PaneContent::FileDiff(d) => d.workspace_id.and_then(|id| {
                self.workspace_store
                    .workspaces
                    .iter()
                    .find(|w| w.id == id)
                    .map(|w| w.color)
            }),
            PaneContent::ConflictResolver(_) => None,
        }
    }
}
