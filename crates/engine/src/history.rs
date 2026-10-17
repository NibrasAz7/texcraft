//! `project.snapshot` local history: a snapshot is a full copy of the
//! project's text sources under `.texcraft/snapshots/<id>/`, where the
//! project root is the open file's directory.
//!
//! Sources = `*.tex`, `*.bib`, `*.sty`, `*.cls`, `*.txt`, `*.toml`
//! directly under the root and one level of subdirectories — walk
//! depth ≤ 2, capped at 512 files, `.texcraft/` itself excluded, files
//! over 1 MiB skipped. `<id>` is a `%Y%m%d-%H%M%S` UTC timestamp with
//! `-2`, `-3` collision suffixes; each snapshot carries a `meta.json`
//! (`{"id","ts","label","files"}`) beside the copied tree.
//!
//! Every path stays under the project root: snapshot ids are validated
//! (`[A-Za-z0-9_-]{1,64}` — no separators) and relative paths come from
//! `strip_prefix` on files the walker itself found.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::{templates::valid_id, EngineError, Session};

/// Snapshot store inside the project's `.texcraft/` build dir.
const SNAPSHOTS: &str = "snapshots";
/// Source extensions eligible for snapshots (case-insensitive).
const SOURCE_EXTS: [&str; 6] = ["tex", "bib", "sty", "cls", "txt", "toml"];
/// Walk bound: the root plus one level of subdirectories.
const MAX_DEPTH: u32 = 2;
/// Hard cap on files per snapshot and per snapshot listing/read.
const MAX_FILES: usize = 512;
/// Files bigger than this are skipped — snapshots are for sources.
const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// `snapshot.read` returns at most this much content, total.
const MAX_READ_BYTES: u64 = 4 * 1024 * 1024;
/// `snapshot.list` returns at most this many entries.
const MAX_LIST: usize = 200;
/// Label character/length policy: `[A-Za-z0-9 _-]`, ≤ 120 chars.
const MAX_LABEL: usize = 120;
/// Timestamp-collision suffix attempts before giving up.
const MAX_COLLISIONS: u32 = 10_000;

/// Project root = the open file's directory.
fn project_root(session: &Session) -> Result<PathBuf, EngineError> {
    let path = session.path().ok_or(EngineError::NoFile)?;
    Ok(path.parent().unwrap_or_else(|| Path::new(".")).to_path_buf())
}

fn snapshots_dir(root: &Path) -> PathBuf {
    root.join(crate::BUILD_DIR).join(SNAPSHOTS)
}

/// Every regular file under `dir` down to `depth` levels (1 = direct
/// children), as sorted relative paths, bounded at `cap` entries so a
/// hostile tree can't allocate unboundedly.
fn walk_files(dir: &Path, depth: u32, cap: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    for _ in 0..depth {
        if pending.is_empty() || out.len() >= cap {
            break;
        }
        let mut next = Vec::new();
        for d in &pending {
            let Ok(rd) = std::fs::read_dir(d) else { continue };
            for entry in rd.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    if next.len() < cap {
                        next.push(p);
                    }
                } else if p.is_file() {
                    if let Ok(rel) = p.strip_prefix(dir) {
                        out.push(rel.to_path_buf());
                    }
                    if out.len() >= cap {
                        out.sort();
                        return out;
                    }
                }
            }
        }
        pending = next;
    }
    out.sort();
    out
}

/// Is `rel` a snapshot-worthy source file? Extension allow-list, size
/// cap, and never inside the `.texcraft/` build dir itself.
fn is_source(rel: &Path, abs: &Path) -> bool {
    if rel.components().next() == Some(std::path::Component::Normal(std::ffi::OsStr::new(crate::BUILD_DIR))) {
        return false;
    }
    let Some(ext) = rel.extension().and_then(|e| e.to_str()) else { return false };
    if !SOURCE_EXTS.iter().any(|s| ext.eq_ignore_ascii_case(s)) {
        return false;
    }
    abs.metadata().map(|m| m.len() <= MAX_FILE_BYTES).unwrap_or(false)
}

/// Source files eligible for snapshotting, relative to `root`.
fn collect_sources(root: &Path) -> Vec<PathBuf> {
    walk_files(root, MAX_DEPTH, MAX_FILES).into_iter().filter(|rel| is_source(rel, &root.join(rel))).collect()
}

/// All real files inside a snapshot dir (i.e. the copied tree), minus
/// the `meta.json` bookkeeping file at its top level.
fn snapshot_files(dir: &Path) -> Vec<PathBuf> {
    walk_files(dir, MAX_DEPTH, MAX_FILES).into_iter().filter(|rel| rel != Path::new("meta.json")).collect()
}

/// `<rel>` as a stable `/`-separated JSON key.
fn rel_key(rel: &Path) -> String {
    rel.components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Unix seconds → civil UTC `(year, month, day)` — the standard
/// days-since-epoch conversion.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    (yoe as i64 + era * 400 + i64::from(m <= 2), m as u32, d as u32)
}

/// `("%Y%m%d-%H%M%S" UTC, unix seconds)` for snapshot ids and meta.
#[allow(clippy::disallowed_methods)] // SystemTime: engine is desktop-only; std::process has no wasm target
fn now_utc() -> (String, u64) {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (y, mo, d) = civil_from_days((secs / 86_400) as i64);
    let rem = secs % 86_400;
    (format!("{y:04}{mo:02}{d:02}-{:02}{:02}{:02}", rem / 3600, (rem % 3600) / 60, rem % 60), secs)
}

/// `base`, or `base-2`, `base-3`, … — first id not already taken.
fn fresh_id(snap_root: &Path, base: &str) -> Result<String, EngineError> {
    for n in 0..MAX_COLLISIONS {
        let id = if n == 0 { base.to_string() } else { format!("{base}-{}", n + 1) };
        if !snap_root.join(&id).exists() {
            return Ok(id);
        }
    }
    Err(EngineError::Io(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "too many same-second snapshots")))
}

/// Keep only `[A-Za-z0-9 _-]`, then cap at 120 chars (char-safe —
/// every kept char is ASCII anyway).
fn sanitize_label(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric() || *c == ' ' || *c == '_' || *c == '-').take(MAX_LABEL).collect()
}

/// `project.snapshot {"label"?}` → `{"id", "files": n}`.
pub fn snapshot(session: &Session, params: &Value) -> Result<Value, EngineError> {
    let root = project_root(session)?;
    let label = params.get("label").and_then(Value::as_str).map(sanitize_label).unwrap_or_default();
    let snap_root = snapshots_dir(&root);
    std::fs::create_dir_all(&snap_root)?;
    let (ts, unix) = now_utc();
    let id = fresh_id(&snap_root, &ts)?;
    let dest = snap_root.join(&id);
    std::fs::create_dir_all(&dest)?;
    let mut files = 0u64;
    for rel in collect_sources(&root) {
        let to = dest.join(&rel);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(root.join(&rel), &to)?;
        files += 1;
    }
    let meta = json!({"id": id, "ts": unix, "label": label, "files": files});
    craft_atomic::atomic_write(&dest.join("meta.json"), meta.to_string().as_bytes())?;
    Ok(json!({"id": id, "files": files}))
}

/// `snapshot.list {}` → `{"snapshots": [{id,ts,label,files}]}`
/// newest-first, capped at 200. Snapshots with unreadable meta are
/// skipped.
pub fn list(session: &Session) -> Result<Value, EngineError> {
    let root = project_root(session)?;
    let mut snaps: Vec<Value> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(snapshots_dir(&root)) {
        for entry in rd.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(entry.path().join("meta.json")) else { continue };
            let Ok(meta) = serde_json::from_str::<Value>(&text) else { continue };
            snaps.push(json!({
                "id": meta.get("id").and_then(Value::as_str).unwrap_or_default(),
                "ts": meta.get("ts").and_then(Value::as_u64).unwrap_or_default(),
                "label": meta.get("label").and_then(Value::as_str).unwrap_or_default(),
                "files": meta.get("files").and_then(Value::as_u64).unwrap_or_default(),
            }));
            if snaps.len() >= MAX_LIST {
                break;
            }
        }
    }
    snaps.sort_by(|a, b| {
        b["ts"].as_u64().cmp(&a["ts"].as_u64()).then_with(|| b["id"].as_str().unwrap_or_default().cmp(a["id"].as_str().unwrap_or_default()))
    });
    Ok(json!({"snapshots": snaps}))
}

/// The validated snapshot directory for an `id` param.
fn snapshot_dir(session: &Session, params: &Value) -> Result<(PathBuf, PathBuf, String), EngineError> {
    let id = params.get("id").and_then(Value::as_str).filter(|s| valid_id(s)).ok_or(EngineError::BadParam("id"))?;
    let root = project_root(session)?;
    let dir = snapshots_dir(&root).join(id);
    if !dir.is_dir() {
        return Err(EngineError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, format!("no snapshot '{id}'"))));
    }
    Ok((root, dir, id.to_string()))
}

/// `snapshot.read {"id"}` → `{"id","files":{"<rel>":"<content>"}}`.
/// Content is lossy UTF-8; files past a 4 MiB total are omitted.
pub fn read(session: &Session, params: &Value) -> Result<Value, EngineError> {
    let (_, dir, id) = snapshot_dir(session, params)?;
    let mut files = serde_json::Map::new();
    let mut total = 0u64;
    for rel in snapshot_files(&dir) {
        let path = dir.join(&rel);
        let Ok(len) = path.metadata().map(|m| m.len()) else { continue };
        if total.saturating_add(len) > MAX_READ_BYTES {
            continue; // past the 4 MiB cap — skip, keep reading small files
        }
        let Ok(bytes) = std::fs::read(&path) else { continue };
        total += len;
        files.insert(rel_key(&rel), Value::String(String::from_utf8_lossy(&bytes).into_owned()));
    }
    Ok(json!({"id": id, "files": Value::Object(files)}))
}

/// `snapshot.restore {"id","confirm":true}` → `{"restored": n}`.
/// `confirm` must be literally `true` — restoring overwrites the
/// working tree. Files a snapshot could never hold (> 1 MiB, injected
/// by hand) are skipped rather than restored.
pub fn restore(session: &Session, params: &Value) -> Result<Value, EngineError> {
    if params.get("confirm").and_then(Value::as_bool) != Some(true) {
        return Err(EngineError::BadParam("confirm"));
    }
    let (root, dir, _) = snapshot_dir(session, params)?;
    let mut restored = 0u64;
    for rel in snapshot_files(&dir) {
        let src = dir.join(&rel);
        if src.metadata().map(|m| m.len()).unwrap_or(u64::MAX) > MAX_FILE_BYTES {
            continue;
        }
        let target = root.join(&rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        craft_atomic::atomic_write(&target, &std::fs::read(&src)?)?;
        restored += 1;
    }
    Ok(json!({"restored": restored}))
}

#[cfg(test)]
mod tests {
    use super::*;

    static NEXT_DIR: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            let n = NEXT_DIR.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("texcraft-history-test-{}-{tag}-{n}", std::process::id()));
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

    /// Session opened on `<dir>/main.tex` plus a small source tree.
    fn fixture(dir: &Path) -> Session {
        std::fs::write(dir.join("main.tex"), "\\documentclass{article}\n\\begin{document}v1\\end{document}\n").unwrap();
        std::fs::write(dir.join("refs.bib"), "@book{x}\n").unwrap();
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub").join("notes.txt"), "notes\n").unwrap();
        std::fs::create_dir_all(dir.join("sub").join("deep")).unwrap();
        std::fs::write(dir.join("sub").join("deep").join("too-deep.tex"), "skip me\n").unwrap(); // depth 3
        let mut s = Session::new();
        s.execute("document.open", &json!({"path": dir.join("main.tex")})).unwrap();
        s
    }

    #[test]
    fn snapshot_create_list_read_restore() {
        let dir = TempDir::new("roundtrip");
        let mut s = fixture(&dir.0);

        let snap = s.execute("project.snapshot", &json!({"label": "first draft §§"})).unwrap();
        let id = snap["id"].as_str().unwrap().to_string();
        assert!(valid_id(&id), "id looks like %Y%m%d-%H%M%S: {id}");
        assert_eq!(snap["files"], 3); // main.tex + refs.bib + sub/notes.txt

        // Listing: newest-first, label sanitised (`§` dropped).
        let list = s.execute("snapshot.list", &json!({})).unwrap();
        let snaps = list["snapshots"].as_array().unwrap();
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0]["id"], id.as_str());
        assert_eq!(snaps[0]["label"], "first draft ");
        assert_eq!(snaps[0]["files"], 3);
        assert!(snaps[0]["ts"].as_u64().unwrap_or(0) > 0);

        // Read: `/`-separated rel paths → contents.
        let read = s.execute("snapshot.read", &json!({"id": id})).unwrap();
        let files = read["files"].as_object().unwrap();
        assert!(files["main.tex"].as_str().unwrap().contains("v1"));
        assert!(files["sub/notes.txt"].as_str().unwrap().contains("notes"));
        assert!(!files.contains_key("meta.json"));

        // Mutate the working tree, then restore must recover v1.
        std::fs::write(dir.0.join("main.tex"), "v2\n").unwrap();
        let out = s.execute("snapshot.restore", &json!({"id": id, "confirm": true})).unwrap();
        assert_eq!(out["restored"], 3);
        assert!(std::fs::read_to_string(dir.0.join("main.tex")).unwrap().contains("v1"));
    }

    #[test]
    fn restore_requires_confirm_and_valid_id() {
        let dir = TempDir::new("confirm");
        let mut s = fixture(&dir.0);
        let id = s.execute("project.snapshot", &json!({})).unwrap()["id"].as_str().unwrap().to_string();
        for params in [json!({"id": id}), json!({"id": id, "confirm": false}), json!({"id": id, "confirm": "yes"})] {
            assert!(matches!(s.execute("snapshot.restore", &params), Err(EngineError::BadParam("confirm"))));
        }
        assert!(std::fs::read_to_string(dir.0.join("main.tex")).unwrap().contains("v1")); // untouched
        assert!(matches!(s.execute("snapshot.restore", &json!({"id": "../x", "confirm": true})), Err(EngineError::BadParam("id"))));
        assert!(matches!(s.execute("snapshot.read", &json!({"id": "ghost"})), Err(EngineError::Io(_))));
        assert!(matches!(Session::new().execute("project.snapshot", &json!({})), Err(EngineError::NoFile)));
    }

    #[test]
    fn skips_texcraft_dir_and_big_files() {
        let dir = TempDir::new("skips");
        let mut s = fixture(&dir.0);
        std::fs::create_dir_all(dir.0.join(".texcraft")).unwrap();
        std::fs::write(dir.0.join(".texcraft").join("build.tex"), "junk\n").unwrap();
        std::fs::write(dir.0.join("big.tex"), vec![b'x'; MAX_FILE_BYTES as usize + 1]).unwrap();
        std::fs::write(dir.0.join("image.png"), vec![0u8; 8]).unwrap(); // not a source ext

        let snap = s.execute("project.snapshot", &json!({})).unwrap();
        assert_eq!(snap["files"], 3); // still just the three sources
        let read = s.execute("snapshot.read", &json!({"id": snap["id"]})).unwrap();
        let files = read["files"].as_object().unwrap();
        assert!(!files.contains_key("big.tex"));
        assert!(!files.keys().any(|k| k.contains(".texcraft")));
        assert!(!files.contains_key("image.png"));
    }

    #[test]
    fn timestamp_collision_appends_suffix() {
        let dir = TempDir::new("collide");
        let mut s = fixture(&dir.0);
        // Two snapshots inside the same second get `-2`.
        let a = s.execute("project.snapshot", &json!({})).unwrap()["id"].as_str().unwrap().to_string();
        let b = s.execute("project.snapshot", &json!({})).unwrap()["id"].as_str().unwrap().to_string();
        assert_ne!(a, b);
        if a[..15] == b[..15] {
            assert_eq!(b, format!("{a}-2"), "collision suffix: {a} vs {b}");
        }
        let list = s.execute("snapshot.list", &json!({})).unwrap();
        assert_eq!(list["snapshots"].as_array().unwrap().len(), 2);
    }
}
