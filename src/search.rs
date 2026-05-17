#![allow(dead_code)]
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
            for col in 0..cols {
                let cell = &grid[Line(line_idx)][Column(col)];
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    continue;
                }
                line_text.push(cell.c);
            }

            let line_lower = line_text.to_lowercase();
            let mut search_from = 0;
            while let Some(pos) = line_lower[search_from..].find(&query_lower) {
                let start = search_from + pos;
                let end = start + query_lower.len();
                self.matches.push(SearchMatch {
                    line: line_idx,
                    start_col: start,
                    end_col: end,
                });
                search_from = start + 1;
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
                self.current_index =
                    Some(if idx == 0 { self.matches.len() - 1 } else { idx - 1 });
            }
        }
    }

    pub fn current_match(&self) -> Option<&SearchMatch> {
        self.current_index.and_then(|i| self.matches.get(i))
    }
}
