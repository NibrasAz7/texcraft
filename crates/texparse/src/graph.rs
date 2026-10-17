//! Project-graph walk over `.tex` file dependencies.
//!
//! LaTeX projects split a document across files. [`extract_deps`] pulls
//! the file-level references (`\input`, `\include`, `\includegraphics`,
//! `\bibliography`, `\addbibresource`, `\documentclass`, `\usepackage`)
//! out of one source; [`resolve`] maps a reference to a path on disk;
//! [`build`] walks `\input`/`\include` edges from an entry file into a
//! [`ProjectGraph`] of roots, reachable sources, resources and unresolved
//! references for the UI to warn about.
//!
//! The scan runs on the same surface lexer as [`crate::structure`], so a
//! command inside a `%` comment (or an escaped `\%`) never counts as a
//! dependency.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};

use crate::lex::{lex, Tok, Token};
use crate::structure::{group_text, opt_arg, skip_ws, Issue};

/// Deepest `\input`/`\include` chain followed from the entry file.
const MAX_GRAPH_DEPTH: usize = 8;
/// Most files visited by one [`build`] call, entry included.
const MAX_GRAPH_FILES: usize = 256;
/// Extensions tried for extension-less `\includegraphics` targets.
const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "pdf", "eps"];

/// One file-level dependency extracted from a .tex source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dep {
    pub kind: DepKind,
    /// The argument as written — extension may be absent (`\input{ch1}`).
    pub target: String,
    /// 1-based line of the referencing command.
    pub line: usize,
}

/// What kind of command produced a [`Dep`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepKind {
    /// `\input{..}` — plain inclusion, any file.
    Input,
    /// `\include{..}` — `\includeonly`-aware inclusion, main file only.
    Include,
    /// `\includegraphics[..]{..}`.
    IncludeGraphics,
    /// `\bibliography{..}` (BibTeX; comma-separated list).
    Bibliography,
    /// `\addbibresource{..}` (biblatex; single file per call).
    AddBibResource,
    /// `\documentclass[..]{..}` — resolves to a `.cls`.
    DocumentClass,
    /// `\usepackage{..}` / `\RequirePackage{..}` — resolves to a `.sty`.
    UsePackage,
}

impl DepKind {
    /// Default extension tried when the target has none (`""` = image set).
    fn default_ext(self) -> &'static str {
        match self {
            DepKind::Input | DepKind::Include => "tex",
            DepKind::Bibliography | DepKind::AddBibResource => "bib",
            DepKind::UsePackage => "sty",
            DepKind::DocumentClass => "cls",
            DepKind::IncludeGraphics => "",
        }
    }
}

/// A resolved project: every reachable source file from a root.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProjectGraph {
    /// Files that contain `\documentclass` — root candidates.
    pub roots: Vec<PathBuf>,
    /// Reachable .tex files, BFS order from `entry` (entry first).
    pub files: Vec<PathBuf>,
    /// Resolved non-.tex deps (graphics, bib, cls, sty) that exist.
    pub resources: Vec<PathBuf>,
    /// Deps that could not be resolved to an existing file — pairs of
    /// (referrer, dep) so the UI can warn. An entry file that cannot be
    /// read is recorded with itself as referrer.
    pub missing: Vec<(PathBuf, Dep)>,
}

/// Extract every file-level dependency reference from `src` (raw .tex
/// text). `line` is 1-based. Commented-out commands are NOT deps.
/// Package lists (`\usepackage{a,b}`) yield one Dep per name, as do
/// `\bibliography{a,b}` lists. `\input{file}` may lack the `.tex`
/// extension — `target` stays as written; resolution lives in [`resolve`].
pub fn extract_deps(src: &str) -> Vec<Dep> {
    let toks = lex(src);
    let mut deps = Vec::new();
    // Brace-group diagnostics from group_text — deps don't surface them.
    let mut issues: Vec<Issue> = Vec::new();
    let mut i = 0usize;
    while let Some(tok) = toks.get(i) {
        i += 1;
        let name = match &tok.kind {
            Tok::Cmd(name) => name.as_str(),
            _ => continue,
        };
        let line = tok.pos.line;
        match name {
            "input" | "include" => {
                let kind = if name == "input" { DepKind::Input } else { DepKind::Include };
                if let Some((target, ni)) = file_arg(&toks, i, name, &mut issues) {
                    i = ni;
                    push_dep(&mut deps, kind, &target, line);
                }
            }
            "includegraphics" | "documentclass" => {
                let (_opts, ni) = opt_arg(&toks, i);
                i = ni;
                let kind = if name == "includegraphics" { DepKind::IncludeGraphics } else { DepKind::DocumentClass };
                if let Some((target, ni)) = file_arg(&toks, i, name, &mut issues) {
                    i = ni;
                    push_dep(&mut deps, kind, &target, line);
                }
            }
            "usepackage" | "RequirePackage" => {
                let (_opts, ni) = opt_arg(&toks, i);
                i = ni;
                if let Some((list, ni)) = file_arg(&toks, i, name, &mut issues) {
                    i = ni;
                    for part in list.split(',') {
                        push_dep(&mut deps, DepKind::UsePackage, part, line);
                    }
                }
            }
            "bibliography" => {
                if let Some((list, ni)) = file_arg(&toks, i, name, &mut issues) {
                    i = ni;
                    for part in list.split(',') {
                        push_dep(&mut deps, DepKind::Bibliography, part, line);
                    }
                }
            }
            "addbibresource" => {
                if let Some((target, ni)) = file_arg(&toks, i, name, &mut issues) {
                    i = ni;
                    push_dep(&mut deps, DepKind::AddBibResource, &target, line);
                }
            }
            _ => {}
        }
    }
    deps
}

/// Resolve `target` against `dir` for `kind`: Input/Include/
/// AddBibResource/Bibliography/UsePackage/DocumentClass try
/// `<target>.tex`/`.bib`/`.sty`/`.cls` as needed when the extension is
/// absent, falling back to the verbatim path; IncludeGraphics tries the
/// path verbatim plus common image extensions (png, jpg, jpeg, pdf, eps).
/// Returns the first candidate that exists on disk (checked with
/// `std::fs`), else the bare joined path. No network, no panics on odd
/// paths.
pub fn resolve(dep: &Dep, dir: &Path) -> PathBuf {
    let base = dir.join(&dep.target);
    let has_ext = Path::new(&dep.target).extension().is_some();
    let mut candidates: Vec<PathBuf> = Vec::new();
    if dep.kind == DepKind::IncludeGraphics {
        candidates.push(base.clone());
        if !has_ext {
            for ext in IMAGE_EXTS {
                candidates.push(base.with_extension(ext));
            }
        }
    } else if has_ext {
        candidates.push(base.clone());
    } else {
        // Extension-less typed ref: prefer the default extension (so a
        // directory named `ch1` never shadows `ch1.tex`), then verbatim.
        candidates.push(base.with_extension(dep.kind.default_ext()));
        candidates.push(base.clone());
    }
    for c in candidates {
        if c.is_file() {
            return c;
        }
    }
    base
}

/// Walk `\input`/`\include` edges from `entry` (a .tex path on disk).
/// Depth-capped at 8, file-capped at 256, cycle-safe (a file is visited
/// once, compared by canonical-or-as-is path). `roots` collects any
/// visited file containing `\documentclass` (case-sensitive). Unreadable
/// files are skipped into [`ProjectGraph::missing`].
pub fn build(entry: &Path) -> ProjectGraph {
    let mut g = ProjectGraph::default();
    let mut visited: HashSet<PathBuf> = HashSet::new();
    let mut queue: VecDeque<(PathBuf, usize, PathBuf, Dep)> = VecDeque::new();

    visited.insert(canon(entry));
    queue.push_back((entry.to_path_buf(), 0, entry.to_path_buf(), Dep { kind: DepKind::Input, target: file_name_lossy(entry), line: 0 }));

    while let Some((path, depth, referrer, via)) = queue.pop_front() {
        let src = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => {
                g.missing.push((referrer, via));
                continue;
            }
        };
        g.files.push(path.clone());
        if declares_document_class(&src) {
            g.roots.push(path.clone());
        }
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        for dep in extract_deps(&src) {
            let resolved = resolve(&dep, &dir);
            if !resolved.is_file() {
                g.missing.push((path.clone(), dep));
                continue;
            }
            match dep.kind {
                DepKind::Input | DepKind::Include => {
                    let key = canon(&resolved);
                    if depth < MAX_GRAPH_DEPTH && visited.len() < MAX_GRAPH_FILES && visited.insert(key) {
                        queue.push_back((resolved, depth + 1, path.clone(), dep));
                    }
                }
                _ => g.resources.push(resolved),
            }
        }
    }
    g
}

/// True when `src` declares `\documentclass` outside a comment.
pub fn declares_document_class(src: &str) -> bool {
    lex(src).iter().any(|t| matches!(&t.kind, Tok::Cmd(c) if c == "documentclass"))
}

/// `{..}` argument, or the bare `\input file` form: the next text token's
/// first whitespace-delimited word.
fn file_arg(toks: &[Token], i: usize, cmd: &str, issues: &mut Vec<Issue>) -> Option<(String, usize)> {
    let j = skip_ws(toks, i);
    match toks.get(j).map(|t| &t.kind) {
        Some(Tok::LBrace) => group_text(toks, i, cmd, issues),
        Some(Tok::Text(s)) => {
            let name = s.split_whitespace().next().unwrap_or_default();
            if name.is_empty() {
                None
            } else {
                Some((name.to_string(), j + 1))
            }
        }
        _ => None,
    }
}

fn push_dep(deps: &mut Vec<Dep>, kind: DepKind, target: &str, line: usize) {
    let target = target.trim();
    if !target.is_empty() {
        deps.push(Dep { kind, target: target.to_string(), line });
    }
}

/// Identity key for visited-set comparisons: canonical when cheap,
/// the path as-is otherwise (Windows-safe either way).
fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

fn file_name_lossy(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

    /// A unique tempdir that removes itself on drop.
    struct Fixture(PathBuf);

    impl Fixture {
        fn new(name: &str) -> Fixture {
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("texparse-graph-{name}-{}-{id}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Fixture(dir)
        }

        fn write(&self, rel: &str, text: &str) -> PathBuf {
            let p = self.0.join(rel);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&p, text).unwrap();
            p
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn dep(kind: DepKind, target: &str, line: usize) -> Dep {
        Dep { kind, target: target.to_string(), line }
    }

    fn file_names(paths: &[PathBuf]) -> Vec<String> {
        paths.iter().filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).collect()
    }

    #[test]
    fn extracts_all_dep_kinds() {
        let src = "\\documentclass[12pt]{article}\n\\usepackage{amsmath, graphicx}\n\\input{ch1}\n\\include{ch2.tex}\n\\includegraphics[scale=0.5]{fig}\n\\bibliography{refs}\n\\addbibresource{extra.bib}\n";
        let deps = extract_deps(src);
        assert_eq!(
            deps,
            vec![
                dep(DepKind::DocumentClass, "article", 1),
                dep(DepKind::UsePackage, "amsmath", 2),
                dep(DepKind::UsePackage, "graphicx", 2),
                dep(DepKind::Input, "ch1", 3),
                dep(DepKind::Include, "ch2.tex", 4),
                dep(DepKind::IncludeGraphics, "fig", 5),
                dep(DepKind::Bibliography, "refs", 6),
                dep(DepKind::AddBibResource, "extra.bib", 7),
            ]
        );
    }

    #[test]
    fn comments_and_escaped_percent() {
        // Commented commands are not deps; an escaped \% does not open a
        // comment, so the later \input still counts.
        let src = "% \\input{nope}\nreal \\% comment \\input{yes}\n% full line\n\\include{also}\n";
        let deps = extract_deps(src);
        assert_eq!(deps, vec![dep(DepKind::Input, "yes", 2), dep(DepKind::Include, "also", 4)]);
    }

    #[test]
    fn usepackage_splits_commas_and_trims() {
        let deps = extract_deps("\\usepackage[utf8]{ a , b ,c }\n");
        assert_eq!(deps, vec![dep(DepKind::UsePackage, "a", 1), dep(DepKind::UsePackage, "b", 1), dep(DepKind::UsePackage, "c", 1)]);
    }

    #[test]
    fn bibliography_splits_commas() {
        let deps = extract_deps("\\bibliography{one,two.bib}\n");
        assert_eq!(deps, vec![dep(DepKind::Bibliography, "one", 1), dep(DepKind::Bibliography, "two.bib", 1)]);
    }

    #[test]
    fn extensionless_input_kept_verbatim() {
        let deps = extract_deps("\\input{ch1}\n\\input{ch2.tex}\n");
        assert_eq!(deps, vec![dep(DepKind::Input, "ch1", 1), dep(DepKind::Input, "ch2.tex", 2)]);
    }

    #[test]
    fn bare_input_target() {
        let deps = extract_deps("\\input plainfile\ntext\n");
        assert_eq!(deps, vec![dep(DepKind::Input, "plainfile", 1)]);
    }

    #[test]
    fn empty_and_comment_only_args_skipped() {
        assert!(extract_deps("\\input{}\n\\input\n").is_empty());
    }

    #[test]
    fn document_class_detection() {
        assert!(declares_document_class("\\documentclass{article}"));
        assert!(declares_document_class("% lead\n  \\documentclass[twocolumn]{book}"));
        assert!(!declares_document_class("% \\documentclass{article}"));
        assert!(!declares_document_class("\\documentclassfake{y}"));
        assert!(!declares_document_class("plain text \\input{x}"));
    }

    #[test]
    fn resolve_fills_in_extensions() {
        let fx = Fixture::new("resolve");
        fx.write("ch1.tex", "");
        fx.write("img.pdf", "");
        fx.write("refs.bib", "");
        fx.write("book.cls", "");
        let dir = &fx.0;
        assert_eq!(resolve(&dep(DepKind::Input, "ch1", 1), dir), dir.join("ch1.tex"));
        assert_eq!(resolve(&dep(DepKind::Include, "ch1.tex", 1), dir), dir.join("ch1.tex"));
        assert_eq!(resolve(&dep(DepKind::IncludeGraphics, "img", 1), dir), dir.join("img.pdf"));
        assert_eq!(resolve(&dep(DepKind::Bibliography, "refs", 1), dir), dir.join("refs.bib"));
        assert_eq!(resolve(&dep(DepKind::DocumentClass, "book", 1), dir), dir.join("book.cls"));
        // Nothing exists: the bare joined path comes back.
        assert_eq!(resolve(&dep(DepKind::Input, "nope", 1), dir), dir.join("nope"));
        assert_eq!(resolve(&dep(DepKind::UsePackage, "amsmath", 1), dir), dir.join("amsmath"));
        assert_eq!(resolve(&dep(DepKind::IncludeGraphics, "gone.png", 1), dir), dir.join("gone.png"));
    }

    #[test]
    fn build_walks_inputs_includes_cycles() {
        let fx = Fixture::new("build");
        let main = fx.write(
            "main.tex",
            "\\documentclass{article}\n\\input{ch1}\n\\input{cyc_a}\n\\input{gone}\n\\includegraphics{pic}\n\\bibliography{refs}\n",
        );
        fx.write("ch1.tex", "chapter one\n\\include{sub/ch2}\n");
        fx.write("sub/ch2.tex", "chapter two\n");
        fx.write("cyc_a.tex", "\\input{cyc_b}\n");
        fx.write("cyc_b.tex", "\\input{cyc_a}\n");
        fx.write("pic.png", "png");
        fx.write("refs.bib", "@book{x,title={t}}\n");

        let g = build(&main);
        assert_eq!(file_names(&g.files), vec!["main.tex", "ch1.tex", "cyc_a.tex", "ch2.tex", "cyc_b.tex"]);
        assert_eq!(g.roots, vec![main.clone()]);
        assert_eq!(file_names(&g.resources), vec!["pic.png", "refs.bib"]);
        // Unresolvable refs: `article.cls` (not in the project dir — real
        // classes live in the TeX tree) and `gone`.
        assert_eq!(g.missing, vec![(main.clone(), dep(DepKind::DocumentClass, "article", 1)), (main.clone(), dep(DepKind::Input, "gone", 4)),]);
    }

    #[test]
    fn build_caps_depth() {
        let fx = Fixture::new("depth");
        let mut paths = Vec::new();
        for n in 0..10 {
            let next = format!("f{}.tex", n + 1);
            let src = if n < 9 { format!("\\input{{{next}}}\n") } else { "end\n".to_string() };
            paths.push(fx.write(&format!("f{n}.tex"), &src));
        }
        let g = build(&paths[0]);
        // Depths 0..=8 → nine files; f9 is one edge too far.
        assert_eq!(g.files.len(), 9);
        assert_eq!(file_names(&g.files)[8], "f8.tex");
    }

    #[test]
    fn build_unreadable_entry_goes_to_missing() {
        let fx = Fixture::new("unreadable");
        let g = build(&fx.0.join("not-there.tex"));
        assert!(g.files.is_empty());
        assert!(g.roots.is_empty());
        assert_eq!(g.missing.len(), 1);
        assert_eq!(g.missing[0].1.target, "not-there.tex");
    }
}
