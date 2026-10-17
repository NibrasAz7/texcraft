//! Pure line-buffer operations behind the editor's line commands
//! (TeXstudio parity: duplicate, move, comment-toggle, erase).
//!
//! Conventions:
//! - `line` parameters are **0-based** line numbers; the UI converts its
//!   1-based `cursor_lc` at the call site.
//! - `col_chars` is a 0-based **char** column (chars before the caret).
//! - Byte offsets never cross a UTF-8 boundary: ranges are cut at `\n`
//!   (always a boundary) and column offsets come from `char_indices`.
//!
//! Newlines: `\n` is the line terminator. CRLF input keeps working —
//! the `\r` rides inside its line's byte range, so moves, duplicates and
//! erases carry it along, and `erase_to_eol` stops before `\r\n`. No
//! normalization is done.

/// Direction for [`move_line`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Move {
    Up,
    Down,
}

/// Byte range of every line in `text` — `[start, end)` where `end`
/// includes the trailing `\n` when the line has one. Always returns at
/// least one entry (empty text → `[(0, 0)]`, matching the editor's
/// single empty row). A trailing `\n` yields a final empty line, like
/// `str::split('\n')` and the galley rows.
pub fn line_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = 0usize;
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            out.push((start, i + 1));
            start = i + 1;
        }
    }
    out.push((start, text.len()));
    out
}

/// End of the line's content within `[s, e)` — before the `\n`, and
/// before a `\r` in CRLF input.
fn content_end(text: &str, s: usize, e: usize) -> usize {
    let mut end = e.min(text.len());
    if end > s && text.as_bytes()[end - 1] == b'\n' {
        end -= 1;
    }
    if end > s && text.as_bytes()[end - 1] == b'\r' {
        end -= 1;
    }
    end
}

/// Duplicate `line` in place; the copy lands directly below. Returns the
/// new caret line (0-based), `None` if `line` is out of range.
pub fn duplicate_line(text: &mut String, line: usize) -> Option<usize> {
    let (s, e) = *line_ranges(text).get(line)?;
    let seg = text.get(s..e)?.to_string();
    if seg.ends_with('\n') {
        text.insert_str(e, &seg);
    } else {
        // Last line carries no newline to copy: split it off with one
        // and append the content after it.
        text.insert_str(e, &format!("\n{seg}"));
    }
    Some(line + 1)
}

/// Swap `line` with its neighbour in `dir`; the caret follows the moved
/// line. Returns the new caret line, `None` at the buffer edges.
pub fn move_line(text: &mut String, line: usize, dir: Move) -> Option<usize> {
    let ranges = line_ranges(text);
    let other = match dir {
        Move::Up => line.checked_sub(1)?,
        Move::Down => line + 1,
    };
    let (a, b) = *ranges.get(other.min(line))?;
    let (c, d) = *ranges.get(other.max(line))?;
    if a == c {
        return None; // single line: nothing to swap
    }
    // [a,b) is the upper line — always newline-terminated since a line
    // follows it. [c,d) may be the last line without a newline: then the
    // moved-down line keeps its terminator and the moved-up line takes
    // over "no trailing newline" status.
    let upper = text.get(a..b)?.to_string();
    let lower = text.get(c..d)?.to_string();
    let mut mid = lower;
    if mid.ends_with('\n') {
        mid.push_str(&upper);
    } else {
        mid.push('\n');
        mid.push_str(upper.strip_suffix('\n').unwrap_or(&upper));
    }
    text.replace_range(a..d, &mid);
    Some(other)
}

/// Toggle `% ` comments over `first..=last` (0-based, clamped). If every
/// non-blank line in the range already starts (after whitespace) with
/// `%`, the command uncomments instead — TeXstudio's toggle rule.
/// Uncommenting strips the `%` plus one following space.
pub fn comment_lines(text: &mut String, first: usize, last: usize) {
    let ranges = line_ranges(text);
    if first >= ranges.len() {
        return;
    }
    let last = last.min(ranges.len().saturating_sub(1));
    if first > last {
        return;
    }
    // Direction vote: blank lines abstain.
    let mut nonblank = 0usize;
    let mut commented = 0usize;
    for &(s, e) in ranges.get(first..=last).unwrap_or(&[]) {
        let ce = content_end(text, s, e);
        let Some(ls) = text.get(s..ce) else { continue };
        let t = ls.trim_start();
        if t.is_empty() {
            continue;
        }
        nonblank += 1;
        if t.starts_with('%') {
            commented += 1;
        }
    }
    let uncomment = nonblank > 0 && commented == nonblank;
    // Apply bottom-up so byte offsets of earlier lines stay valid.
    for &(s, e) in ranges.get(first..=last).unwrap_or(&[]).iter().rev() {
        let ce = content_end(text, s, e);
        let Some(ls) = text.get(s..ce) else { continue };
        if uncomment {
            let ws = ls.len() - ls.trim_start().len();
            if !ls.trim_start().starts_with('%') {
                continue;
            }
            let mut drop = s + ws + 1; // the '%'
            if text.as_bytes().get(drop) == Some(&b' ') {
                drop += 1; // and one space after it
            }
            text.replace_range(s + ws..drop, "");
        } else if !ls.trim_start().starts_with('%') {
            text.insert_str(s, "% ");
        }
    }
}

/// Delete `line` including its newline; a last line without a newline
/// takes the separator before it. Returns the new caret line (clamped),
/// `None` if `line` is out of range.
pub fn erase_line(text: &mut String, line: usize) -> Option<usize> {
    let (s, e) = *line_ranges(text).get(line)?;
    let last = e == text.len() && !text.get(s..e).is_some_and(|l| l.ends_with('\n'));
    let ds = if last { s.saturating_sub(1) } else { s };
    if !text.is_char_boundary(ds) || !text.is_char_boundary(e) {
        return None;
    }
    text.replace_range(ds..e, "");
    let rows = line_ranges(text).len();
    Some(line.min(rows.saturating_sub(1)))
}

/// Delete from `col_chars` (0-based char column) to the end of `line`,
/// keeping the newline. Out-of-range or overshooting inputs are no-ops.
pub fn erase_to_eol(text: &mut String, line: usize, col_chars: usize) {
    let Some(&(s, e)) = line_ranges(text).get(line) else { return };
    let ce = content_end(text, s, e);
    let Some(ls) = text.get(s..ce) else { return };
    let off = ls.char_indices().nth(col_chars).map(|(i, _)| i).unwrap_or(ls.len());
    let from = s + off;
    if from < ce && text.is_char_boundary(from) && text.is_char_boundary(ce) {
        text.replace_range(from..ce, "");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ranges(t: &str) -> Vec<(usize, usize)> {
        line_ranges(t)
    }

    // ---------- line_ranges ----------

    #[test]
    fn ranges_empty_and_trailing() {
        assert_eq!(ranges(""), vec![(0, 0)]);
        assert_eq!(ranges("a"), vec![(0, 1)]);
        assert_eq!(ranges("a\nb"), vec![(0, 2), (2, 3)]);
        assert_eq!(ranges("a\n"), vec![(0, 2), (2, 2)]);
    }

    // ---------- duplicate_line ----------

    #[test]
    fn duplicate_first_middle_last() {
        let mut t = String::from("a\nb\nc\n");
        assert_eq!(duplicate_line(&mut t, 0), Some(1));
        assert_eq!(t, "a\na\nb\nc\n");

        let mut t = String::from("a\nb\nc\n");
        assert_eq!(duplicate_line(&mut t, 1), Some(2));
        assert_eq!(t, "a\nb\nb\nc\n");

        let mut t = String::from("a\nb\nc");
        assert_eq!(duplicate_line(&mut t, 2), Some(3));
        assert_eq!(t, "a\nb\nc\nc");
    }

    #[test]
    fn duplicate_empty_buffer_and_oob() {
        let mut t = String::new();
        assert_eq!(duplicate_line(&mut t, 0), Some(1));
        assert_eq!(t, "\n");
        let mut t = String::from("a");
        assert_eq!(duplicate_line(&mut t, 7), None);
        assert_eq!(t, "a");
    }

    // ---------- move_line ----------

    #[test]
    fn move_middle_up_and_down() {
        let mut t = String::from("a\nb\nc\n");
        assert_eq!(move_line(&mut t, 1, Move::Up), Some(0));
        assert_eq!(t, "b\na\nc\n");

        let mut t = String::from("a\nb\nc\n");
        assert_eq!(move_line(&mut t, 1, Move::Down), Some(2));
        assert_eq!(t, "a\nc\nb\n");
    }

    #[test]
    fn move_edges_and_last_line() {
        let mut t = String::from("a\nb\n");
        assert_eq!(move_line(&mut t, 0, Move::Up), None);
        assert_eq!(t, "a\nb\n");

        // Last line without trailing newline keeps the file newline-free.
        let mut t = String::from("a\nb");
        assert_eq!(move_line(&mut t, 1, Move::Up), Some(0));
        assert_eq!(t, "b\na");

        let mut t = String::from("a\nb");
        assert_eq!(move_line(&mut t, 0, Move::Down), Some(1));
        assert_eq!(t, "b\na");

        let mut t = String::from("a\nb");
        assert_eq!(move_line(&mut t, 1, Move::Down), None);
        assert_eq!(t, "a\nb");
    }

    #[test]
    fn move_empty_and_single() {
        let mut t = String::new();
        assert_eq!(move_line(&mut t, 0, Move::Up), None);
        assert_eq!(move_line(&mut t, 0, Move::Down), None);
    }

    // ---------- comment_lines ----------

    #[test]
    fn comment_and_uncomment() {
        let mut t = String::from("a\nb\nc\n");
        comment_lines(&mut t, 0, 2);
        assert_eq!(t, "% a\n% b\n% c\n");
        comment_lines(&mut t, 0, 2); // all commented → toggle off
        assert_eq!(t, "a\nb\nc\n");
    }

    #[test]
    fn comment_partial_range_and_indented() {
        let mut t = String::from("a\n  b\nc\n");
        comment_lines(&mut t, 1, 2);
        assert_eq!(t, "a\n%   b\n% c\n");
        // Mixed state comments again: only the uncommented line gains
        // a marker (already-commented lines are left alone).
        comment_lines(&mut t, 0, 2);
        assert_eq!(t, "% a\n%   b\n% c\n");
        // Indented comment strips at first non-ws char.
        let mut t = String::from("a\n  % b\n");
        comment_lines(&mut t, 0, 1);
        assert_eq!(t, "% a\n  % b\n");
        comment_lines(&mut t, 0, 1);
        assert_eq!(t, "a\n  b\n");
    }

    #[test]
    fn comment_blank_lines_abstain() {
        // One non-blank commented line → toggle uncomments only it.
        let mut t = String::from("% a\n\nb\n");
        comment_lines(&mut t, 0, 0);
        assert_eq!(t, "a\n\nb\n");
        // Blank lines get commented like any other (spec: "each line
        // that isn't already a comment").
        let mut t = String::from("\n");
        comment_lines(&mut t, 0, 0);
        assert_eq!(t, "% \n");
    }

    #[test]
    fn comment_empty_buffer() {
        let mut t = String::new();
        comment_lines(&mut t, 0, 0);
        assert_eq!(t, "% ");
    }

    // ---------- erase_line ----------

    #[test]
    fn erase_first_middle_last() {
        let mut t = String::from("a\nb\nc\n");
        assert_eq!(erase_line(&mut t, 0), Some(0));
        assert_eq!(t, "b\nc\n");

        let mut t = String::from("a\nb\nc\n");
        assert_eq!(erase_line(&mut t, 1), Some(1));
        assert_eq!(t, "a\nc\n");

        let mut t = String::from("a\nb\nc");
        assert_eq!(erase_line(&mut t, 2), Some(1));
        assert_eq!(t, "a\nb");

        // Erasing the trailing empty line removes the newline before
        // it and clamps the caret.
        let mut t = String::from("a\nb\nc\n");
        assert_eq!(erase_line(&mut t, 3), Some(2));
        assert_eq!(t, "a\nb\nc");
    }

    #[test]
    fn erase_to_empty_and_oob() {
        let mut t = String::from("only");
        assert_eq!(erase_line(&mut t, 0), Some(0));
        assert_eq!(t, "");
        assert_eq!(erase_line(&mut t, 9), None);
    }

    // ---------- erase_to_eol ----------

    #[test]
    fn erase_to_eol_basic() {
        let mut t = String::from("abcdef\nnext\n");
        erase_to_eol(&mut t, 0, 3);
        assert_eq!(t, "abc\nnext\n");
        erase_to_eol(&mut t, 0, 0);
        assert_eq!(t, "\nnext\n");
        // Column past the end: no-op.
        erase_to_eol(&mut t, 1, 99);
        assert_eq!(t, "\nnext\n");
        // Out-of-range line: no-op.
        erase_to_eol(&mut t, 42, 0);
        assert_eq!(t, "\nnext\n");
    }

    #[test]
    fn erase_to_eol_empty() {
        let mut t = String::new();
        erase_to_eol(&mut t, 0, 0);
        assert_eq!(t, "");
    }

    // ---------- CRLF ----------

    #[test]
    fn crlf_lines_stay_intact() {
        // \r travels with its line for dup/move/erase.
        let mut t = String::from("a\r\nb\r\nc\r\n");
        assert_eq!(duplicate_line(&mut t, 1), Some(2));
        assert_eq!(t, "a\r\nb\r\nb\r\nc\r\n");

        let mut t = String::from("a\r\nb\r\n");
        assert_eq!(move_line(&mut t, 1, Move::Up), Some(0));
        assert_eq!(t, "b\r\na\r\n");

        let mut t = String::from("a\r\nb\r\n");
        assert_eq!(erase_line(&mut t, 0), Some(0));
        assert_eq!(t, "b\r\n");

        // erase_to_eol stops before the \r\n terminator.
        let mut t = String::from("abc\r\ndef\r\n");
        erase_to_eol(&mut t, 0, 2);
        assert_eq!(t, "ab\r\ndef\r\n");
    }

    // ---------- hostile UTF-8 ----------

    #[test]
    fn unicode_heavy_buffer() {
        let mut t = String::from("α β γ\n\\section{汉语标题}\nemoji 🦀🦀\ncombining a\u{0301}\n");
        assert_eq!(duplicate_line(&mut t, 1), Some(2));
        assert_eq!(t, "α β γ\n\\section{汉语标题}\n\\section{汉语标题}\nemoji 🦀🦀\ncombining a\u{0301}\n");

        let mut t = String::from("α β γ\nemoji 🦀\n");
        assert_eq!(move_line(&mut t, 1, Move::Up), Some(0));
        assert_eq!(t, "emoji 🦀\nα β γ\n");

        // Col in chars, not bytes: "α β" is 3 chars but 5 bytes.
        let mut t = String::from("α β γ δ\n");
        erase_to_eol(&mut t, 0, 3);
        assert_eq!(t, "α β\n");

        let mut t = String::from("🦀\n汉\n");
        comment_lines(&mut t, 0, 1);
        assert_eq!(t, "% 🦀\n% 汉\n");

        let mut t = String::from("🦀\n汉");
        assert_eq!(erase_line(&mut t, 1), Some(0));
        assert_eq!(t, "🦀");
    }
}
