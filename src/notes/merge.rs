//! Joining two versions of a note so that no text is lost.
//!
//! Used wherever two copies of a note meet: linking a note to a file (the
//! file's text and the note's), unlinking it, and an edit here racing an
//! edit made in another app. The other side's text is kept as it is and
//! every paragraph of ours it does not already contain is added after it,
//! so text both sides share appears once.

/// Line ending used by a text written on Windows (Notepad++ and friends).
const CRLF: &str = "\r\n";
const LF: &str = "\n";
/// What separates the paragraphs of a note.
const PARAGRAPH_BREAK: &str = "\n\n";

/// `theirs`, followed by every paragraph of `ours` that `theirs` does not
/// already contain. Paragraphs are separated by blank lines; one counts as
/// present when its text (trimmed) appears anywhere in the result, so a
/// paragraph shared by both sides, or repeated in `ours`, is written once.
///
/// When one side already holds all of the other, it is returned unchanged:
/// `ours` grown from `theirs` (lines typed at the end) stays as typed.
/// Line endings are compared as `\n`; added text takes `theirs`'s style.
pub fn merge_note_texts(theirs: &str, ours: &str) -> String {
    let theirs_norm = normalize(theirs);
    let ours_norm = normalize(ours);
    let theirs_core = theirs_norm.trim();
    let ours_core = ours_norm.trim();

    if ours_core.is_empty() || theirs_core.contains(ours_core) {
        return theirs.to_string();
    }
    if theirs_core.is_empty() || ours_core.contains(theirs_core) {
        return ours.to_string();
    }

    let mut merged = theirs_norm.trim_end().to_string();
    for paragraph in paragraphs(&ours_norm) {
        if merged.contains(paragraph.trim()) {
            continue;
        }
        merged.push_str(PARAGRAPH_BREAK);
        merged.push_str(paragraph);
    }
    merged.push_str(LF);

    if uses_crlf(theirs) {
        merged.replace(LF, CRLF)
    } else {
        merged
    }
}

/// `text` with Windows line endings turned into `\n`.
fn normalize(text: &str) -> String {
    text.replace(CRLF, LF)
}

/// Whether `text` was written with Windows line endings.
fn uses_crlf(text: &str) -> bool {
    text.contains(CRLF)
}

/// The paragraphs of `text`: runs of lines that are not blank, each
/// without its line break at the end. A paragraph's indentation is kept.
fn paragraphs(text: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut start: Option<usize> = None;
    let mut end = 0;
    let mut offset = 0;
    for line in text.split_inclusive(LF) {
        if line.trim().is_empty() {
            if let Some(s) = start.take() {
                found.push(text[s..end].trim_end());
            }
        } else {
            start.get_or_insert(offset);
            end = offset + line.len();
        }
        offset += line.len();
    }
    if let Some(s) = start {
        found.push(text[s..end].trim_end());
    }
    found
}

#[cfg(test)]
mod tests {
    use super::merge_note_texts as merge;

    #[test]
    fn an_empty_side_gives_the_other() {
        assert_eq!(merge("", "mine"), "mine");
        assert_eq!(merge("theirs", ""), "theirs");
        assert_eq!(merge("  \n\n", "mine"), "mine");
        assert_eq!(merge("theirs", " \n "), "theirs");
        assert_eq!(merge("", ""), "");
    }

    #[test]
    fn identical_texts_stay_as_they_are() {
        assert_eq!(merge("a\n\nb\n", "a\n\nb\n"), "a\n\nb\n");
    }

    #[test]
    fn a_side_holding_all_of_the_other_wins_unchanged() {
        // Typed at the end here: ours grew from theirs.
        assert_eq!(merge("todo", "todo\n- more"), "todo\n- more");
        // Typed at the end in the other app.
        assert_eq!(merge("todo\n- more", "todo"), "todo\n- more");
    }

    #[test]
    fn paragraphs_only_we_have_are_added_after_theirs() {
        let theirs = "# Vault note\n\nshared idea\n";
        let ours = "shared idea\n\nlocal only\nsecond line";
        assert_eq!(
            merge(theirs, ours),
            "# Vault note\n\nshared idea\n\nlocal only\nsecond line\n"
        );
    }

    #[test]
    fn nothing_is_lost_when_both_sides_differ() {
        let merged = merge("alpha", "beta");
        assert!(merged.contains("alpha"));
        assert!(merged.contains("beta"));
        assert_eq!(merged, "alpha\n\nbeta\n");
    }

    #[test]
    fn a_paragraph_repeated_in_ours_is_added_once() {
        assert_eq!(merge("x", "y\n\ny\n\nz"), "x\n\ny\n\nz\n");
    }

    #[test]
    fn several_blank_lines_and_indented_blank_lines_split_paragraphs() {
        assert_eq!(merge("x", "a\n\n\n\nb\n  \t\nc"), "x\n\na\n\nb\n\nc\n");
    }

    #[test]
    fn merging_again_adds_nothing_new() {
        let once = merge("theirs\n\nshared", "shared\n\nmine");
        assert_eq!(merge(&once, "shared\n\nmine"), once);
        assert_eq!(merge(&once, "theirs\n\nshared"), once);
    }

    #[test]
    fn windows_line_endings_are_compared_as_unix_ones() {
        // Same text, saved on Windows: nothing is added twice.
        assert_eq!(merge("a\r\nb\r\n", "a\nb\n"), "a\r\nb\r\n");
        // Added text follows the file's own line endings.
        assert_eq!(merge("a\r\n", "b\nc"), "a\r\n\r\nb\r\nc\r\n");
        // A file with Unix endings stays that way.
        assert_eq!(merge("a\n", "b\r\nc"), "a\n\nb\nc\n");
    }

    #[test]
    fn an_added_paragraph_keeps_its_indentation() {
        assert_eq!(
            merge("x", "    code()\n    more()"),
            "x\n\n    code()\n    more()\n"
        );
        // Present with other indentation: already there.
        assert_eq!(merge("code()\n\nx", "y\n\n  code()"), "code()\n\nx\n\ny\n");
    }

    #[test]
    fn non_ascii_text_is_kept_whole() {
        let merged = merge("café ☕", "проект\n\n日本語");
        assert_eq!(merged, "café ☕\n\nпроект\n\n日本語\n");
    }
}
