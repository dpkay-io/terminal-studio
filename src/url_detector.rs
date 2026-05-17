use std::sync::OnceLock;

use regex::Regex;

static URL_REGEX: OnceLock<Regex> = OnceLock::new();

fn url_regex() -> &'static Regex {
    URL_REGEX.get_or_init(|| Regex::new(r"https?://[^\s<>\[\]\{\}|\\^`\x00-\x1f\x7f]+").unwrap())
}

pub struct DetectedUrl {
    pub line: i32,
    pub start_col: usize,
    pub end_col: usize,
    pub url: String,
}

pub fn detect_urls(lines: &[(i32, String)]) -> Vec<DetectedUrl> {
    let re = url_regex();
    let mut urls = Vec::new();
    for (line_idx, text) in lines {
        for m in re.find_iter(text) {
            urls.push(DetectedUrl {
                line: *line_idx,
                start_col: m.start(),
                end_col: m.end(),
                url: m.as_str().to_string(),
            });
        }
    }
    urls
}

pub fn url_at_position(urls: &[DetectedUrl], line: i32, col: usize) -> Option<&str> {
    urls.iter()
        .find(|u| u.line == line && col >= u.start_col && col < u.end_col)
        .map(|u| u.url.as_str())
}
