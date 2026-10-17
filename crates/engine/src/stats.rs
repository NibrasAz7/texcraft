//! `text.stats`: document statistics — word/character/line counts plus
//! structural tallies (commands, math spans, environments, sections,
//! labels, citations). The counting lives in `texparse::stats`
//! (single source — the UI and other surfaces share it); this module
//! is the command plumbing: params → analyze → JSON.
//!
//! Conventions (see `texparse::stats` for the full doc): comments and
//! math-span text never count as words, command names never count,
//! `\begin{…}`/`\end{…}` environment names are consumed as structure,
//! and `\label`/`\cite`-argument text does count as words.

use serde_json::{json, Value};

use crate::{EngineError, Session};

/// `text.stats {"source"?: "<text>"}` → every `DocStats` field name
/// verbatim. `params.source` wins when present; absent → the session
/// buffer. A non-string `source` is `BadParam("source")`.
pub fn run(session: &Session, params: &Value) -> Result<Value, EngineError> {
    let st = match params.get("source") {
        Some(v) => texparse::analyze(v.as_str().ok_or(EngineError::BadParam("source"))?),
        None => texparse::analyze(session.source()),
    };
    Ok(json!({
        "words": st.words,
        "chars": st.chars,
        "chars_no_space": st.chars_no_space,
        "lines": st.lines,
        "commands": st.commands,
        "math_inline": st.math_inline,
        "math_display": st.math_display,
        "envs": st.envs,
        "sections": st.sections,
        "labels": st.labels,
        "cites": st.cites,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINI: &str = "\\documentclass{article}\n\\begin{document}\n\\section{Intro}\nOne two three.\n$x+y$ and \\[z\\]\n\\label{sec:a}\\cite{k1,k2}\n\\end{document}\n";

    #[test]
    fn stats_run_via_execute() {
        let mut s = Session::new();
        s.execute("source.set", &json!({"text": MINI})).unwrap();
        let v = s.execute("text.stats", &json!({})).unwrap();
        // All eleven fields present; counts come from texparse::stats
        // (whose own tests pin the exact semantics — here we only check
        // the plumbing delivers them).
        for k in ["words", "chars", "chars_no_space", "lines", "commands", "math_inline", "math_display", "envs", "sections", "labels", "cites"] {
            assert!(v.get(k).and_then(Value::as_u64).is_some(), "missing {k}");
        }
        assert_eq!(v["labels"], 1);
        assert_eq!(v["sections"], 1);
        assert_eq!(v["envs"], 1);
        // Math spans counted, their contents kept out of `words`.
        assert_eq!(v["math_inline"], 1);
        assert_eq!(v["math_display"], 1);
        assert!(v["words"].as_u64().unwrap_or(0) > 0);
        // `params.source` overrides the buffer; non-string → BadParam.
        let v = s.execute("text.stats", &json!({"source": "\\label{l} hi"})).unwrap();
        assert_eq!(v["labels"], 1);
        assert!(matches!(s.execute("text.stats", &json!({"source": 5})), Err(EngineError::BadParam("source"))));
    }

    #[test]
    fn stats_hostile_never_panics() {
        let mut s = Session::new();
        for src in ["", "\\", "$", "$$", "$$$", "\\(", "\\[", "((((", "\\begin{", "%\n", "\u{0}$\\"] {
            s.execute("source.set", &json!({"text": src})).unwrap();
            let _ = s.execute("text.stats", &json!({}));
        }
    }
}
