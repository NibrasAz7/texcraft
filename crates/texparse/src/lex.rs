//! `.tex` tokenizer — the default catcode regime, positioned tokens.
//!
//! This is the surface level real editors use: `\command`, `{` `}`, `[` `]`,
//! `% comments`, `$` math shifts, everything else as text runs. Catcode
//! changes (`\catcode`, verbatim bodies, `\verb`) are not modelled — the
//! structure scanner special-cases verbatim-like environments where it can.

use crate::caps;

/// Byte position plus 1-based line/column for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pos {
    pub offset: usize,
    pub line: usize,
    pub col: usize,
}

/// Token kinds emitted by [`lex`].
#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    /// A run of ordinary characters (letters, digits, punctuation).
    Text(String),
    /// Control word: `\foo` — alphabetic name, no leading backslash stored.
    Cmd(String),
    /// Control symbol: `\%`, `\{`, `\,` — a single non-letter after `\`.
    CmdSym(char),
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    /// `%` through end of line — the comment text without the `%`.
    Comment(String),
    /// A single `$` (the scanner decides `$$` display math itself).
    MathShift,
    Newline,
}

/// A token with its source span (`pos` = start, `end` = byte after last).
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: Tok,
    pub pos: Pos,
    pub end: usize,
}

/// Tokenize `src`. Never fails: every byte lands in some token.
pub fn lex(src: &str) -> Vec<Token> {
    let mut toks = Vec::new();
    let mut it = src.char_indices().peekable();
    let mut line = 1usize;
    let mut col = 1usize;

    // Advance past one char, updating line/col. Returns its byte offset.
    fn bump(it: &mut std::iter::Peekable<std::str::CharIndices<'_>>, line: &mut usize, col: &mut usize) -> Option<(usize, char)> {
        let (off, c) = it.next()?;
        if c == '\n' {
            *line += 1;
            *col = 1;
        } else {
            *col += 1;
        }
        Some((off, c))
    }

    while let Some(&(off, c)) = it.peek() {
        let pos = Pos { offset: off, line, col };
        match c {
            '%' => {
                bump(&mut it, &mut line, &mut col);
                let mut text = String::new();
                while let Some(&(_, d)) = it.peek() {
                    if d == '\n' {
                        break;
                    }
                    if text.len() < caps::MAX_ARG {
                        text.push(d);
                    }
                    bump(&mut it, &mut line, &mut col);
                }
                let end = it.peek().map(|&(o, _)| o).unwrap_or(src.len());
                toks.push(Token { kind: Tok::Comment(text), pos, end });
            }
            '\\' => {
                bump(&mut it, &mut line, &mut col);
                match it.peek() {
                    Some(&(_, d)) if d.is_ascii_alphabetic() => {
                        let mut name = String::new();
                        while let Some(&(_, d)) = it.peek() {
                            if !d.is_ascii_alphabetic() {
                                break;
                            }
                            if name.len() < caps::MAX_NAME {
                                name.push(d);
                            }
                            bump(&mut it, &mut line, &mut col);
                        }
                        let end = it.peek().map(|&(o, _)| o).unwrap_or(src.len());
                        toks.push(Token { kind: Tok::Cmd(name), pos, end });
                    }
                    Some(&(_, d)) => {
                        bump(&mut it, &mut line, &mut col);
                        let end = it.peek().map(|&(o, _)| o).unwrap_or(src.len());
                        toks.push(Token { kind: Tok::CmdSym(d), pos, end });
                    }
                    None => {
                        toks.push(Token { kind: Tok::Text("\\".into()), pos, end: src.len() });
                    }
                }
            }
            '{' | '}' | '[' | ']' => {
                bump(&mut it, &mut line, &mut col);
                let kind = match c {
                    '{' => Tok::LBrace,
                    '}' => Tok::RBrace,
                    '[' => Tok::LBracket,
                    _ => Tok::RBracket,
                };
                toks.push(Token { kind, pos, end: off + c.len_utf8() });
            }
            '$' => {
                bump(&mut it, &mut line, &mut col);
                toks.push(Token { kind: Tok::MathShift, pos, end: off + 1 });
            }
            '\n' => {
                bump(&mut it, &mut line, &mut col);
                toks.push(Token { kind: Tok::Newline, pos, end: off + 1 });
            }
            _ => {
                // Ordinary text run: accumulate until a special char.
                let mut text = String::new();
                let mut end = off;
                while let Some(&(o, d)) = it.peek() {
                    if matches!(d, '%' | '\\' | '{' | '}' | '[' | ']' | '$' | '\n') {
                        break;
                    }
                    if text.len() < caps::MAX_ARG {
                        text.push(d);
                    }
                    end = o + d.len_utf8();
                    bump(&mut it, &mut line, &mut col);
                }
                toks.push(Token { kind: Tok::Text(text), pos, end });
            }
        }
    }
    toks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<Tok> {
        lex(src).into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn commands_and_groups() {
        let k = kinds(r"\section{Intro} text \% done");
        assert_eq!(
            k,
            vec![
                Tok::Cmd("section".into()),
                Tok::LBrace,
                Tok::Text("Intro".into()),
                Tok::RBrace,
                Tok::Text(" text ".into()),
                Tok::CmdSym('%'),
                Tok::Text(" done".into()),
            ]
        );
    }

    #[test]
    fn comments_eat_to_eol() {
        let k = kinds("a% \\section{no}\nb");
        assert_eq!(k, vec![Tok::Text("a".into()), Tok::Comment(" \\section{no}".into()), Tok::Newline, Tok::Text("b".into())]);
    }

    #[test]
    fn positions_are_one_based() {
        let t = lex("ab\n\\cite{x}");
        let cite = t.iter().find(|t| t.kind == Tok::Cmd("cite".into())).unwrap();
        assert_eq!((cite.pos.line, cite.pos.col, cite.pos.offset), (2, 1, 3));
    }

    #[test]
    fn math_shift_and_brackets() {
        let k = kinds("$x$\\[y\\]");
        assert_eq!(k, vec![Tok::MathShift, Tok::Text("x".into()), Tok::MathShift, Tok::CmdSym('['), Tok::Text("y".into()), Tok::CmdSym(']'),]);
    }

    #[test]
    fn hostile_input_never_panics() {
        for src in ["", "\\", "%", "{", "}", "\u{0}\u{1}\u{2}", "\\\\\\", "$$$$$", "[[[["] {
            let toks = lex(src);
            assert!(!toks.is_empty() || src.is_empty());
        }
        // Deeply unbalanced input still terminates.
        let deep = "{".repeat(10_000);
        let _ = lex(&deep);
        let long_cmd = format!("\\{}", "a".repeat(10_000));
        let toks = lex(&long_cmd);
        match &toks[0].kind {
            Tok::Cmd(n) => assert_eq!(n.len(), caps::MAX_NAME),
            other => panic!("expected Cmd, got {other:?}"),
        }
    }
}
