//! Folders and files to open that arrive from a background thread: the
//! folder picker, and paths dropped from the OS once a thread has sorted
//! them into folders and files. Each request names the window it was asked
//! in, and only that window's frame takes it, so it opens where the user
//! asked.

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;

use crate::editor_group::GroupId;
use crate::workspace::WindowId;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct OpenRequest {
    /// The window asked in; None = the main window.
    pub(super) window: Option<WindowId>,
    /// Shown in the Open Folder dialog.
    pub(super) folders: Vec<PathBuf>,
    /// Opened in the editor.
    pub(super) files: Vec<PathBuf>,
    /// The split files open in; None = the focused one.
    pub(super) target_group: Option<GroupId>,
    /// Paths that could not be opened (missing, too big, not text).
    pub(super) skipped: Vec<PathBuf>,
}

/// Shared between the UI thread and the threads that fill it.
#[derive(Clone, Default)]
pub(super) struct OpenQueue(Arc<Mutex<Vec<OpenRequest>>>);

impl OpenQueue {
    /// Queues `request` and wakes the UI to take it.
    pub(super) fn push(&self, request: OpenRequest, ctx: &egui::Context) {
        self.0.lock().push(request);
        ctx.request_repaint();
    }

    /// Removes and returns the requests for `window`, oldest first. Cheap
    /// when there are none, as on almost every frame.
    pub(super) fn take_for(&self, window: Option<&WindowId>) -> Vec<OpenRequest> {
        let mut queue = self.0.lock();
        if queue.is_empty() {
            return Vec::new();
        }
        let (mine, rest): (Vec<OpenRequest>, Vec<OpenRequest>) =
            queue.drain(..).partition(|r| r.window.as_ref() == window);
        *queue = rest;
        mine
    }

    /// Drops requests for windows that no longer exist, so a window closed
    /// before its picker returned leaves nothing behind.
    pub(super) fn forget_windows_except(&self, open: &[WindowId]) {
        self.0
            .lock()
            .retain(|r| r.window.as_ref().is_none_or(|w| open.contains(w)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(window: Option<u64>, folder: &str) -> OpenRequest {
        OpenRequest {
            window: window.map(WindowId),
            folders: vec![PathBuf::from(folder)],
            ..Default::default()
        }
    }

    #[test]
    fn each_window_takes_only_its_own_requests_in_order() {
        let ctx = egui::Context::default();
        let queue = OpenQueue::default();
        queue.push(request(None, "/a"), &ctx);
        queue.push(request(Some(7), "/b"), &ctx);
        queue.push(request(None, "/c"), &ctx);

        let main = queue.take_for(None);
        assert_eq!(main, vec![request(None, "/a"), request(None, "/c")]);
        assert!(queue.take_for(None).is_empty(), "taken once");
        assert_eq!(
            queue.take_for(Some(&WindowId(7))),
            vec![request(Some(7), "/b")]
        );
        assert!(queue.take_for(Some(&WindowId(7))).is_empty());
    }

    #[test]
    fn requests_for_closed_windows_are_dropped() {
        let ctx = egui::Context::default();
        let queue = OpenQueue::default();
        queue.push(request(Some(1), "/a"), &ctx);
        queue.push(request(Some(2), "/b"), &ctx);
        queue.push(request(None, "/c"), &ctx);
        queue.forget_windows_except(&[WindowId(2)]);
        assert!(queue.take_for(Some(&WindowId(1))).is_empty());
        assert_eq!(queue.take_for(Some(&WindowId(2))).len(), 1);
        assert_eq!(
            queue.take_for(None).len(),
            1,
            "the main window always stays"
        );
    }

    #[test]
    fn clones_share_one_queue() {
        let ctx = egui::Context::default();
        let queue = OpenQueue::default();
        let thread_side = queue.clone();
        std::thread::spawn(move || thread_side.push(request(None, "/t"), &ctx))
            .join()
            .unwrap();
        assert_eq!(queue.take_for(None).len(), 1);
    }
}
