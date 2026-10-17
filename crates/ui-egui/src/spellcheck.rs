//! Spell-checking for the editor: a Hunspell dictionary (`spellbook`,
//! MPL-2.0) loaded once from `dicts/` plus a rescan-on-change pass over
//! the buffer.
//!
//! Only prose is checked — the scan walks `texparse::lex` tokens so
//! command names, comments, math spans, `\verb` runs and the `{…}`
//! arguments of reference-ish commands (`\label`, `\cite`, `\input`, …)
//! are never flagged, matching TeXstudio-style expectations.
//!
//! Ranges are reported as **char indices**, not bytes — the editor maps
//! chars to galley positions. `texparse` token `pos.offset`/`end` are
//! byte offsets; the scan converts them through a forward-only
//! `char_indices` cursor.

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

use texparse::{lex, Tok};

/// Cap on recorded misspelling ranges so a hostile buffer can't
/// allocate unboundedly — 10k squiggles is already unreadable.
const MAX_RANGES: usize = 10_000;

/// Suggestions returned per misspelled word.
const MAX_SUGGESTIONS: usize = 8;

/// Commands whose `{…}` argument is a reference, file or package name —
/// not prose — and is skipped wholesale. The `cite` prefix additionally
/// matches the whole natbib/biblatex family (`\citep`, `\citet`, …).
const SKIP_ARG_CMDS: &[&str] = &[
    "label",
    "ref",
    "eqref",
    "autoref",
    "pageref",
    "input",
    "include",
    "includegraphics",
    "bibliography",
    "addbibresource",
    "documentclass",
    "usepackage",
    "RequirePackage",
    "url",
    "path",
    "verb",
];

/// Environments whose *body* is math, not prose — `\begin{equation}`
/// etc. enter math mode the way `$…$` does, so text inside (and any
/// nested env, tracked by begin/end balance) is never checked.
const MATH_ENVS: &[&str] = &[
    "math",
    "displaymath",
    "equation",
    "equation*",
    "align",
    "align*",
    "gather",
    "gather*",
    "multline",
    "multline*",
    "eqnarray",
    "eqnarray*",
    "flalign",
    "flalign*",
    "alignat",
    "alignat*",
];

/// Editor spell-check state: the loaded dictionary (if `dicts/` was
/// found at load time), the misspelled char-index ranges for the
/// current buffer, and a dirty stamp so rescans happen on change, not
/// every frame.
pub struct SpellCheck {
    /// `None` when no dictionary was found. spellbook 0.4's
    /// `Dictionary` owns its parsed data (no lifetime parameter), so
    /// the aff/dic strings don't need to outlive the load.
    dict: Option<spellbook::Dictionary>,
    /// Char-index ranges of misspelled words in the current source.
    ranges: Vec<(usize, usize)>,
    /// Source content generation these ranges belong to (caller passes
    /// a change stamp — e.g. a counter it bumps on buffer edits).
    stamp: u64,
    enabled: bool,
}

impl SpellCheck {
    /// Load `dicts/en_US.{aff,dic}` — searches `WARRAQCRAFT_DICTS` first,
    /// then `dicts/` next to the exe and `dicts/` in every ancestor of
    /// the exe dir (dev checkouts: workspace-root `dicts/`). Missing or
    /// unparseable files leave the checker unavailable — never an error.
    /// On wasm there is no filesystem: always unavailable.
    pub fn load() -> Self {
        Self { dict: load_dict(), ranges: Vec::new(), stamp: u64::MAX, enabled: true }
    }

    /// A dictionary was found and parsed.
    pub fn is_available(&self) -> bool {
        self.dict.is_some()
    }

    /// Whether checking is switched on (independent of availability).
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
    }

    /// Rescan when `stamp` changed. Returns `&[(start,end)]` char ranges.
    /// Cheap no-op when the stamp is unchanged or the checker is
    /// disabled/unavailable.
    pub fn scan(&mut self, src: &str, stamp: u64) -> &[(usize, usize)] {
        match &self.dict {
            Some(dict) if self.enabled => {
                if stamp == self.stamp {
                    return self.ranges.as_slice();
                }
                self.stamp = stamp;
                self.ranges = bad_word_ranges(src, &|w| word_ok(dict, w));
            }
            _ => {
                // Disabled or no dictionary: drop stale squiggles and
                // force a rescan if re-enabled at the same stamp.
                self.ranges.clear();
                self.stamp = u64::MAX;
            }
        }
        self.ranges.as_slice()
    }

    /// Suggestions for the word at char range `(start,end)` — capped at
    /// [`MAX_SUGGESTIONS`]. Empty when unavailable or the range is bad.
    pub fn suggest(&self, src: &str, start: usize, end: usize) -> Vec<String> {
        let mut out = Vec::new();
        let Some(dict) = &self.dict else { return out };
        if start >= end {
            return out;
        }
        let Some(bs) = src.char_indices().nth(start).map(|(b, _)| b) else { return out };
        // `end` past the last char clamps to the buffer end.
        let be = src.char_indices().nth(end).map(|(b, _)| b).unwrap_or(src.len());
        let Some(word) = src.get(bs..be) else { return out };
        dict.suggest(word, &mut out);
        out.truncate(MAX_SUGGESTIONS);
        out
    }
}

/// `dict.check` plus a `'s`/`'` possessive fallback: "author's" checks
/// when "author" does.
fn word_ok(dict: &spellbook::Dictionary, word: &str) -> bool {
    if dict.check(word) {
        return true;
    }
    for suffix in ["'s", "'S", "'"] {
        if let Some(stem) = word.strip_suffix(suffix) {
            if !stem.is_empty() && dict.check(stem) {
                return true;
            }
        }
    }
    false
}

/// Pure word scan: char-index ranges of the words `is_ok` rejects. The
/// dictionary-free core of [`SpellCheck::scan`], unit-tested directly.
///
/// A "word" is a maximal run of Unicode-alphabetic chars plus internal
/// `'` (letters on both sides). Skipped: runs under 2 chars, all-uppercase
/// acronyms, words glued to a digit (`x9`), everything inside math
/// (`$…$`, `$$…$$`, `\(…\)`, `\[…\]`), comments, command names, the
/// `{…}`/`[…]` arguments of [`SKIP_ARG_CMDS`], and `\verb|…|` runs.
pub(crate) fn bad_word_ranges(src: &str, is_ok: &dyn Fn(&str) -> bool) -> Vec<(usize, usize)> {
    let mut scan = Scan { is_ok, ranges: Vec::new(), byte_cur: 0, char_cur: 0 };
    let mut in_math = false;
    // A skip-listed command ran; its `{…}` argument is still expected.
    let mut skip_arg = false;
    // Inside a skipped `{…}`/`[…]` group: counts nesting, checks nothing.
    let mut depth = 0usize;
    // `\verb` delimiter, once seen. Verbatim survives across tokens:
    // `\verb|a$b|` lexes the `|`s and `a`/`b` runs as separate tokens.
    let mut verb_delim: Option<char> = None;
    // Saw `\verb` (and maybe its `*`), waiting for the delimiter char.
    let mut verb_pending = false;
    // `\begin{name}` / `\end{name}` name capture: `Some(is_begin)` while
    // waiting for `{`, then the name text accumulates until `}`.
    let mut env_cap: Option<bool> = None;
    let mut env_brace = false;
    let mut env_name = String::new();
    // Depth inside math environments — `\end` always pops the nearest
    // `\begin`, so the balance counts regardless of env names.
    let mut math_env = 0usize;

    let toks = lex(src);
    let mut i = 0;
    while let Some(tok) = toks.get(i) {
        i += 1;
        // \verb|…|: verbatim until the delimiter char repeats.
        if let Some(d) = verb_delim {
            match &tok.kind {
                Tok::CmdSym(c) if *c == d => verb_delim = None,
                Tok::Text(t) => {
                    if let Some((o, _)) = t.char_indices().find(|&(_, c)| c == d) {
                        verb_delim = None;
                        let rest = tok.pos.offset + o + d.len_utf8();
                        scan.scan_text(src, rest, tok.end);
                    }
                }
                _ => {}
            }
            continue;
        }
        // Inside a skipped group: only nesting matters.
        if depth > 0 {
            match tok.kind {
                Tok::LBrace | Tok::LBracket => depth += 1,
                Tok::RBrace | Tok::RBracket => depth -= 1,
                _ => {}
            }
            continue;
        }
        if verb_pending {
            match &tok.kind {
                // \verb* — the starred form; the delimiter follows.
                Tok::CmdSym('*') => continue,
                Tok::CmdSym(c) => {
                    verb_pending = false;
                    verb_delim = Some(*c);
                    continue;
                }
                // The delimiter is an ordinary char (`|` `!` `+` …) —
                // it lands inside the next Text token, and the closing
                // delimiter usually does too (`\verb|x|` → Text "|x|").
                Tok::Text(t) => {
                    verb_pending = false;
                    let mut chars = t.char_indices();
                    if let Some((_, d)) = chars.next() {
                        match chars.find(|&(_, c)| c == d) {
                            Some((o, _)) => {
                                // Closed inline: the rest is normal text.
                                scan.scan_text(src, tok.pos.offset + o + d.len_utf8(), tok.end);
                            }
                            // Unclosed: stay verbatim until it turns up.
                            None => verb_delim = Some(d),
                        }
                    }
                    continue;
                }
                // Nothing usable (newline, brace): \verb is broken —
                // disarm and let the token be processed normally.
                _ => verb_pending = false,
            }
        }
        match &tok.kind {
            // The lexer emits one MathShift per `$`: `$$` is a display-math
            // toggle, not two inlines — pair it via lookahead (same trick
            // as the stats pass).
            Tok::MathShift => {
                if matches!(toks.get(i).map(|t| &t.kind), Some(Tok::MathShift)) {
                    i += 1;
                }
                in_math = !in_math;
            }
            // \[ … \] and \( … \) delimit math too.
            Tok::CmdSym('(') | Tok::CmdSym('[') => in_math = true,
            Tok::CmdSym(')') | Tok::CmdSym(']') => in_math = false,
            Tok::Cmd(name) => {
                if name == "begin" || name == "end" {
                    env_cap = Some(name == "begin");
                    env_brace = false;
                    env_name.clear();
                }
                if name == "verb" {
                    verb_pending = true;
                }
                if name.starts_with("cite") || SKIP_ARG_CMDS.contains(&name.as_str()) {
                    skip_arg = true;
                }
            }
            // `{` opens the name group, text accumulates the name,
            // `}` commits it: inside a math env any \end pops, outside
            // only a \begin{<math env>} pushes.
            Tok::LBrace if env_cap.is_some() && !env_brace => env_brace = true,
            Tok::Text(t) if env_cap.is_some() && env_brace => env_name.push_str(t),
            Tok::RBrace if env_cap.is_some() && env_brace => {
                let is_begin = env_cap.take().unwrap_or(false);
                if math_env > 0 {
                    if is_begin {
                        math_env += 1;
                    } else {
                        math_env -= 1;
                    }
                } else if is_begin && MATH_ENVS.contains(&env_name.trim()) {
                    math_env = 1;
                }
                env_name.clear();
            }
            // `\begin` not followed by `{name}` (or a stray `{`) —
            // broken syntax, drop the pending capture.
            Tok::LBrace | Tok::RBrace if env_cap.is_some() => {
                env_cap = None;
                env_name.clear();
            }
            Tok::LBrace if skip_arg => {
                skip_arg = false;
                depth = 1;
            }
            // \cite[see]{key}: skip the optional […], stay armed for {…}.
            Tok::LBracket if skip_arg => depth = 1,
            Tok::Text(_) if in_math || math_env > 0 => {}
            Tok::Text(text) => {
                if text.trim().is_empty() {
                    // Whitespace: a `\cmd {…}` argument may still follow.
                    continue;
                }
                // `\cite foo` — not a group argument: disarm, check it.
                skip_arg = false;
                scan.scan_text(src, tok.pos.offset, tok.end);
            }
            // Comments, newlines, stray braces/brackets: nothing to check.
            _ => {}
        }
    }
    scan.ranges
}

/// Per-scan scratch: the checker callback, results so far, and the
/// forward-only byte→char cursor (tokens arrive in source order).
struct Scan<'a> {
    is_ok: &'a dyn Fn(&str) -> bool,
    ranges: Vec<(usize, usize)>,
    /// Byte offset `char_cur` was counted up to.
    byte_cur: usize,
    /// Char index at `byte_cur`.
    char_cur: usize,
}

impl Scan<'_> {
    /// Pull word runs out of one `Text` token (occupying `src[base..end]`)
    /// and record the ranges `is_ok` rejects.
    fn scan_text(&mut self, src: &str, base: usize, end: usize) {
        let Some(text) = src.get(base..end) else { return };
        let mut it = text.char_indices().peekable();
        let mut wbyte = 0usize; // word start, absolute byte offset
        let mut wchars = 0usize;
        let mut glued = false; // word touches a digit → identifier, not prose
        let mut prev_digit = false;
        while let Some((o, c)) = it.next() {
            if c.is_alphabetic() {
                if wchars == 0 {
                    wbyte = base + o;
                    glued = prev_digit;
                }
                wchars += 1;
            } else if c == '\'' && wchars > 0 && it.peek().is_some_and(|&(_, n)| n.is_alphabetic()) {
                // Internal apostrophe: "it's" stays one word; "'quoted'"
                // and "cats'" split — `'` is kept only between letters.
                wchars += 1;
            } else {
                if wchars > 0 {
                    self.emit_word(src, wbyte, base + o, wchars, glued || c.is_ascii_digit());
                }
                wchars = 0;
                glued = false;
            }
            prev_digit = c.is_ascii_digit();
        }
        if wchars > 0 {
            self.emit_word(src, wbyte, end, wchars, glued);
        }
    }

    /// Record one finished word (`src[wbyte..ebyte]`, `wchars` chars)
    /// when it is prose and `is_ok` rejects it.
    fn emit_word(&mut self, src: &str, wbyte: usize, ebyte: usize, wchars: usize, glued: bool) {
        if wchars < 2 || glued || self.ranges.len() >= MAX_RANGES {
            return;
        }
        let Some(w) = src.get(wbyte..ebyte) else { return };
        // All-uppercase → acronym (NASA), not prose. Mixed case is checked.
        if w.chars().all(|c| !c.is_lowercase()) {
            return;
        }
        if (self.is_ok)(w) {
            return;
        }
        let cstart = self.char_at(src, wbyte);
        self.byte_cur = ebyte;
        self.char_cur = cstart + wchars;
        self.ranges.push((cstart, self.char_cur));
    }

    /// Advance the byte→char cursor to `byte` (lexer offsets are always
    /// char boundaries) and return its char index.
    fn char_at(&mut self, src: &str, byte: usize) -> usize {
        if byte > self.byte_cur {
            if let Some(gap) = src.get(self.byte_cur..byte) {
                self.char_cur += gap.chars().count();
            }
            self.byte_cur = byte;
        }
        self.char_cur
    }
}

/// Native: first `en_US.aff`+`en_US.dic` pair that parses wins.
/// Candidates: `$WARRAQCRAFT_DICTS`, then `dicts/` next to the exe and in
/// every ancestor of the exe dir (dev checkouts: workspace-root dicts/).
#[cfg(not(target_arch = "wasm32"))]
fn load_dict() -> Option<spellbook::Dictionary> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(env) = std::env::var_os("WARRAQCRAFT_DICTS") {
        dirs.push(PathBuf::from(env));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            dirs.extend(dir.ancestors().map(|a| a.join("dicts")));
        }
    }
    for dir in dirs {
        if let (Ok(aff), Ok(dic)) = (std::fs::read_to_string(dir.join("en_US.aff")), std::fs::read_to_string(dir.join("en_US.dic"))) {
            if let Ok(d) = spellbook::Dictionary::new(&aff, &dic) {
                return Some(d);
            }
        }
    }
    None
}

/// Wasm: no filesystem — the checker stays unavailable.
#[cfg(target_arch = "wasm32")]
fn load_dict() -> Option<spellbook::Dictionary> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ok` lists the correctly spelled words; anything else is flagged.
    fn ranges(src: &str, ok: &[&str]) -> Vec<(usize, usize)> {
        bad_word_ranges(src, &|w| ok.contains(&w))
    }

    #[test]
    fn flags_plain_prose() {
        assert_eq!(ranges("hello wrld", &["hello"]), vec![(6, 10)]);
    }

    #[test]
    fn comments_are_skipped() {
        assert_eq!(ranges("fine % wrd wrdd\nnext", &["fine", "next"]), Vec::<(usize, usize)>::new());
    }

    #[test]
    fn command_names_are_not_words() {
        assert_eq!(ranges(r"\wrd foo", &["foo"]), Vec::<(usize, usize)>::new());
    }

    #[test]
    fn ref_like_args_are_skipped() {
        let src = r"\label{wrd} \cite{wrd} \citep[see]{wrd} \includegraphics{wrd} ok";
        assert_eq!(ranges(src, &["see", "ok"]), Vec::<(usize, usize)>::new());
    }

    #[test]
    fn math_is_skipped() {
        // Inline $…$, display $$…$$, \[…\] and \(…\) all hide contents.
        let src = "$wrd$ ok $$wrd$$ \\[wrd\\] \\(wrd\\) ok";
        assert_eq!(ranges(src, &["ok"]), Vec::<(usize, usize)>::new());
    }

    #[test]
    fn math_envs_are_skipped() {
        // \begin{equation}…\end{equation} is math like $…$; nested
        // envs (split) keep the balance.
        let src = "\\begin{equation} wrd \\begin{split} wrdd \\end{split} \\end{equation} ok \\begin{figure} wrddd \\end{figure}";
        assert_eq!(ranges(src, &["ok", "figure"]), vec![(85, 90)]);
    }

    #[test]
    fn internal_apostrophe_keeps_word() {
        // "it's" reaches the checker as one word, not "it" + "s".
        assert_eq!(ranges("it's wrd", &["it's"]), vec![(5, 8)]);
        // Leading/trailing quotes split: 'wrd' is still checked.
        assert_eq!(ranges("'wrd'", &[]), vec![(1, 4)]);
    }

    #[test]
    fn escaped_percent_is_not_a_word() {
        assert_eq!(ranges(r"50\% wrd", &[]), vec![(5, 8)]);
    }

    #[test]
    fn acronyms_and_digit_words_are_skipped() {
        // NASA: all caps → acronym. x9: glued to a digit → identifier.
        assert_eq!(ranges("NASA and x9 wrd", &["and"]), vec![(12, 15)]);
    }

    #[test]
    fn ranges_are_char_indices_not_bytes() {
        // é is 2 bytes: "wrd" sits at chars 6..9 (bytes 7..10).
        assert_eq!(ranges("héllo wrd", &["héllo"]), vec![(6, 9)]);
    }

    #[test]
    fn verb_runs_are_skipped() {
        assert_eq!(ranges(r"\verb|wrd wrdd| ok", &["ok"]), Vec::<(usize, usize)>::new());
    }

    #[test]
    fn disabled_or_missing_dict_returns_empty() {
        let mut sc = SpellCheck::load();
        sc.set_enabled(false);
        assert!(sc.scan("wrd", 1).is_empty());
        assert!(sc.suggest("wrd", 0, 3).is_empty() || sc.is_available());
    }

    #[test]
    fn dict_loads_scans_and_suggests_when_files_present() {
        // Needs real dicts/ files (scripts/fetch-dict.ps1); silently
        // passes where they are absent.
        let mut sc = SpellCheck::load();
        if !sc.is_available() {
            return;
        }
        assert_eq!(sc.scan("hello wrd", 1), &[(6, 9)]);
        assert!(!sc.suggest("hello wrd", 6, 9).is_empty());
        // Unchanged stamp returns the cached ranges, not a rescan.
        assert_eq!(sc.scan("fine", 1), &[(6, 9)]);
        assert_eq!(sc.scan("fine", 2), &[]);
    }
}
