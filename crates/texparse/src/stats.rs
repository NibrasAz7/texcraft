//! Document statistics for the "Analyse Text" stats panel.
//!
//! [`analyze`] walks the [`lex`] token stream once — O(n) — and fills a
//! [`DocStats`] with word/character/line counts plus tallies of commands,
//! math spans, environments, sectioning commands, labels and citations.
//!
//! Conventions (LaTeX-editor semantics, not TeX's mouth):
//!
//! - **Words** are whitespace-separated runs inside [`Tok::Text`] tokens
//!   that occur outside math — the `wc -w` convention, so a run of pure
//!   punctuation still counts. A `\command`'s *name* never counts (it is
//!   a [`Tok::Cmd`], not text); its argument text does
//!   (`\section{Two Words}` → 2 words). `\begin{…}`/`\end{…}` names are
//!   consumed while looking for math environments, so they never count
//!   either.
//! - **Math spans** open on `$` (inline), `$$` or `\[` or a math-family
//!   environment (display) and `\(` (inline); they close on the matching
//!   delimiter — for `\begin{name}` the first `\end{name}` with the same
//!   name. Span bodies contribute no words; a span is counted once, when
//!   it opens.
//! - Comments are [`Tok::Comment`] tokens: invisible to `words` and to
//!   every command tally, though `chars`/`chars_no_space`/`lines` measure
//!   the raw source as written.

use crate::lex::{lex, Tok};
use crate::structure::{group_text, Issue};

/// Document statistics for the stats panel + `text.stats` command.
/// All counts derive from the lexer — comments never count, and a
/// `\command`'s NAME never counts as a word (its argument text does).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DocStats {
    /// Whitespace-separated runs inside Text tokens (non-comment,
    /// non-math). LaTeX convention: argument text counts, commands don't.
    pub words: usize,
    pub chars: usize,
    pub chars_no_space: usize,
    /// Physical source lines (`\n` + 1 for non-empty src).
    pub lines: usize,
    /// `\cmd` tokens.
    pub commands: usize,
    /// `$…$` / `\(…\)` inline math spans (count of spans, not chars).
    pub math_inline: usize,
    /// `$$…$$` / `\[…\]` / equation-family display environments.
    pub math_display: usize,
    /// `\begin{…}` count.
    pub envs: usize,
    /// Heading commands (section/subsection/…/part/chapter/paragraph).
    pub sections: usize,
    /// `\label{…}` count.
    pub labels: usize,
    /// `\cite`-family occurrences (cite, citep, citet, parencite, …).
    pub cites: usize,
}

/// Which math span is open, if any, and what closes it.
enum Math {
    /// `$…$` or `$$…$$` — closed by a MathShift; a second consecutive
    /// MathShift is consumed too, so `$$` is one closer, not two toggles.
    Dollar,
    /// `\(…\)` — closed by `\)`.
    Paren,
    /// `\[…\]` — closed by `\]`.
    Bracket,
    /// `\begin{name}` for a math-family environment — closed by the
    /// first `\end` whose group reads the same `name`.
    Env(String),
}

/// Compute stats for `.tex` source `src` — total cost O(n), caps: words
/// counted only from Text tokens; math spans tracked by the lexer's math
/// delimiters or a small state machine over tokens.
pub fn analyze(src: &str) -> DocStats {
    let mut d = DocStats {
        chars: src.chars().count(),
        chars_no_space: src.chars().filter(|c| !c.is_whitespace()).count(),
        lines: if src.is_empty() { 0 } else { src.matches('\n').count() + 1 },
        ..DocStats::default()
    };

    let toks = lex(src);
    let mut math: Option<Math> = None;
    // Sink for the issues `group_text` reports on unclosed arguments —
    // stats are best-effort counts, so they carry no diagnostics channel.
    let mut issues: Vec<Issue> = Vec::new();
    let mut i = 0usize;

    while let Some(tok) = toks.get(i) {
        i += 1;
        match &tok.kind {
            Tok::Text(s) => {
                if math.is_none() {
                    d.words += s.split_whitespace().count();
                }
            }
            Tok::MathShift => match math {
                None => {
                    // `$$` arrives as two consecutive MathShift tokens.
                    if matches!(toks.get(i).map(|t| &t.kind), Some(Tok::MathShift)) {
                        i += 1;
                        d.math_display += 1;
                    } else {
                        d.math_inline += 1;
                    }
                    math = Some(Math::Dollar);
                }
                Some(Math::Dollar) => {
                    if matches!(toks.get(i).map(|t| &t.kind), Some(Tok::MathShift)) {
                        i += 1;
                    }
                    math = None;
                }
                // A `$` inside \(…\), \[…\] or an env span is just content.
                _ => {}
            },
            Tok::CmdSym('(') => {
                if math.is_none() {
                    math = Some(Math::Paren);
                    d.math_inline += 1;
                }
            }
            Tok::CmdSym(')') => {
                if matches!(math, Some(Math::Paren)) {
                    math = None;
                }
            }
            Tok::CmdSym('[') => {
                if math.is_none() {
                    math = Some(Math::Bracket);
                    d.math_display += 1;
                }
            }
            Tok::CmdSym(']') => {
                if matches!(math, Some(Math::Bracket)) {
                    math = None;
                }
            }
            Tok::Cmd(name) => {
                d.commands += 1;
                match name.as_str() {
                    "begin" => {
                        d.envs += 1;
                        if let Some((env, ni)) = group_text(&toks, i, name, &mut issues) {
                            i = ni;
                            if math.is_none() && is_math_env(&env) {
                                math = Some(Math::Env(env));
                                d.math_display += 1;
                            }
                        }
                    }
                    "end" => {
                        if let Some((env, ni)) = group_text(&toks, i, name, &mut issues) {
                            i = ni;
                            if matches!(&math, Some(Math::Env(open)) if open == &env) {
                                math = None;
                            }
                        }
                    }
                    "label" => d.labels += 1,
                    c if is_section(c) => d.sections += 1,
                    // citep/citet/parencite/nocite/… — any name with "cite".
                    c if c.contains("cite") => d.cites += 1,
                    _ => {}
                }
            }
            _ => {}
        }
    }
    d
}

/// Sectioning commands counted as headings.
fn is_section(name: &str) -> bool {
    matches!(name, "part" | "chapter" | "section" | "subsection" | "subsubsection" | "paragraph" | "subparagraph")
}

/// Display-math environments: the `amsmath` family, starred or not.
fn is_math_env(name: &str) -> bool {
    let base = name.strip_suffix('*').unwrap_or(name);
    matches!(base, "equation" | "align" | "gather" | "multline" | "eqnarray")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_words_chars_lines() {
        let s = analyze("Hello world.\nSecond line.");
        assert_eq!(s.words, 4);
        assert_eq!(s.chars, 25);
        assert_eq!(s.chars_no_space, 22);
        assert_eq!(s.lines, 2);
    }

    #[test]
    fn comments_never_count() {
        let s = analyze("real words % \\section{fake} more words\nnext");
        assert_eq!(s.words, 3); // real, words, next
        assert_eq!(s.commands, 0);
        assert_eq!(s.sections, 0);
    }

    #[test]
    fn command_args_count_name_doesnt() {
        let s = analyze(r"\section{My Great Title}");
        assert_eq!(s.words, 3); // My Great Title — "section" does not count
        assert_eq!(s.commands, 1);
        assert_eq!(s.sections, 1);
    }

    #[test]
    fn inline_math_skipped_but_counted() {
        let s = analyze("a $x + y$ b \\( z \\) c");
        assert_eq!(s.words, 3); // a, b, c
        assert_eq!(s.math_inline, 2);
        assert_eq!(s.math_display, 0);
    }

    #[test]
    fn display_math_dollar_and_bracket() {
        let s = analyze("pre $$E=mc^2$$ mid \\[x^2\\] post");
        assert_eq!(s.words, 3); // pre, mid, post
        assert_eq!(s.math_display, 2);
        assert_eq!(s.math_inline, 0);
    }

    #[test]
    fn math_env_display_skips_body() {
        let s = analyze(r"before \begin{align} a &= b \label{eq:x} \end{align} after");
        assert_eq!(s.words, 2); // before, after
        assert_eq!(s.math_display, 1);
        assert_eq!(s.envs, 1);
        assert_eq!(s.labels, 1); // \label inside math still counts
        assert_eq!(s.commands, 3); // begin, label, end
    }

    #[test]
    fn labels_cites_envs_sections() {
        let s = analyze(r"\section{A}\label{s:a}\cite{k}\citep{j}\parencite{m}\nocite{n}\begin{equation}x\end{equation}");
        assert_eq!(s.sections, 1);
        assert_eq!(s.labels, 1);
        assert_eq!(s.cites, 4);
        assert_eq!(s.envs, 1);
        assert_eq!(s.math_display, 1);
        assert_eq!(s.commands, 8);
        // Argument text counts: A, s:a, k, j, m, n — "x" is inside math.
        assert_eq!(s.words, 6);
    }

    #[test]
    fn env_names_and_non_math_envs() {
        let s = analyze(r"\begin{document}body\end{document}");
        assert_eq!(s.words, 1); // just "body" — env names never count
        assert_eq!(s.envs, 1);
        assert_eq!(s.math_display, 0);

        let s = analyze(r"\begin{itemize}\item one\item two\end{itemize}");
        assert_eq!(s.words, 2); // one, two — a plain env is not math
        assert_eq!(s.commands, 4);
    }

    #[test]
    fn empty_and_whitespace_sources() {
        assert_eq!(analyze(""), DocStats::default());
        let w = analyze("  \n\n ");
        assert_eq!(w.words, 0);
        assert_eq!(w.lines, 3);
        assert_eq!(w.chars, 5);
        assert_eq!(w.chars_no_space, 0);
    }

    #[test]
    fn unclosed_and_hostile_math_never_panics() {
        let s = analyze("words $never closed");
        assert_eq!(s.math_inline, 1);
        assert_eq!(s.words, 1); // "never closed" is swallowed by math
        for src in ["$", "$$", "$x$$", "\\(", "\\[", "\\]", "\\begin{equation}", "\\end{align}", "$$$$$", "\\begin{"] {
            let _ = analyze(src);
        }
        let deep = "$".repeat(10_000);
        let _ = analyze(&deep);
    }
}
