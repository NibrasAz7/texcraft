//! `math.preview`: TeXstudio-style "preview selected math". A math
//! fragment is wrapped in a minimal `article` preamble, written to a
//! scratch file under `.warraqcraft/preview/` next to the document, and
//! compiled through the same [`compile_file`] pipeline as
//! `document.compile` — engine detection, tectonic preference,
//! nonstop/halt-on-error flags, log parsing; shell escape stays off.
//! The response carries the PDF path and page count; the UI
//! rasterizes and caches it. Read-only for the session: the scratch
//! file and build artifacts are the only filesystem effects.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::{compile_file, CompileError, EngineError, Session};

/// Largest accepted fragment — previews are small by definition.
const MAX_LATEX: usize = 64 * 1024;
/// `log` in the response: at most the last this-many characters of
/// the report's log tail.
const MAX_LOG_CHARS: usize = 1200;
/// Preview scratch dir under the document's `.warraqcraft/` build dir.
const PREVIEW_DIR: &str = "preview";

/// Minimal `article` preamble around the fragment. Fragments that
/// already carry their own math/environment delimiters (`$`, `\(`,
/// `\[`, `\begin{`) pass through verbatim; anything else is wrapped
/// in `\( … \)`. The preamble occupies the leading lines, so a
/// hostile fragment cannot smuggle `% !TeX` magic comments —
/// `parse_magic` only reads the leading comment block.
fn wrap_fragment(latex: &str) -> String {
    let t = latex.trim();
    let verbatim = t.starts_with('$') || t.starts_with("\\(") || t.starts_with("\\[") || t.starts_with("\\begin{");
    let body = if verbatim { t.to_string() } else { format!("\\( {t} \\)") };
    format!("\\documentclass{{article}}\n\\usepackage{{amsmath,amssymb}}\n\\pagestyle{{empty}}\n\\begin{{document}}\n{body}\n\\end{{document}}\n")
}

/// The open document's directory, or the process cwd for an untitled
/// session (history.rs's `project_root` parent-dir logic with a cwd
/// fallback instead of `NoFile`).
fn doc_dir(session: &Session) -> PathBuf {
    match session.path() {
        Some(p) => p.parent().unwrap_or_else(|| Path::new(".")).to_path_buf(),
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    }
}

/// The last ≤ `max` characters of `s`, cut on a char boundary.
fn char_tail(s: &str, max: usize) -> String {
    s.chars().skip(s.chars().count().saturating_sub(max)).collect()
}

/// `math.preview {"latex": "<fragment>"}` →
/// `{"pdf": "<abs path>"|null, "pages": n, "log": "<tail>",
///   "errors": [{"line","message"}]}` — the error list shares
/// `document.compile`'s shape. `BadParam("latex")` on a missing,
/// empty or >64KiB fragment; `Io`/`NoTexEngine` on compile failure.
pub fn run(session: &Session, params: &Value) -> Result<Value, EngineError> {
    let latex = params.get("latex").and_then(Value::as_str).filter(|s| s.len() <= MAX_LATEX).ok_or(EngineError::BadParam("latex"))?;
    if latex.trim().is_empty() {
        return Err(EngineError::BadParam("latex"));
    }
    let src = wrap_fragment(latex);
    let dir = doc_dir(session).join(crate::BUILD_DIR).join(PREVIEW_DIR);
    std::fs::create_dir_all(&dir)?;
    let scratch = dir.join("m.tex");
    craft_atomic::atomic_write(&scratch, src.as_bytes())?;

    // `compile_file` builds into `<scratch-dir>/.warraqcraft/`, i.e.
    // `.warraqcraft/preview/.warraqcraft/m.pdf` — still under the build dir.
    let report = compile_file(&scratch, Some(&src))?;
    Ok(json!({
        "pdf": report.pdf.as_ref().map(|p| p.display().to_string()),
        "pages": report.pages,
        "log": char_tail(&report.log_tail, MAX_LOG_CHARS),
        "errors": report.errors.iter().map(|e: &CompileError| json!({"line": e.line, "message": e.message})).collect::<Vec<_>>(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    static NEXT_DIR: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            let n = NEXT_DIR.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("warraqcraft-preview-test-{}-{tag}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn fixture(dir: &Path) -> Session {
        let main = dir.join("main.tex");
        std::fs::write(&main, "\\documentclass{article}\n\\begin{document}x\\end{document}\n").unwrap();
        let mut s = Session::new();
        s.execute("document.open", &json!({"path": main})).unwrap();
        s
    }

    #[test]
    fn bad_latex_is_bad_param_never_panic() {
        let mut s = Session::new();
        for p in [json!({}), json!({"latex": 42}), json!({"latex": ["$x$"]}), json!({"latex": ""}), json!({"latex": "   \n "})] {
            assert!(matches!(s.execute("math.preview", &p), Err(EngineError::BadParam("latex"))), "{p}");
        }
        let giant = json!({"latex": "x".repeat(MAX_LATEX + 1)});
        assert!(matches!(s.execute("math.preview", &giant), Err(EngineError::BadParam("latex"))));
        let max_ok_but_no_engine_or_fine = json!({"latex": "x".repeat(MAX_LATEX)});
        assert!(!matches!(s.execute("math.preview", &max_ok_but_no_engine_or_fine), Err(EngineError::BadParam(_))));
    }

    #[test]
    fn fragment_wrapping() {
        let plain = wrap_fragment(" x + y ");
        assert!(plain.starts_with("\\documentclass{article}"));
        assert!(plain.contains("\\( x + y \\)"));
        assert!(plain.ends_with("\\end{document}\n"));
        for v in ["$x$", "\\(x\\)", "\\[x\\]", "\\begin{equation}x\\end{equation}", "  $x$  "] {
            let wrapped = wrap_fragment(v);
            assert!(wrapped.contains(v.trim()), "{v}");
            assert!(!wrapped.contains("\\( \\"), "{v}");
        }
    }

    #[test]
    fn scratch_written_and_compile_resilient() {
        let dir = TempDir::new("compile");
        let mut s = fixture(&dir.0);
        let scratch = dir.0.join(crate::BUILD_DIR).join(PREVIEW_DIR).join("m.tex");
        if crate::detect_engines().is_empty() {
            // No engine: the scratch file is still written, then the
            // compile step errors — never a panic.
            assert!(s.execute("math.preview", &json!({"latex": "x+y"})).is_err());
            assert!(scratch.is_file());
            assert!(std::fs::read_to_string(&scratch).unwrap().contains("\\( x+y \\)"));
            return;
        }
        // An installed engine can still fail here — e.g. tectonic's
        // first-run bundle download on an offline box. Not a defect.
        if let Ok(v) = s.execute("math.preview", &json!({"latex": "x + y"})) {
            assert!(v["pdf"].as_str().is_some_and(|p| p.ends_with("m.pdf")), "{v}");
            assert!(v["pages"].as_u64().unwrap_or(0) >= 1, "{v}");
            assert!(v["errors"].as_array().is_some_and(Vec::is_empty), "{v}");
            assert!(v["log"].as_str().is_none_or(|l| l.chars().count() <= MAX_LOG_CHARS));
        }
        assert!(scratch.is_file());
    }
}
