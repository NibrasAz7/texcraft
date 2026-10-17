//! Editor "magic comment" header directives.
//!
//! TeXstudio/TeXShop-style `key = value` settings embedded in the file
//! header: `% !TeX program = xelatex`, `% !TeX root = main.tex`,
//! `% !BIB program = biber`. Directive families are `% !TeX`/`% !TEX`
//! (case-insensitive) and `% !BIB`; keys are lowercased, values trimmed.
//!
//! Only the leading comment block is scanned — the first
//! [`MAX_LINES`](const@MAX_LINES) lines or [`MAX_BYTES`](const@MAX_BYTES)
//! bytes of source, whichever comes first — and the scan stops at the
//! first non-comment, non-empty line, so directives in the body are not
//! honored. Malformed or hostile input yields a default all-empty
//! [`Magic`]; parsing never fails.

/// Longest header block scanned for directives, in lines.
const MAX_LINES: usize = 100;
/// Longest header block scanned for directives, in bytes.
const MAX_BYTES: usize = 8 * 1024;

/// The engine/root/encoding hints editors read from magic comments.
///
/// `program`/`ts-program`, `root`, `encoding`, `spellcheck`/
/// `spellcheck-language` and `jobname` fill the matching fields;
/// `% !BIB program`, `% !BIB bibtex` and `% !TeX bibliography` set the
/// bibliography *program*. Recognised fieldless keys (`options`,
/// `aux-root`, `clean`, `makeindex`, `glossary`) and anything unknown
/// land in [`Magic::extras`] as `(key, value)` pairs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Magic {
    /// `% !TeX program` / `% !TEX ts-program` — the build tool.
    pub program: Option<String>,
    /// `% !TeX root` — the master file of a multi-file project.
    pub root: Option<String>,
    /// `% !TeX encoding` — e.g. `UTF-8`, `latin1`.
    pub encoding: Option<String>,
    /// `% !BIB program` / `% !TeX bibliography` — e.g. `biber`, `bibtex`.
    pub bibliography: Option<String>,
    /// `% !TeX spellcheck` — e.g. `en_US`, `none`.
    pub spellcheck: Option<String>,
    /// `% !TeX jobname` — output name override.
    pub jobname: Option<String>,
    /// `(key, value)` pairs with no dedicated field — recognised
    /// fieldless keys and unknown keys alike, in order of appearance.
    pub extras: Vec<(String, String)>,
}

impl Magic {
    /// Normalised build engine hinted by `program`/`ts-program`, falling
    /// back to `% !BIB program` when no program is set.
    ///
    /// The first whitespace-separated word is lowercased and stripped of
    /// a `.exe` suffix (`xelatex.exe`, `xelatex -synctex=1` → `"xelatex"`,
    /// `xetex` → `"xelatex"`). An unknown value is returned trimmed and
    /// verbatim.
    pub fn engine_hint(&self) -> Option<&str> {
        let raw = self.program.as_deref().or(self.bibliography.as_deref())?.trim();
        if raw.is_empty() {
            return None;
        }
        let mut name = raw.split_whitespace().next().unwrap_or(raw).to_ascii_lowercase();
        if name.ends_with(".exe") {
            name.truncate(name.len() - ".exe".len());
        }
        let engine = match name.as_str() {
            "pdflatex" | "pdftex" => "pdflatex",
            "xelatex" | "xetex" => "xelatex",
            "lualatex" | "luatex" => "lualatex",
            "latex" => "latex",
            "latexmk" => "latexmk",
            "bibtex" | "bibtex8" | "bibtexu" => "bibtex",
            "biber" => "biber",
            _ => return Some(raw),
        };
        Some(engine)
    }
}

/// Read the magic-comment header of a `.tex` source. Always succeeds;
/// absent or malformed directives leave fields empty.
pub fn parse(src: &str) -> Magic {
    let mut magic = Magic::default();
    let mut bytes = 0usize;
    for (i, raw) in src.strip_prefix('\u{feff}').unwrap_or(src).lines().enumerate() {
        if i >= MAX_LINES || bytes >= MAX_BYTES {
            break;
        }
        // Bound the per-line work to the remaining byte budget so a
        // single huge first line can't push us past MAX_BYTES. The cut
        // point is rounded down to a char boundary.
        let end = floor_boundary(raw, MAX_BYTES - bytes);
        let line = raw.get(..end).unwrap_or(raw);
        bytes += line.len() + 1;
        let body = line.trim_start();
        if body.is_empty() {
            continue;
        }
        if !body.starts_with('%') {
            break; // first code line ends the header block
        }
        if let Some((key, value, is_bib)) = directive(body) {
            assign(&mut magic, &key, &value, is_bib);
        }
    }
    magic
}

/// `% !TeX key = value` / `% !BIB key = value` → `(key, value, is_bib)`.
/// `line` starts at `%`; `!`, the directive family and the `=` are
/// required. Returns `None` for ordinary comments.
fn directive(line: &str) -> Option<(String, String, bool)> {
    let rest = line.strip_prefix('%')?.trim_start();
    let rest = rest.strip_prefix('!')?.trim_start();
    // Family name: a run of ASCII letters (`TeX`, `TEX`, `BIB`, …).
    let name_len = rest.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(rest.len());
    let (name, kv) = rest.split_at(name_len);
    let is_bib = match name.to_ascii_lowercase().as_str() {
        "tex" => false,
        "bib" => true,
        _ => return None,
    };
    // `key = value`, split on the FIRST `=` only — values may contain `=`.
    let (key, value) = kv.trim_start().split_once('=')?;
    let key = key.trim().to_ascii_lowercase();
    if key.is_empty() {
        return None;
    }
    Some((key, value.trim().to_string(), is_bib))
}

/// Store one directive; a later directive overrides an earlier field.
fn assign(m: &mut Magic, key: &str, value: &str, is_bib: bool) {
    let slot = match key {
        // `% !BIB program`/`bibtex` selects the bibliography tool.
        "program" => {
            if is_bib {
                &mut m.bibliography
            } else {
                &mut m.program
            }
        }
        "ts-program" => &mut m.program,
        "bibliography" | "bibtex" => &mut m.bibliography,
        "root" => &mut m.root,
        "encoding" => &mut m.encoding,
        "spellcheck" | "spellcheck-language" => &mut m.spellcheck,
        "jobname" => &mut m.jobname,
        _ => {
            m.extras.push((key.to_string(), value.to_string()));
            return;
        }
    };
    *slot = if value.is_empty() { None } else { Some(value.to_string()) };
}

/// Largest `n <= limit` that lies on a `s` char boundary.
fn floor_boundary(s: &str, limit: usize) -> usize {
    let mut n = limit.min(s.len());
    while n > 0 && !s.is_char_boundary(n) {
        n -= 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tex_directives() {
        let m = parse("% !TeX program = xelatex\n% !TeX root = main.tex\n% !TeX encoding = UTF-8\n% !TeX jobname = draft\n% !TeX spellcheck = en_US\n\\documentclass{article}\n");
        assert_eq!(m.program.as_deref(), Some("xelatex"));
        assert_eq!(m.root.as_deref(), Some("main.tex"));
        assert_eq!(m.encoding.as_deref(), Some("UTF-8"));
        assert_eq!(m.jobname.as_deref(), Some("draft"));
        assert_eq!(m.spellcheck.as_deref(), Some("en_US"));
        assert!(m.extras.is_empty());
        assert_eq!(m.engine_hint(), Some("xelatex"));
    }

    #[test]
    fn texshop_and_case_insensitive() {
        // TeXShop's `%!TEX TS-program` form, no space after `%`, shouty keys.
        let m = parse("%!TEX TS-PROGRAM = LuaLaTeX\n");
        assert_eq!(m.program.as_deref(), Some("LuaLaTeX")); // values keep case
        assert_eq!(m.engine_hint(), Some("lualatex"));
        let m = parse("% !TeX  \t PROGRAM\t=\tpdflatex -synctex=1\n");
        assert_eq!(m.program.as_deref(), Some("pdflatex -synctex=1"));
        assert_eq!(m.engine_hint(), Some("pdflatex"));
    }

    #[test]
    fn bib_directives() {
        let m = parse("% !BIB program = biber\n");
        assert_eq!(m.bibliography.as_deref(), Some("biber"));
        assert!(m.program.is_none());
        assert_eq!(m.engine_hint(), Some("biber"));
        // `bibtex` alias and `% !TeX bibliography`.
        let m = parse("% !BIB bibtex = bibtexu\n% !TeX bibliography = bibtex8\n");
        assert_eq!(m.bibliography.as_deref(), Some("bibtex8"));
        assert_eq!(m.engine_hint(), Some("bibtex"));
    }

    #[test]
    fn aliases_and_extras() {
        let m = parse("% !TeX spellcheck-language = de_DE\n% !TeX options = --shell-escape\n% !TeX clean = all\n% !TeX made-up-key = v\n");
        assert_eq!(m.spellcheck.as_deref(), Some("de_DE"));
        assert_eq!(
            m.extras,
            vec![
                ("options".to_string(), "--shell-escape".to_string()),
                ("clean".to_string(), "all".to_string()),
                ("made-up-key".to_string(), "v".to_string()),
            ]
        );
    }

    #[test]
    fn header_block_only() {
        // Directives after the first code line are ignored.
        let m = parse("% !TeX program = xelatex\n\n  % plain comment\n\\documentclass{article}\n% !TeX program = pdflatex\n");
        assert_eq!(m.program.as_deref(), Some("xelatex"));
        // Blank and indented comment lines stay inside the header block.
        let m = parse("\n\n   \n  % !TeX root = a.tex\n");
        assert_eq!(m.root.as_deref(), Some("a.tex"));
        // The 101st line is past the scan window.
        let mut src = "% comment\n".repeat(100);
        src.push_str("% !TeX program = xelatex\n");
        assert!(parse(&src).program.is_none());
    }

    #[test]
    fn no_directives() {
        let m = parse("\\documentclass{article}\n% !TeX program = xelatex\n");
        assert_eq!(m, Magic::default());
        assert_eq!(m.engine_hint(), None);
    }

    #[test]
    fn engine_hint_values() {
        let m = parse("% !TeX program = XeLaTeX.EXE\n");
        assert_eq!(m.engine_hint(), Some("xelatex"));
        let m = parse("% !TeX program = context\n");
        assert_eq!(m.engine_hint(), Some("context")); // verbatim fallback
        let m = parse("% !TeX program =\n");
        assert_eq!(m.engine_hint(), None); // empty value clears the field
    }

    #[test]
    fn value_with_equals() {
        let m = parse("% !TeX options = -a=b=c\n");
        assert_eq!(m.extras, vec![("options".to_string(), "-a=b=c".to_string())]);
    }

    #[test]
    fn hostile_never_panics() {
        for src in [
            "",
            "%",
            "% !",
            "% !TeX",
            "% !TeX ",
            "% !TeX program",
            "% !TeX = value",
            "% !TeX program =",
            "% !TeX program = x", // no newline at EOF
            "% !tex program = y",
            "\0\0\0",
            "% !TeX \0 = \0",
            "% !TeX program = xelatex\r\n\r\n\\documentclass{article}",
            "\u{feff}% !TeX program = xelatex\n",
        ] {
            let _ = parse(src);
        }
        let m = parse("\u{feff}% !TeX program = xelatex\n");
        assert_eq!(m.engine_hint(), Some("xelatex"));

        // 1 MiB of directives and one huge line: bounded, no panic.
        let big = "% !TeX program = xelatex\n".repeat(40_000);
        assert_eq!(parse(&big).engine_hint(), Some("xelatex"));
        let huge_line = format!("% !TeX program = {}", "x".repeat(1 << 20));
        let m = parse(&huge_line);
        assert!(m.program.as_deref().is_some_and(|v| v.len() <= MAX_BYTES));
    }
}
