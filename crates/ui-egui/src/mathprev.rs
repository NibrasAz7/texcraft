//! Inline math preview — TeXstudio "preview formula" parity. The math
//! region enclosing the caret is found by [`math_at`]/[`math_span`]
//! (pure, platform-free), compiled to a one-page PDF by the engine's
//! `math.preview` command on a worker thread, rasterized with hayro and
//! cached as an egui texture keyed by the exact fragment text.
//!
//! [`MathPreview`] is native-only — the web build has no filesystem or
//! TeX engine, so the wasm version is a same-shaped no-op that keeps
//! call sites unconditional.

/// How far (in chars) the delimiter search reaches to each side of the
/// caret — an opener further away doesn't enclose it.
const SEARCH_CHARS: usize = 4096;

/// Math environments recognised as math regions: the LaTeX display
/// family, starred or not.
const MATH_ENVS: &[&str] = &["equation", "equation*", "align", "align*", "gather", "gather*", "multline", "multline*", "eqnarray", "eqnarray*"];

/// In-flight preview compiles — new requests are dropped past this.
#[cfg(not(target_arch = "wasm32"))]
const MAX_PENDING: usize = 16;
/// Cached textures — evicted FIFO past this.
#[cfg(not(target_arch = "wasm32"))]
const MAX_CACHED: usize = 64;
/// Remembered per-fragment errors — evicted FIFO past this.
#[cfg(not(target_arch = "wasm32"))]
const MAX_ERRORS: usize = 64;
/// Rasterize scale for fragments (≈2× the PDF's point size).
#[cfg(not(target_arch = "wasm32"))]
const PREVIEW_SCALE: f32 = 2.0;
/// Safety cap: neither pixmap side may exceed this (a hostile PDF can
/// declare an absurd page box).
#[cfg(not(target_arch = "wasm32"))]
const MAX_PREVIEW_PX: f32 = 2048.0;

/// Delimiter families that pair with each other.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// `$…$` inline math (toggle token).
    Dollar,
    /// `$$…$$` display math (toggle token).
    Display,
    /// `\(…\)`.
    Paren,
    /// `\[…\]`.
    Bracket,
    /// `\begin{env}…\end{env}`.
    Env,
}

/// A scanned delimiter token inside the search window.
struct Tok {
    kind: Kind,
    /// Byte offset of the token's first char (the `\` or `$`).
    at: usize,
    /// Byte offset one past the token's last char.
    end: usize,
    /// `true` for openers and `$`/`$$` toggles, `false` for the pure
    /// closers `\)`, `\]` and `\end{…}`.
    open: bool,
    /// Env name — `Env` tokens only.
    name: String,
}

/// An unmatched opener on the parse stack.
struct Frame {
    kind: Kind,
    /// Token start byte; also the span's left edge.
    at: usize,
    /// First byte of the inner body (one past the opener).
    inner_at: usize,
    /// Env name for `Kind::Env` (matched against `\end{…}`).
    name: String,
}

/// `{name}` after a `\begin`/`\end` word, whitespace before the brace
/// allowed. `chars` is the materialized window and `i` sits just past
/// the word; on success returns `(name_lo, name_hi, token_end)` byte
/// offsets and advances `i` past the `}`.
fn env_name(chars: &[(usize, char)], i: &mut usize) -> Option<(usize, usize, usize)> {
    let mut j = *i;
    while j < chars.len() && matches!(chars[j].1, ' ' | '\t' | '\r' | '\n') {
        j += 1;
    }
    if chars.get(j).map(|&(_, c)| c) != Some('{') {
        return None;
    }
    j += 1;
    let name_lo = chars.get(j).map(|&(o, _)| o)?;
    while j < chars.len() && (chars[j].1.is_ascii_alphabetic() || chars[j].1 == '*') {
        j += 1;
    }
    let (end_o, close) = *chars.get(j)?;
    if close != '}' || end_o == name_lo {
        return None;
    }
    *i = j + 1;
    Some((name_lo, end_o, end_o + 1))
}

/// Delimiter tokens in `src[lo..hi]`, in order. Backslash runs are
/// resolved first — `\$` (odd run) is consumed as an escaped char, and
/// `\\x` (even run) can't start a `\x` delimiter. Bounded by `hi - lo`.
fn scan(src: &str, lo: usize, hi: usize) -> Vec<Tok> {
    let chars: Vec<(usize, char)> = src.char_indices().skip_while(|&(o, _)| o < lo).take_while(|&(o, _)| o < hi).collect();
    let mut toks = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        let (o, c) = chars[i];
        match c {
            '\\' => {
                let mut run = 1usize;
                while i + run < chars.len() && chars[i + run].1 == '\\' {
                    run += 1;
                }
                i += run;
                if run.is_multiple_of(2) {
                    continue; // `\\` pairs — every backslash is escaped
                }
                // Odd run: the last `\` (a 1-byte char) opens a control
                // sequence starting at `bs`.
                let bs = o + run - 1;
                match chars.get(i) {
                    Some(&(_, d)) if d.is_ascii_alphabetic() => {
                        let ws = chars[i].0;
                        let mut we = ws;
                        while i < chars.len() && chars[i].1.is_ascii_alphabetic() {
                            we = chars[i].0 + chars[i].1.len_utf8();
                            i += 1;
                        }
                        let word = src.get(ws..we).unwrap_or("");
                        if word == "begin" || word == "end" {
                            if let Some((nlo, nhi, tok_end)) = env_name(&chars, &mut i) {
                                if let Some(name) = src.get(nlo..nhi) {
                                    if MATH_ENVS.contains(&name) {
                                        toks.push(Tok { kind: Kind::Env, at: bs, end: tok_end, open: word == "begin", name: name.to_string() });
                                    }
                                }
                            }
                        }
                    }
                    Some(&(_, d)) => {
                        // Control symbol `\c` — consume it; a few are
                        // math delimiters, the rest (`\$`, `\%`, …) text.
                        let kind = match d {
                            '(' => Some((Kind::Paren, true)),
                            ')' => Some((Kind::Paren, false)),
                            '[' => Some((Kind::Bracket, true)),
                            ']' => Some((Kind::Bracket, false)),
                            _ => None,
                        };
                        if let Some((kind, open)) = kind {
                            toks.push(Tok { kind, at: bs, end: o + run + 1, open, name: String::new() });
                        }
                        i += 1;
                    }
                    None => {}
                }
            }
            '$' => {
                let double = chars.get(i + 1).is_some_and(|&(_, c)| c == '$');
                let (kind, len) = if double { (Kind::Display, 2) } else { (Kind::Dollar, 1) };
                toks.push(Tok { kind, at: o, end: o + len, open: true, name: String::new() });
                i += len;
            }
            _ => i += 1,
        }
    }
    toks
}

/// All matched math spans in `src[lo..hi]` as
/// `(span_lo, span_hi, inner_lo, inner_hi)` byte tuples — span edges
/// include the delimiters, `inner_*` is the stripped body.
fn math_spans(src: &str, lo: usize, hi: usize) -> Vec<(usize, usize, usize, usize)> {
    let mut stack: Vec<Frame> = Vec::new();
    let mut spans = Vec::new();
    for t in scan(src, lo, hi) {
        let hit = match (t.kind, t.open) {
            // Pure openers never close anything.
            (Kind::Paren | Kind::Bracket | Kind::Env, true) => None,
            // `\end{n}` pairs with the nearest unmatched `\begin{n}` —
            // a different env name does not match.
            (Kind::Env, false) => stack.iter().rposition(|f| f.kind == Kind::Env && f.name == t.name),
            // `$`/`$$` toggles and the `\)`/`]` closers pair with the
            // nearest same-kind opener, innermost first.
            _ => stack.iter().rposition(|f| f.kind == t.kind),
        };
        match hit {
            // Frames above the match dangled unclosed — the drain pops
            // them (they never produced a span).
            Some(k) => {
                if let Some(f) = stack.drain(k..).next() {
                    spans.push((f.at, t.end, f.inner_at, t.at));
                }
            }
            // Unmatched closers are ignored; openers and unmatched
            // toggles wait for their pair.
            None if t.open => stack.push(Frame { kind: t.kind, at: t.at, inner_at: t.end, name: t.name }),
            None => {}
        }
    }
    spans
}

/// The innermost matched math span enclosing char-index `ci`:
/// `(span_lo, span_hi, inner_lo, inner_hi)` byte offsets.
fn enclosing(src: &str, ci: usize) -> Option<(usize, usize, usize, usize)> {
    // `bi` = byte offset of char `ci` (end of string past the last char).
    let bi = src.char_indices().nth(ci).map(|(b, _)| b).unwrap_or(src.len());
    let lo = src.char_indices().nth(ci.saturating_sub(SEARCH_CHARS)).map(|(b, _)| b).unwrap_or(0);
    let hi = src.get(bi..).and_then(|s| s.char_indices().nth(SEARCH_CHARS)).map(|(o, _)| bi + o).unwrap_or(src.len());
    math_spans(src, lo, hi).into_iter().filter(|&(l, h, _, _)| l <= bi && bi < h).min_by_key(|&(l, h, _, _)| h - l)
}

/// The math expression enclosing char-index `ci` in `src`, with its
/// delimiters stripped: inside `$…$`/`\(…\)`/`\[…\]`/`$$…$$` or a math
/// env `\begin{equation|align|gather|multline|eqnarray}[*]…\end{…}` →
/// `Some(inner)`. Else `None`. Char-boundary safe, O(window) bounded —
/// delimiters are searched at most [`SEARCH_CHARS`] chars each way.
///
/// The caret counts as inside from the first delimiter byte up to (not
/// including) the byte past the last one. `\$` after an odd backslash
/// run is escaped, not a delimiter; unmatched pairs are ignored; nested
/// regions resolve innermost-first.
pub fn math_at(src: &str, ci: usize) -> Option<String> {
    let (_, _, il, ih) = enclosing(src, ci)?;
    src.get(il..ih).map(str::to_string)
}

/// The `$…$`-style span (byte range incl. delimiters) around `ci` —
/// for highlighting the previewed region.
// Wired to the region-highlight call site in a later wave; keep the
// gate quiet whichever way the module is declared.
#[allow(dead_code)]
pub fn math_span(src: &str, ci: usize) -> Option<(usize, usize)> {
    enclosing(src, ci).map(|(l, h, _, _)| (l, h))
}

/// The engine's `math.preview` always writes the same scratch file
/// (`<doc-dir>/.texcraft/preview/m.tex`), so fragment compiles are
/// serialized here — concurrent workers would clobber each other's PDF.
#[cfg(not(target_arch = "wasm32"))]
static PREVIEW_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Worker half of [`MathPreview::request`]: compile `latex` through a
/// scratch session's `math.preview` command, then rasterize the first
/// page of the returned PDF.
#[cfg(not(target_arch = "wasm32"))]
fn compile_preview(latex: &str) -> Result<egui::ColorImage, String> {
    let _guard = PREVIEW_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut session = texcraft_engine::Session::new();
    let out = session.execute("math.preview", &serde_json::json!({"latex": latex})).map_err(|e| e.to_string())?;
    match out.get("pdf").and_then(serde_json::Value::as_str) {
        Some(pdf) => render_first_page(std::path::Path::new(pdf), PREVIEW_SCALE),
        // No PDF: surface the first reported error, else the last log
        // line, else a generic note.
        None => Err(out
            .get("errors")
            .and_then(serde_json::Value::as_array)
            .and_then(|errs| {
                errs.iter().find_map(|e| e.as_str().or_else(|| e.get("message").and_then(serde_json::Value::as_str)).map(str::to_string))
            })
            .or_else(|| out.get("log").and_then(serde_json::Value::as_str).and_then(|l| l.lines().last().map(str::to_string)))
            .unwrap_or_else(|| "math preview produced no PDF".to_string())),
    }
}

/// Rasterize page 0 of `pdf_path` at `scale` (clamped so neither side
/// exceeds [`MAX_PREVIEW_PX`]) into an `egui::ColorImage`. Same hayro
/// pipeline as the document preview in `lib.rs`.
#[cfg(not(target_arch = "wasm32"))]
fn render_first_page(pdf_path: &std::path::Path, scale: f32) -> Result<egui::ColorImage, String> {
    let data = std::fs::read(pdf_path).map_err(|e| format!("cannot read {}: {e}", pdf_path.display()))?;
    let pdf = hayro::hayro_syntax::Pdf::new(data).map_err(|e| format!("cannot parse PDF: {e:?}"))?;
    let Some(page) = pdf.pages().first() else {
        return Err(format!("{} has no pages", pdf_path.display()));
    };
    let (w, h) = page.render_dimensions();
    let scale = scale.min(MAX_PREVIEW_PX / w.max(h).max(1.0));
    let cache = hayro::RenderCache::new();
    let interp = hayro::hayro_interpret::InterpreterSettings::default();
    let settings = hayro::RenderSettings::default();
    let pixmap_settings = hayro::PixmapSettings { x_scale: scale, y_scale: scale, bg_color: hayro::vello_cpu::color::palette::css::WHITE };
    let pixmap = hayro::render(page, &cache, &interp, &settings, &pixmap_settings);
    Ok(egui::ColorImage::from_rgba_premultiplied([usize::from(pixmap.width()), usize::from(pixmap.height())], pixmap.data_as_u8_slice()))
}

/// Cache for math-fragment previews: key = the exact latex text.
/// Worker threads compile+rasterize; [`poll`](Self::poll) uploads
/// finished textures.
#[cfg(not(target_arch = "wasm32"))]
pub struct MathPreview {
    cache: std::collections::HashMap<String, egui::TextureHandle>,
    /// FIFO eviction order for `cache` (oldest first).
    cache_order: std::collections::VecDeque<String>,
    pending: std::collections::HashSet<String>,
    rx: std::sync::mpsc::Receiver<(String, Result<egui::ColorImage, String>)>,
    tx: std::sync::mpsc::Sender<(String, Result<egui::ColorImage, String>)>,
    /// Textures that need ctx.load_texture — produced by workers.
    done: Vec<(String, egui::ColorImage)>,
    /// Last error per key (bounded, for a status hint).
    errors: std::collections::HashMap<String, String>,
    /// FIFO eviction order for `errors` (oldest first).
    errors_order: std::collections::VecDeque<String>,
}

#[cfg(not(target_arch = "wasm32"))]
impl MathPreview {
    pub fn new() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Self {
            cache: std::collections::HashMap::new(),
            cache_order: std::collections::VecDeque::new(),
            pending: std::collections::HashSet::new(),
            rx,
            tx,
            done: Vec::new(),
            errors: std::collections::HashMap::new(),
            errors_order: std::collections::VecDeque::new(),
        }
    }

    /// Queue a preview compile for `latex` if not cached/pending.
    /// Worker: scratch `texcraft_engine::Session`, `math.preview`,
    /// rasterize page 0 of the returned pdf path.
    ///
    /// Skipped while ≥ [`MAX_PENDING`] compiles are in flight, for a
    /// blank fragment, or while a previous failure for the exact same
    /// text is remembered (the error evicts eventually — and any edit
    /// changes the key, so fixed math retries immediately).
    pub fn request(&mut self, latex: &str) {
        if latex.trim().is_empty()
            || self.cache.contains_key(latex)
            || self.pending.contains(latex)
            || self.errors.contains_key(latex)
            || self.pending.len() >= MAX_PENDING
        {
            return;
        }
        let key = latex.to_string();
        let tx = self.tx.clone();
        self.pending.insert(key.clone());
        std::thread::spawn(move || {
            let img = compile_preview(&key);
            let _ = tx.send((key, img));
        });
    }

    /// Drain finished jobs → upload textures. Call every frame.
    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok((key, res)) = self.rx.try_recv() {
            self.pending.remove(&key);
            match res {
                Ok(img) => self.done.push((key, img)),
                Err(e) => {
                    if !self.errors.contains_key(&key) {
                        self.errors_order.push_back(key.clone());
                    }
                    self.errors.insert(key, e);
                    while self.errors.len() > MAX_ERRORS {
                        match self.errors_order.pop_front() {
                            Some(old) => {
                                self.errors.remove(&old);
                            }
                            None => break,
                        }
                    }
                }
            }
        }
        for (key, img) in self.done.drain(..) {
            let name = format!("math preview: {}", key.get(..24).unwrap_or(key.as_str()));
            let tex = ctx.load_texture(name, img, egui::TextureOptions::LINEAR);
            while self.cache.len() >= MAX_CACHED {
                match self.cache_order.pop_front() {
                    Some(old) => {
                        self.cache.remove(&old);
                    }
                    None => break,
                }
            }
            self.cache.insert(key.clone(), tex);
            self.cache_order.push_back(key.clone());
            // A success supersedes any earlier error for the key.
            if self.errors.remove(&key).is_some() {
                if let Some(i) = self.errors_order.iter().position(|k| *k == key) {
                    self.errors_order.remove(i);
                }
            }
        }
        // Keep repainting while compiles are in flight so the texture
        // lands the frame it's ready.
        if !self.pending.is_empty() {
            ctx.request_repaint();
        }
    }

    /// Ready texture for `latex`, if any.
    pub fn texture(&self, latex: &str) -> Option<&egui::TextureHandle> {
        self.cache.get(latex)
    }

    /// Latest error for `latex`.
    pub fn error(&self, latex: &str) -> Option<&str> {
        self.errors.get(latex).map(String::as_str)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for MathPreview {
    fn default() -> Self {
        Self::new()
    }
}

/// Wasm: no filesystem, no TeX engine — a same-shaped no-op so call
/// sites stay unconditional.
#[cfg(target_arch = "wasm32")]
#[derive(Default)]
pub struct MathPreview;

#[cfg(target_arch = "wasm32")]
impl MathPreview {
    pub fn new() -> Self {
        Self
    }

    /// No-op on wasm.
    pub fn request(&mut self, _latex: &str) {}

    /// No-op on wasm.
    pub fn poll(&mut self, _ctx: &egui::Context) {}

    /// Always `None` on wasm.
    pub fn texture(&self, _latex: &str) -> Option<&egui::TextureHandle> {
        None
    }

    /// Always `None` on wasm.
    pub fn error(&self, _latex: &str) -> Option<&str> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{math_at, math_span};

    #[test]
    fn dollar_inline() {
        assert_eq!(math_at("$x^2$", 2).as_deref(), Some("x^2"));
        assert_eq!(math_span("$x^2$", 2), Some((0, 5)));
    }

    #[test]
    fn paren_and_bracket_inline() {
        assert_eq!(math_at("\\(a\\)", 3).as_deref(), Some("a"));
        assert_eq!(math_span("\\(a\\)", 3), Some((0, 5)));
        assert_eq!(math_at("\\[b\\]", 3).as_deref(), Some("b"));
    }

    #[test]
    fn double_dollar_display() {
        assert_eq!(math_at("$$c$$", 2).as_deref(), Some("c"));
        assert_eq!(math_span("$$c$$", 2), Some((0, 5)));
    }

    #[test]
    fn env_body_stripped() {
        let src = "\\begin{align*}a&=b\\end{align*}";
        assert_eq!(math_at(src, 15).as_deref(), Some("a&=b"));
        assert_eq!(math_span(src, 15), Some((0, 30)));
        let eq = "\\begin{equation}E=mc^2\\end{equation}";
        assert_eq!(math_at(eq, 18).as_deref(), Some("E=mc^2"));
    }

    #[test]
    fn plain_text_is_none() {
        assert_eq!(math_at("just text", 4), None);
        assert_eq!(math_span("just text", 4), None);
        // A non-math env is not a math region either.
        assert_eq!(math_at("\\begin{document}x\\end{document}", 16), None);
    }

    #[test]
    fn escaped_dollar_is_not_a_delimiter() {
        let src = "cost \\$5 and $x$";
        assert_eq!(math_at(src, 6), None); // the escaped `\$`
        assert_eq!(math_at(src, 9), None); // plain text between
        assert_eq!(math_at(src, 14).as_deref(), Some("x"));
    }

    #[test]
    fn caret_at_boundaries() {
        assert_eq!(math_at("$x$", 0).as_deref(), Some("x")); // on the opener
        assert_eq!(math_at("$x$", 2).as_deref(), Some("x")); // on the closer
        assert_eq!(math_at("$x$", 3), None); // just past it
        assert_eq!(math_at("a$x$", 0), None); // outside, left
    }

    #[test]
    fn multibyte_char_offsets() {
        // 'α' is 2 bytes: `ci` is a char index, `math_span` returns bytes.
        let src = "α $x$";
        assert_eq!(math_at(src, 3).as_deref(), Some("x"));
        assert_eq!(math_span(src, 3), Some((3, 6)));
    }

    #[test]
    fn innermost_wins_when_nested() {
        let src = "$a\\(b\\)c$";
        assert_eq!(math_at(src, 4).as_deref(), Some("b")); // inside \(…\)
        assert_eq!(math_at(src, 1).as_deref(), Some("a\\(b\\)c")); // outer $…$
        let env = "\\begin{equation}\\(x\\)\\end{equation}";
        assert_eq!(math_at(env, 18).as_deref(), Some("x"));
    }

    #[test]
    fn weird_dollar_runs_nearest_pair() {
        // `$$`…`$$` is the only matched pair enclosing the `b`.
        assert_eq!(math_at("$$a$b$$", 4).as_deref(), Some("a$b"));
        assert_eq!(math_span("$$a$b$$", 4), Some((0, 7)));
    }

    #[test]
    fn unmatched_is_none() {
        assert_eq!(math_at("a $x", 3), None);
        assert_eq!(math_at("$a\\(", 3), None);
        assert_eq!(math_at("$a$ and $b$", 5), None);
        assert_eq!(math_at("$a$ and $b$", 8).as_deref(), Some("b"));
    }
}
