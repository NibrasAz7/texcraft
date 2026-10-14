//! Document-structure scan over the `.tex` token stream.
//!
//! Extracts what an editor needs — document class, packages, the heading
//! tree, environments, labels, citation/reference keys, included files —
//! plus consistency issues (unmatched braces, `\end` without `\begin`).
//! This is a scan, not an expansion: `\newcommand`-defined sectioning and
//! catcode changes are out of scope. Content after `\end{document}` is
//! ignored, and verbatim-like environment bodies are skipped (the lexer
//! still tokenizes them, so `%` inside a verbatim body can truncate the
//! scan of that one line — a documented limitation).

use crate::caps;
use crate::lex::{lex, Pos, Tok, Token};

/// A structural problem worth surfacing to the user.
#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
    pub message: String,
    pub pos: Pos,
}

/// A sectioning command occurrence. `level`: part=0, chapter=1,
/// section=2, subsection=3, subsubsection=4, paragraph=5, subparagraph=6.
#[derive(Debug, Clone, PartialEq)]
pub struct Heading {
    pub level: u8,
    pub title: String,
    pub pos: Pos,
}

/// A matched `\begin{name}` … `\end{name}` pair.
#[derive(Debug, Clone, PartialEq)]
pub struct Env {
    pub name: String,
    pub begin: Pos,
    pub end: Pos,
}

/// Everything the scan found in one `.tex` source.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DocStructure {
    /// `\documentclass[..]{name}` → (name, options).
    pub class: Option<(String, Pos)>,
    pub class_options: String,
    /// `\usepackage` / `\RequirePackage`, one entry per package name.
    pub packages: Vec<(String, Pos)>,
    pub title: Option<(String, Pos)>,
    pub author: Option<(String, Pos)>,
    pub date: Option<(String, Pos)>,
    pub headings: Vec<Heading>,
    /// Completed environments, in order of `\end`.
    pub envs: Vec<Env>,
    pub labels: Vec<(String, Pos)>,
    /// Citation keys (`\cite`, `\parencite`, `\nocite`, …), split on commas.
    pub cite_keys: Vec<(String, Pos)>,
    /// Cross-reference targets (`\ref`, `\eqref`, `\autoref`, …).
    pub ref_keys: Vec<(String, Pos)>,
    /// `\input` / `\include` file names.
    pub inputs: Vec<(String, Pos)>,
    /// `\includegraphics` file names.
    pub graphics: Vec<(String, Pos)>,
    /// `\bibliography` / `\addbibresource` file names.
    pub bibliographies: Vec<(String, Pos)>,
    /// `\bibliographystyle` names.
    pub bibstyles: Vec<(String, Pos)>,
    /// Names defined by `\newcommand`/`\renewcommand`/`\def` family.
    pub commands: Vec<(String, Pos)>,
    pub issues: Vec<Issue>,
}

const SECTIONING: &[(&str, u8)] =
    &[("part", 0), ("chapter", 1), ("section", 2), ("subsection", 3), ("subsubsection", 4), ("paragraph", 5), ("subparagraph", 6)];

const CITES: &[&str] = &["cite", "parencite", "textcite", "autocite", "footcite", "citep", "citet", "citeyear", "citeauthor", "nocite", "smartcite"];

const REFS: &[&str] = &["ref", "eqref", "pageref", "autoref", "cref", "Cref", "vref"];

const VERBATIM: &[&str] = &["verbatim", "verbatim*", "lstlisting", "minted", "Verbatim"];

/// Lex `src` and extract its structure. Always succeeds — malformed input
/// lands in [`DocStructure::issues`].
pub fn parse_structure(src: &str) -> DocStructure {
    let toks = lex(src);
    let mut d = DocStructure::default();
    let mut depth = 0usize;
    let mut depth_warned = false;
    let mut env_stack: Vec<(String, Pos)> = Vec::new();
    let mut verbatim: Option<String> = None;
    let mut i = 0usize;

    while let Some(tok) = toks.get(i) {
        i += 1;
        match &tok.kind {
            // Verbatim body: skip everything until the matching \end{name}.
            _ if verbatim.is_some() => {
                if let Tok::Cmd(c) = &tok.kind {
                    if c == "end" {
                        if let Some((name, ni)) = group_text(&toks, i, c, &mut d.issues) {
                            if Some(&name) == verbatim.as_ref() {
                                let begin = env_stack.pop().map(|(_, p)| p).unwrap_or(tok.pos);
                                d.envs.push(Env { name, begin, end: tok.pos });
                                verbatim = None;
                                i = ni;
                            }
                        }
                    }
                }
            }
            Tok::Comment(_) => {}
            Tok::LBrace => {
                depth += 1;
                if depth > caps::MAX_DEPTH && !depth_warned {
                    depth_warned = true;
                    d.issues.push(Issue { message: "brace nesting too deep".into(), pos: tok.pos });
                }
            }
            Tok::RBrace => {
                if depth == 0 {
                    d.issues.push(Issue { message: "unmatched }".into(), pos: tok.pos });
                } else {
                    depth -= 1;
                }
            }
            Tok::Cmd(name) => {
                let cmd_pos = tok.pos;
                match name.as_str() {
                    "begin" => {
                        if let Some((env_name, ni)) = group_text(&toks, i, name, &mut d.issues) {
                            i = ni;
                            if env_stack.len() >= caps::MAX_DEPTH && !depth_warned {
                                depth_warned = true;
                                d.issues.push(Issue { message: "environment nesting too deep".into(), pos: cmd_pos });
                            } else {
                                if VERBATIM.contains(&env_name.as_str()) {
                                    verbatim = Some(env_name.clone());
                                }
                                env_stack.push((env_name, cmd_pos));
                            }
                        }
                    }
                    "end" => {
                        if let Some((env_name, ni)) = group_text(&toks, i, name, &mut d.issues) {
                            i = ni;
                            match env_stack.pop() {
                                Some((open_name, open_pos)) if open_name == env_name => {
                                    d.envs.push(Env { name: env_name.clone(), begin: open_pos, end: cmd_pos });
                                    if env_name == "document" {
                                        for (n, p) in env_stack.drain(..) {
                                            d.issues.push(Issue { message: format!("unclosed \\begin{{{n}}}"), pos: p });
                                        }
                                        if depth > 0 {
                                            d.issues.push(Issue { message: format!("{depth} unclosed {{"), pos: tok.pos });
                                        }
                                        return d;
                                    }
                                }
                                Some((open_name, open_pos)) => {
                                    d.issues.push(Issue {
                                        message: format!("\\end{{{env_name}}} but \\begin{{{open_name}}} is still open"),
                                        pos: cmd_pos,
                                    });
                                    env_stack.push((open_name, open_pos));
                                }
                                None => {
                                    d.issues.push(Issue { message: format!("\\end{{{env_name}}} without \\begin"), pos: cmd_pos });
                                }
                            }
                        }
                    }
                    "documentclass" => {
                        let (opts, ni) = opt_arg(&toks, i);
                        i = ni;
                        d.class_options = opts;
                        if let Some((name, ni)) = group_text(&toks, i, name, &mut d.issues) {
                            i = ni;
                            d.class = Some((name, cmd_pos));
                        }
                    }
                    "usepackage" | "RequirePackage" => {
                        let (_opts, ni) = opt_arg(&toks, i);
                        i = ni;
                        if let Some((names, ni)) = group_text(&toks, i, name, &mut d.issues) {
                            i = ni;
                            for n in names.split(',') {
                                let n = n.trim();
                                if !n.is_empty() {
                                    d.packages.push((n.to_string(), cmd_pos));
                                }
                            }
                        }
                    }
                    "title" | "author" | "date" => {
                        if let Some((text, ni)) = group_text(&toks, i, name, &mut d.issues) {
                            i = ni;
                            let slot = match name.as_str() {
                                "title" => &mut d.title,
                                "author" => &mut d.author,
                                _ => &mut d.date,
                            };
                            *slot = Some((text, cmd_pos));
                        }
                    }
                    "label" => {
                        if let Some((key, ni)) = group_text(&toks, i, name, &mut d.issues) {
                            i = ni;
                            d.labels.push((key, cmd_pos));
                        }
                    }
                    "input" | "include" => {
                        if let Some((f, ni)) = group_text(&toks, i, name, &mut d.issues) {
                            i = ni;
                            d.inputs.push((f, cmd_pos));
                        }
                    }
                    "includegraphics" => {
                        let (_o, ni) = opt_arg(&toks, i);
                        i = ni;
                        if let Some((f, ni)) = group_text(&toks, i, name, &mut d.issues) {
                            i = ni;
                            d.graphics.push((f, cmd_pos));
                        }
                    }
                    "bibliography" | "addbibresource" => {
                        if let Some((files, ni)) = group_text(&toks, i, name, &mut d.issues) {
                            i = ni;
                            for f in files.split(',') {
                                let f = f.trim();
                                if !f.is_empty() {
                                    d.bibliographies.push((f.to_string(), cmd_pos));
                                }
                            }
                        }
                    }
                    "bibliographystyle" => {
                        if let Some((s, ni)) = group_text(&toks, i, name, &mut d.issues) {
                            i = ni;
                            d.bibstyles.push((s, cmd_pos));
                        }
                    }
                    "newcommand" | "renewcommand" | "providecommand" | "DeclareMathOperator" | "DeclareRobustCommand" => {
                        if let Some((n, ni)) = group_text(&toks, i, name, &mut d.issues) {
                            i = ni;
                            d.commands.push((n.trim_start_matches('\\').to_string(), cmd_pos));
                        }
                    }
                    "def" => {
                        // \def\foo — the name is the next command token.
                        let j = skip_ws(&toks, i);
                        if let Some(Token { kind: Tok::Cmd(n), .. }) = toks.get(j) {
                            d.commands.push((n.clone(), cmd_pos));
                            i = j + 1;
                        }
                    }
                    c => {
                        if let Some((_, lvl)) = SECTIONING.iter().find(|(n, _)| *n == c) {
                            let lvl = *lvl;
                            let (_short, ni) = opt_arg(&toks, i);
                            i = ni;
                            let title = group_text(&toks, i, name, &mut d.issues).map(|(t, ni)| {
                                i = ni;
                                t
                            });
                            d.headings.push(Heading { level: lvl, title: title.unwrap_or_default(), pos: cmd_pos });
                        } else if CITES.contains(&c) {
                            let (_o, ni) = opt_arg(&toks, i);
                            i = ni;
                            if let Some((keys, ni)) = group_text(&toks, i, name, &mut d.issues) {
                                i = ni;
                                for k in keys.split(',') {
                                    let k = k.trim();
                                    if !k.is_empty() {
                                        d.cite_keys.push((k.to_string(), cmd_pos));
                                    }
                                }
                            }
                        } else if REFS.contains(&c) {
                            if let Some((key, ni)) = group_text(&toks, i, name, &mut d.issues) {
                                i = ni;
                                d.ref_keys.push((key, cmd_pos));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    if depth > 0 {
        d.issues.push(Issue { message: format!("{depth} unclosed {{"), pos: end_pos(&toks) });
    }
    for (n, p) in env_stack {
        d.issues.push(Issue { message: format!("unclosed \\begin{{{n}}}"), pos: p });
    }
    d
}

fn end_pos(toks: &[Token]) -> Pos {
    toks.last().map(|t| t.pos).unwrap_or(Pos { offset: 0, line: 1, col: 1 })
}

/// Skip whitespace-only Text, Newline and Comment tokens.
fn skip_ws(toks: &[Token], mut i: usize) -> usize {
    while let Some(t) = toks.get(i) {
        match &t.kind {
            Tok::Text(s) if s.trim().is_empty() => i += 1,
            Tok::Newline | Tok::Comment(_) => i += 1,
            _ => break,
        }
    }
    i
}

/// `[opts]` — consume an optional bracket group, return its text and the
/// index after it (or the starting index when there is none).
fn opt_arg(toks: &[Token], i: usize) -> (String, usize) {
    let j = skip_ws(toks, i);
    if !matches!(toks.get(j).map(|t| &t.kind), Some(Tok::LBracket)) {
        return (String::new(), i);
    }
    let mut depth = 0usize;
    let mut text = String::new();
    let mut k = j;
    while let Some(t) = toks.get(k) {
        k += 1;
        match &t.kind {
            Tok::LBracket => {
                depth += 1;
                if depth > 1 {
                    text.push('[');
                }
            }
            Tok::RBracket => {
                if depth <= 1 {
                    return (text, k);
                }
                depth -= 1;
                text.push(']');
            }
            _ => push_surface(&mut text, t),
        }
        if text.len() >= caps::MAX_ARG {
            break;
        }
    }
    (text, k)
}

/// `{balanced}` — consume a mandatory brace group and return its surface
/// text plus the index after the closing brace. When the next non-space
/// token isn't `{`, a single token is the argument (TeX's single-token
/// argument rule, e.g. `\section x`). A group that never closes pushes an
/// issue (credited to `\cmd`) and returns what was captured.
fn group_text(toks: &[Token], i: usize, cmd: &str, issues: &mut Vec<Issue>) -> Option<(String, usize)> {
    let j = skip_ws(toks, i);
    let first = toks.get(j)?;
    if !matches!(first.kind, Tok::LBrace) {
        // Single-token argument: for a text run that means its first
        // character (TeX skips spaces, then takes one token).
        let text = match &first.kind {
            Tok::Text(s) => s.trim_start().chars().next().map(String::from).unwrap_or_default(),
            _ => {
                let mut t = String::new();
                push_surface(&mut t, first);
                t
            }
        };
        return Some((text, j + 1));
    }
    let mut depth = 0usize;
    let mut text = String::new();
    let mut k = j;
    while let Some(t) = toks.get(k) {
        k += 1;
        match &t.kind {
            Tok::LBrace => {
                depth += 1;
                if depth > 1 {
                    text.push('{');
                }
            }
            Tok::RBrace => {
                if depth <= 1 {
                    return Some((text, k));
                }
                depth -= 1;
                text.push('}');
            }
            _ => push_surface(&mut text, t),
        }
        if text.len() >= caps::MAX_ARG || depth > caps::MAX_DEPTH {
            break;
        }
    }
    // Unclosed group: return what we captured plus an issue — the main
    // loop's own depth counter never saw these tokens.
    let pos = toks.get(j).map(|t| t.pos).unwrap_or(Pos { offset: 0, line: 1, col: 1 });
    issues.push(Issue { message: format!("unclosed {{ argument to \\{cmd}"), pos });
    Some((text, k))
}

/// Render a token back to its surface text for argument capture.
fn push_surface(out: &mut String, t: &Token) {
    if out.len() >= caps::MAX_ARG {
        return;
    }
    match &t.kind {
        Tok::Text(s) => out.push_str(s),
        Tok::Cmd(n) => {
            out.push('\\');
            out.push_str(n);
        }
        Tok::CmdSym(c) => {
            out.push('\\');
            out.push(*c);
        }
        Tok::LBrace => out.push('{'),
        Tok::RBrace => out.push('}'),
        Tok::LBracket => out.push('['),
        Tok::RBracket => out.push(']'),
        Tok::Comment(_) => {}
        Tok::MathShift => out.push('$'),
        Tok::Newline => out.push(' '),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_document() {
        let src = r#"
\documentclass[12pt]{article}
\usepackage{amsmath, graphicx}
\usepackage{polyglossia}
\title{My Paper}
\author{A. Writer}
\begin{document}
\maketitle
\section{Intro}\label{sec:intro}
See \cite{knuth84,lamport94} and \ref{sec:intro}.
\begin{equation} E=mc^2 \label{eq:e}\end{equation}
\input{chapters/methods}
\bibliography{refs}
\end{document}
trailing \section{ignored}
"#;
        let d = parse_structure(src);
        assert_eq!(d.class.as_ref().map(|(c, _)| c.as_str()), Some("article"));
        assert_eq!(d.class_options, "12pt");
        assert_eq!(d.packages.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(), vec!["amsmath", "graphicx", "polyglossia"]);
        assert_eq!(d.title.as_ref().map(|(t, _)| t.as_str()), Some("My Paper"));
        assert_eq!(d.headings.len(), 1);
        assert_eq!((d.headings[0].level, d.headings[0].title.as_str()), (2, "Intro"));
        assert_eq!(d.labels.iter().map(|(l, _)| l.as_str()).collect::<Vec<_>>(), vec!["sec:intro", "eq:e"]);
        assert_eq!(d.cite_keys.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), vec!["knuth84", "lamport94"]);
        assert_eq!(d.ref_keys[0].0, "sec:intro");
        assert_eq!(d.inputs[0].0, "chapters/methods");
        assert_eq!(d.bibliographies[0].0, "refs");
        assert!(d.envs.iter().any(|e| e.name == "document"));
        assert!(d.envs.iter().any(|e| e.name == "equation"));
        assert!(d.issues.is_empty(), "unexpected issues: {:?}", d.issues);
    }

    #[test]
    fn env_mismatch_and_unclosed() {
        let d = parse_structure(r"\begin{a}\begin{b}\end{a}x{");
        assert!(d.issues.iter().any(|i| i.message.contains("\\end{a} but \\begin{b}")));
        assert!(d.issues.iter().any(|i| i.message.contains("unclosed \\begin{b}")));
        assert!(d.issues.iter().any(|i| i.message.contains("unclosed {")));
    }

    #[test]
    fn end_without_begin() {
        let d = parse_structure(r"\end{foo}");
        assert_eq!(d.issues.len(), 1);
        assert!(d.issues[0].message.contains("without \\begin"));
    }

    #[test]
    fn verbatim_body_skipped() {
        let d = parse_structure("\\begin{verbatim}\n\\section{fake} % } \n\\end{verbatim}\n\\section{Real}");
        assert_eq!(d.headings.len(), 1);
        assert_eq!(d.headings[0].title, "Real");
    }

    #[test]
    fn short_title_optional_arg() {
        let d = parse_structure(r"\section[Short]{Long Title}");
        assert_eq!(d.headings[0].title, "Long Title");
    }

    #[test]
    fn single_token_arg() {
        let d = parse_structure("\\section x more");
        assert_eq!(d.headings[0].title, "x");
    }

    #[test]
    fn newcommand_and_def() {
        let d = parse_structure(r"\newcommand{\R}{\mathbb{R}} \def\T{\top}");
        assert_eq!(d.commands.iter().map(|(c, _)| c.as_str()).collect::<Vec<_>>(), vec!["R", "T"]);
    }

    #[test]
    fn unclosed_argument_is_reported() {
        let d = parse_structure("\\section{Intro\nmore text");
        assert!(d.issues.iter().any(|i| i.message.contains("unclosed { argument to \\section")));
    }

    #[test]
    fn hostile_never_panics() {
        for src in ["", "{", "}", "\\begin", "\\end", "\\begin{", "\\section", "\\cite{", "{{{{", "\\begin{a", "]\\[", "\\documentclass"] {
            let _ = parse_structure(src);
        }
        let deep = "\\begin{a}".repeat(5_000);
        let _ = parse_structure(&deep);
        let braces = "{".repeat(5_000);
        let _ = parse_structure(&braces);
    }
}
