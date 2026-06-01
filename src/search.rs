use std::sync::Arc;

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use parking_lot::RwLock;

use crate::terminal::Session;

pub struct SearchMatch {
    pub line: i32,
    pub start_col: usize,
    pub end_col: usize,
}

pub struct SearchState {
    pub query: String,
    pub matches: Vec<SearchMatch>,
    pub current_index: Option<usize>,
    pub active: bool,
}

impl SearchState {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            matches: Vec::new(),
            current_index: None,
            active: false,
        }
    }

    pub fn search(&mut self, session: &Arc<RwLock<Session>>) {
        self.matches.clear();
        self.current_index = None;
        if self.query.is_empty() {
            return;
        }

        let session = session.read();
        let term = &session.term;
        let grid = term.grid();
        let cols = term.columns();
        let total_lines = term.screen_lines() as i32;
        let history = grid.history_size() as i32;

        let query_lower = self.query.to_lowercase();

        for line_idx in (-history)..total_lines {
            let mut line_text = String::with_capacity(cols);
            let mut char_to_col: Vec<usize> = Vec::with_capacity(cols);
            for col in 0..cols {
                let cell = &grid[Line(line_idx)][Column(col)];
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    continue;
                }
                char_to_col.push(col);
                line_text.push(cell.c);
            }

            let line_lower = line_text.to_lowercase();
            let mut search_from = 0;
            while let Some(byte_pos) = line_lower[search_from..].find(&query_lower) {
                let match_byte_start = search_from + byte_pos;
                let match_byte_end = match_byte_start + query_lower.len();
                let char_start = line_lower[..match_byte_start].chars().count();
                let char_len = line_lower[match_byte_start..match_byte_end].chars().count();
                let start_col = char_to_col.get(char_start).copied().unwrap_or(char_start);
                let end_col = char_to_col
                    .get(char_start + char_len)
                    .copied()
                    .unwrap_or_else(|| start_col + char_len);
                self.matches.push(SearchMatch {
                    line: line_idx,
                    start_col,
                    end_col,
                });
                search_from = match_byte_start + 1;
            }
        }

        if !self.matches.is_empty() {
            self.current_index = Some(0);
        }
    }

    pub fn next_match(&mut self) {
        if let Some(idx) = self.current_index {
            if !self.matches.is_empty() {
                self.current_index = Some((idx + 1) % self.matches.len());
            }
        }
    }

    pub fn prev_match(&mut self) {
        if let Some(idx) = self.current_index {
            if !self.matches.is_empty() {
                self.current_index = Some(if idx == 0 {
                    self.matches.len() - 1
                } else {
                    idx - 1
                });
            }
        }
    }

    pub fn current_match(&self) -> Option<&SearchMatch> {
        self.current_index.and_then(|i| self.matches.get(i))
    }
}

// ── Plain-text search (for file editors, notes, diffs) ─────────────────

#[allow(dead_code)]
pub struct TextSearchMatch {
    pub line: usize,
    pub col_start: usize,
    pub col_end: usize,
}

pub struct TextSearchState {
    pub query: String,
    pub matches: Vec<TextSearchMatch>,
    pub current_index: Option<usize>,
    pub active: bool,
}

impl TextSearchState {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            matches: Vec::new(),
            current_index: None,
            active: false,
        }
    }

    pub fn search(&mut self, content: &str) {
        self.matches.clear();
        self.current_index = None;
        if self.query.is_empty() {
            return;
        }

        let query_lower = self.query.to_lowercase();

        for (line_idx, line) in content.lines().enumerate() {
            let line_lower = line.to_lowercase();
            let mut search_from = 0;
            while let Some(byte_pos) = line_lower[search_from..].find(&query_lower) {
                let abs_start = search_from + byte_pos;
                let abs_end = abs_start + query_lower.len();
                let col_start = line_lower[..abs_start].chars().count();
                let col_end = col_start + line_lower[abs_start..abs_end].chars().count();
                self.matches.push(TextSearchMatch {
                    line: line_idx,
                    col_start,
                    col_end,
                });
                search_from = abs_start + 1;
            }
        }

        if !self.matches.is_empty() {
            self.current_index = Some(0);
        }
    }

    pub fn next_match(&mut self) {
        if let Some(idx) = self.current_index {
            if !self.matches.is_empty() {
                self.current_index = Some((idx + 1) % self.matches.len());
            }
        }
    }

    pub fn prev_match(&mut self) {
        if let Some(idx) = self.current_index {
            if !self.matches.is_empty() {
                self.current_index = Some(if idx == 0 {
                    self.matches.len() - 1
                } else {
                    idx - 1
                });
            }
        }
    }

    pub fn current_match(&self) -> Option<&TextSearchMatch> {
        self.current_index.and_then(|i| self.matches.get(i))
    }

    pub fn clear(&mut self) {
        self.query.clear();
        self.matches.clear();
        self.current_index = None;
        self.active = false;
    }
}

#[cfg(test)]
mod text_search_tests {
    use super::*;

    #[test]
    fn empty_query_no_matches() {
        let mut s = TextSearchState::new();
        s.search("hello world");
        assert!(s.matches.is_empty());
        assert_eq!(s.current_index, None);
    }

    #[test]
    fn simple_match() {
        let mut s = TextSearchState::new();
        s.query = "hello".to_string();
        s.search("hello world");
        assert_eq!(s.matches.len(), 1);
        assert_eq!(s.matches[0].line, 0);
        assert_eq!(s.matches[0].col_start, 0);
        assert_eq!(s.matches[0].col_end, 5);
        assert_eq!(s.current_index, Some(0));
    }

    #[test]
    fn case_insensitive() {
        let mut s = TextSearchState::new();
        s.query = "HELLO".to_string();
        s.search("Hello World\nhello again");
        assert_eq!(s.matches.len(), 2);
        assert_eq!(s.matches[0].line, 0);
        assert_eq!(s.matches[1].line, 1);
    }

    #[test]
    fn multiple_matches_per_line() {
        let mut s = TextSearchState::new();
        s.query = "ab".to_string();
        s.search("ab cd ab ef ab");
        assert_eq!(s.matches.len(), 3);
        assert_eq!(s.matches[0].col_start, 0);
        assert_eq!(s.matches[1].col_start, 6);
        assert_eq!(s.matches[2].col_start, 12);
    }

    #[test]
    fn multi_line_content() {
        let mut s = TextSearchState::new();
        s.query = "fn".to_string();
        s.search("fn main() {\n    fn helper() {\n    }\n}");
        assert_eq!(s.matches.len(), 2);
        assert_eq!(s.matches[0].line, 0);
        assert_eq!(s.matches[1].line, 1);
    }

    #[test]
    fn no_match_returns_empty() {
        let mut s = TextSearchState::new();
        s.query = "xyz".to_string();
        s.search("hello world");
        assert!(s.matches.is_empty());
        assert_eq!(s.current_index, None);
    }

    #[test]
    fn next_wraps_around() {
        let mut s = TextSearchState::new();
        s.query = "a".to_string();
        s.search("a b a");
        assert_eq!(s.current_index, Some(0));
        s.next_match();
        assert_eq!(s.current_index, Some(1));
        s.next_match();
        assert_eq!(s.current_index, Some(0));
    }

    #[test]
    fn prev_wraps_around() {
        let mut s = TextSearchState::new();
        s.query = "a".to_string();
        s.search("a b a");
        assert_eq!(s.current_index, Some(0));
        s.prev_match();
        assert_eq!(s.current_index, Some(1));
        s.prev_match();
        assert_eq!(s.current_index, Some(0));
    }

    #[test]
    fn clear_resets_all() {
        let mut s = TextSearchState::new();
        s.query = "test".to_string();
        s.active = true;
        s.search("test data");
        assert!(!s.matches.is_empty());
        s.clear();
        assert!(s.query.is_empty());
        assert!(s.matches.is_empty());
        assert_eq!(s.current_index, None);
        assert!(!s.active);
    }

    #[test]
    fn current_match_returns_correct() {
        let mut s = TextSearchState::new();
        s.query = "x".to_string();
        s.search("x y\nz x");
        assert_eq!(s.current_match().unwrap().line, 0);
        s.next_match();
        assert_eq!(s.current_match().unwrap().line, 1);
    }

    #[test]
    fn empty_content() {
        let mut s = TextSearchState::new();
        s.query = "test".to_string();
        s.search("");
        assert!(s.matches.is_empty());
    }
}
