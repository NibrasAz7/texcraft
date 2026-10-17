//! Block folding (TeXstudio parity): collapse a section or environment
//! to its header line. `TextEdit` cannot hide text, so the editor shows
//! a derived buffer — each collapsed region renders as its first line
//! plus a `% … N lines` comment — and becomes read-only while any fold
//! is active (`folded` is cleared on the next edit, so the derived text
//! can never be written back to the session).

use std::collections::BTreeSet;

use texparse::DocStructure;

/// A foldable block in 1-based source lines: `start` stays visible,
/// `start + 1..=end` collapses into the marker comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub start: usize,
    pub end: usize,
}

/// Foldable regions of the document: each heading runs to the line
/// before the next same-or-higher-level heading (or EOF); each matched
/// environment runs `\begin` line → `\end` line. Single-line blocks are
/// not foldable. Sorted by start; at most one region per start line
/// (the widest wins).
pub fn regions(doc: &DocStructure, line_count: usize) -> Vec<Region> {
    let mut out = Vec::new();
    for (i, h) in doc.headings.iter().enumerate() {
        let end =
            doc.headings.iter().skip(i + 1).find(|n| n.level <= h.level).map(|n| n.pos.line.saturating_sub(1)).unwrap_or(line_count).min(line_count);
        if end > h.pos.line {
            out.push(Region { start: h.pos.line, end });
        }
    }
    for e in &doc.envs {
        let (start, end) = (e.begin.line, e.end.line.min(line_count));
        if end > start {
            out.push(Region { start, end });
        }
    }
    out.sort_by_key(|r| (r.start, std::cmp::Reverse(r.end)));
    out.dedup_by_key(|r| r.start);
    out
}

/// The folded editor buffer: `text` is what the `TextEdit` shows and
/// `src_lines` maps each visible row (0-based) back to its 1-based
/// source line for gutter numbering and marks.
pub struct View {
    pub text: String,
    pub src_lines: Vec<usize>,
}

/// Build the folded buffer. `None` when `folded` is empty (the source is
/// shown as-is). Fold starts that don't match a current region are
/// ignored — stale entries degrade to an unfolded view, never corrupt.
pub fn build_view(src: &str, folded: &BTreeSet<usize>, regions: &[Region]) -> Option<View> {
    if folded.is_empty() {
        return None;
    }
    let mut text = String::with_capacity(src.len());
    let mut src_lines = Vec::new();
    let mut hide_until = 0usize; // hide source lines <= this (1-based)
    for (i, line) in src.split('\n').enumerate() {
        let ln = i + 1;
        if ln <= hide_until {
            continue;
        }
        src_lines.push(ln);
        let region = regions.iter().find(|r| r.start == ln && folded.contains(&ln));
        match region {
            Some(r) => {
                let hidden = r.end.saturating_sub(ln);
                text.push_str(line.strip_suffix('\r').unwrap_or(line));
                text.push_str(&format!("  % … {hidden} folded line{}", if hidden == 1 { "" } else { "s" }));
                hide_until = r.end;
            }
            None => text.push_str(line),
        }
        text.push('\n');
    }
    if !src.ends_with('\n') {
        text.pop();
    }
    Some(View { text, src_lines })
}

/// The visible row (0-based) showing `src_line` (1-based): itself when
/// visible, else its region's header row (a caret can't be left inside a
/// collapsed block), else 0.
pub fn visible_row(view: &View, regions: &[Region], src_line: usize) -> usize {
    if let Some(row) = view.src_lines.iter().position(|&l| l == src_line) {
        return row;
    }
    if let Some(r) = regions.iter().find(|r| r.start < src_line && src_line <= r.end) {
        if let Some(row) = view.src_lines.iter().position(|&l| l == r.start) {
            return row;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use texparse::parse_structure;

    const DOC: &str = "\\section{A}\ntext\n\\subsection{a1}\nx\n\\section{B}\ny\n\\begin{equation}\nz\n\\end{equation}\n";

    #[test]
    fn regions_from_headings_and_envs() {
        let d = parse_structure(DOC);
        let rs = regions(&d, 10);
        // \section{A} runs to line 4 (before \section{B} at 5); the
        // subsection inside ends there too; \section{B} runs to EOF;
        // equation env folds 7..=9.
        assert!(rs.contains(&Region { start: 1, end: 4 }));
        assert!(rs.contains(&Region { start: 3, end: 4 }));
        assert!(rs.contains(&Region { start: 5, end: 10 }));
        assert!(rs.contains(&Region { start: 7, end: 9 }));
    }

    #[test]
    fn build_view_collapses() {
        let d = parse_structure(DOC);
        let rs = regions(&d, 10);
        let mut folded = BTreeSet::new();
        folded.insert(1);
        let v = build_view(DOC, &folded, &rs).unwrap_or_else(|| View { text: String::new(), src_lines: Vec::new() });
        assert!(v.text.starts_with("\\section{A}  % … 3 folded lines\n"), "got: {}", v.text);
        assert!(v.text.contains("\\section{B}"));
        assert!(!v.text.contains("subsection"));
        // Source lines 2..=4 hidden: no visible row maps to them.
        assert_eq!(v.src_lines, vec![1, 5, 6, 7, 8, 9, 10]);
    }

    #[test]
    fn build_view_none_when_unfolded() {
        assert!(build_view(DOC, &BTreeSet::new(), &[]).is_none());
    }

    #[test]
    fn visible_row_inside_fold_maps_to_header() {
        let d = parse_structure(DOC);
        let rs = regions(&d, 10);
        let folded: BTreeSet<usize> = [1].into_iter().collect();
        let v = build_view(DOC, &folded, &rs).unwrap_or_else(|| View { text: String::new(), src_lines: Vec::new() });
        assert_eq!(visible_row(&v, &rs, 4), 0); // line 4 is hidden → header row
        assert_eq!(visible_row(&v, &rs, 7), 3); // visible line 7 → row 3
    }
}
