//! LaTeX syntax highlighting for the editor, driven by `texparse::lex`
//! tokens — the same tokens the outline and diagnostics read.

use std::sync::Arc;

use egui::text::{ByteIndex, LayoutJob, LayoutSection, TextFormat};
use texparse::{lex, Tok};

use crate::theme;

/// Build a galley for `src` with token-based coloring. `wrap` is the
/// wrap width the layouter contract hands us (unused — code does not
/// wrap).
pub fn layout(ui: &egui::Ui, src: &dyn egui::TextBuffer, _wrap: f32) -> Arc<egui::Galley> {
    let font_id = egui::TextStyle::Monospace.resolve(ui.style());
    let plain = TextFormat { font_id: font_id.clone(), color: theme::TEXT, ..Default::default() };
    let fmt = |color| TextFormat { color, ..plain.clone() };

    // The job owns the text; sections are contiguous byte ranges into it.
    // Code never wraps (`_wrap` ignored) so galley rows stay 1:1 with
    // source lines — the gutter relies on that.
    let mut job = LayoutJob { text: src.as_str().to_string(), sections: Vec::new(), ..LayoutJob::default() };
    job.wrap.max_width = f32::INFINITY;

    // Track $ … $ toggles so inline math reads differently from text.
    let mut in_math = false;
    let mut cursor = 0usize;
    for tok in lex(src.as_str()) {
        // Defensive: fill any gap the lexer left with plain format.
        if tok.pos.offset > cursor {
            job.sections.push(LayoutSection { leading_space: 0.0, byte_range: ByteIndex(cursor)..ByteIndex(tok.pos.offset), format: plain.clone() });
        }
        let format = match &tok.kind {
            Tok::Comment(_) => fmt(theme::SYN_COMMENT),
            Tok::Cmd(_) | Tok::CmdSym(_) => fmt(if in_math { theme::SYN_MATH } else { theme::SYN_CMD }),
            Tok::MathShift => {
                in_math = !in_math;
                fmt(theme::SYN_MATH)
            }
            Tok::LBrace | Tok::RBrace | Tok::LBracket | Tok::RBracket => fmt(theme::SYN_BRACE),
            Tok::Text(_) | Tok::Newline => fmt(if in_math { theme::SYN_MATH } else { theme::TEXT }),
        };
        job.sections.push(LayoutSection { leading_space: 0.0, byte_range: ByteIndex(tok.pos.offset)..ByteIndex(tok.end), format });
        cursor = tok.end;
    }
    if src.as_str().len() > cursor {
        job.sections.push(LayoutSection { leading_space: 0.0, byte_range: ByteIndex(cursor)..ByteIndex(src.as_str().len()), format: plain });
    }
    // Empty text still needs one section or the sanity check trips.
    if job.sections.is_empty() {
        job.sections.push(LayoutSection { leading_space: 0.0, byte_range: ByteIndex(0)..ByteIndex(0), format: TextFormat::default() });
    }
    ui.painter().layout_job(job)
}
