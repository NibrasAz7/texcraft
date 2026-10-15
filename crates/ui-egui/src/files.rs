//! Project file tree — a shallow, bounded scan of the document's folder
//! for the sidebar. Not a VCS: we list everything and flag the kinds we
//! know.

use std::path::{Path, PathBuf};

/// File kinds the tree knows how to badge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Dir,
    Tex,
    Bib,
    Style,
    Image,
    Other,
}

/// One row in the flattened tree (`depth` = indent level).
#[derive(Debug)]
pub struct Node {
    pub name: String,
    pub path: PathBuf,
    pub kind: Kind,
    pub depth: usize,
}

/// Hard caps: never descend deeper or list more than this — trees under
/// `read_dir` are untrusted input.
const MAX_DEPTH: usize = 6;
const MAX_NODES: usize = 400;

/// Directories never shown (build output, VCS, caches).
const SKIP_DIRS: &[&str] = &[".texcraft", ".git", "target", "node_modules"];

fn kind_of(path: &Path, is_dir: bool) -> Kind {
    if is_dir {
        return Kind::Dir;
    }
    match path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()) {
        Some(e) if e == "tex" || e == "ltx" => Kind::Tex,
        Some(e) if e == "bib" => Kind::Bib,
        Some(e) if e == "sty" || e == "cls" || e == "clo" => Kind::Style,
        Some(e) if matches!(e.as_str(), "png" | "jpg" | "jpeg" | "pdf" | "svg" | "eps") => Kind::Image,
        _ => Kind::Other,
    }
}

/// Scan `root` into a flat pre-order node list (dirs first, then files,
/// both name-sorted). `expanded` holds dir paths that are open; a dir
/// not in the set renders closed without descending.
pub fn scan(root: &Path, expanded: &dyn Fn(&Path) -> bool) -> Vec<Node> {
    let mut out = Vec::new();
    visit(root, 0, &mut out, expanded);
    out
}

fn visit(dir: &Path, depth: usize, out: &mut Vec<Node>, expanded: &dyn Fn(&Path) -> bool) {
    if depth > MAX_DEPTH || out.len() >= MAX_NODES {
        return;
    }
    let Ok(read) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<(String, PathBuf, bool)> = read
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                return None;
            }
            let path = e.path();
            let is_dir = path.is_dir();
            if is_dir && SKIP_DIRS.contains(&name.as_str()) {
                return None;
            }
            Some((name, path, is_dir))
        })
        .collect();
    entries.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())));
    for (name, path, is_dir) in entries {
        if out.len() >= MAX_NODES {
            return;
        }
        out.push(Node { name, path: path.clone(), kind: kind_of(&path, is_dir), depth });
        if is_dir && expanded(&path) {
            visit(&path, depth + 1, out, expanded);
        }
    }
}
