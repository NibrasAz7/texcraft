//! Template gallery: shipped `<id>.tex` + `<id>.meta.toml` pairs.
//!
//! Discovery mirrors `find_tectonic()`: the `WARRAQCRAFT_TEMPLATES` env
//! override first (tests, packaging), then `templates/` next to the
//! executable, then the nearest ancestor directory containing
//! `templates/` — which covers dev checkouts where the binary lives
//! under `target/` and templates sit at the workspace root.
//!
//! Meta files are a restricted TOML subset — `name = "…"` and
//! `desc = "…"` lines only — parsed by hand so the engine needs no
//! `toml` dependency. A malformed meta file is skipped, never fatal;
//! a `.tex` without one still lists with `name = id`, `desc = ""`.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::EngineError;

/// Ids are file-name components; keeping them boring means an `id`
/// can never escape the templates directory (`..`, `/`, `\` rejected).
const ID_MAX: usize = 64;
/// Bound on one directory scan — template dirs are ours but the env
/// override can point anywhere.
const MAX_TEMPLATES: usize = 512;
/// Templates are small starter files; refuse anything silly-large so
/// `template.read`/`template.new` can't allocate unboundedly.
const MAX_TEMPLATE_BYTES: u64 = 1024 * 1024;
/// Meta `name`/`desc` values are short display strings.
const MAX_META_VALUE: usize = 256;

/// Shared id rule for template and snapshot ids: `[A-Za-z0-9_-]{1,64}`.
pub(crate) fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= ID_MAX && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// The templates directory, most-trusted first. `None` when the
/// install ships none.
fn templates_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("WARRAQCRAFT_TEMPLATES").map(PathBuf::from).filter(|p| p.is_dir()) {
        return Some(dir);
    }
    let exe = std::env::current_exe().ok()?;
    // ancestors: the exe's own directory ("templates/ next to the
    // exe") then every ancestor — `target/debug/deps/t.exe` reaches
    // the workspace root's `templates/` in three steps.
    exe.ancestors().skip(1).map(|a| a.join("templates")).find(|p| p.is_dir())
}

fn no_templates_dir() -> EngineError {
    EngineError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "no templates directory found"))
}

/// Extract the closing-quote-delimited string from a restricted TOML
/// basic-string value (`"…"` — `\"` and `\\` escapes honoured).
/// `None` when the value isn't a basic string.
fn toml_string(v: &str) -> Option<String> {
    let v = v.strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = v.chars();
    while let Some(c) = chars.next() {
        match c {
            // Anything after the closing quote (e.g. a `#` comment)
            // is ignored — the restricted format has nothing else.
            '"' => return Some(out),
            '\\' => match chars.next() {
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => return None,
            },
            _ => {
                if out.len() >= MAX_META_VALUE {
                    return None;
                }
                out.push(c);
            }
        }
    }
    None
}

/// Parsed `<id>.meta.toml`. `name`/`desc` are the default (English)
/// strings; `lang` tags the template's language (`"en"`, `"ar"`, …);
/// `localized` holds per-language name/desc overrides from `name_<lang>`
/// / `desc_<lang>` keys, so a template can title itself in any UI
/// language without a new field per language.
#[derive(Default)]
struct Meta {
    name: String,
    desc: String,
    lang: String,
    localized: std::collections::BTreeMap<String, (String, String)>,
}

/// `Meta` from a restricted meta file; `None` when the file can't be
/// read, carries no `name`, or has a non-string `name`/`desc` value.
/// Unknown keys are ignored so newer fields stay compatible.
fn parse_meta(path: &Path) -> Option<Meta> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut meta = Meta::default();
    let mut name = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let key = key.trim();
        match key {
            "name" if name.is_none() => name = Some(toml_string(value.trim())?),
            "desc" => meta.desc = toml_string(value.trim())?,
            "lang" => meta.lang = toml_string(value.trim())?,
            _ => {
                if let Some(l) = key.strip_prefix("name_") {
                    meta.localized.entry(l.to_string()).or_default().0 = toml_string(value.trim())?;
                } else if let Some(l) = key.strip_prefix("desc_") {
                    meta.localized.entry(l.to_string()).or_default().1 = toml_string(value.trim())?;
                }
            }
        }
    }
    meta.name = name?;
    Some(meta)
}

struct Entry {
    id: String,
    name: String,
    desc: String,
    lang: String,
    /// `(name, desc)` overrides keyed by language code.
    localized: std::collections::BTreeMap<String, (String, String)>,
    path: PathBuf,
}

/// Union of `*.tex` and valid `*.meta.toml` ids in the templates dir,
/// sorted by id. `.tex`-only entries fall back to `name = id`.
fn scan() -> Vec<Entry> {
    let Some(dir) = templates_dir() else { return Vec::new() };
    let Ok(rd) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut metas: std::collections::BTreeMap<String, Meta> = std::collections::BTreeMap::new();
    let mut texs: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for entry in rd.flatten() {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else { continue };
        if let Some(id) = file_name.strip_suffix(".meta.toml") {
            // Malformed meta files are skipped entirely.
            if valid_id(id) && metas.len() < MAX_TEMPLATES {
                if let Some(meta) = parse_meta(&path) {
                    metas.insert(id.to_string(), meta);
                }
            }
        } else if let Some(id) = file_name.strip_suffix(".tex") {
            if valid_id(id) && texs.len() < MAX_TEMPLATES {
                texs.insert(id.to_string());
            }
        }
    }
    // Union of meta and .tex ids (BTreeSet → sorted output); a `.tex`
    // without meta falls back to `name = id`, `desc = ""`.
    let mut ids: std::collections::BTreeSet<String> = metas.keys().cloned().collect();
    ids.extend(texs);
    ids.into_iter()
        .map(|id| {
            let meta = metas.remove(&id).unwrap_or_else(|| Meta { name: id.clone(), ..Meta::default() });
            Entry { path: dir.join(format!("{id}.tex")), id, name: meta.name, desc: meta.desc, lang: meta.lang, localized: meta.localized }
        })
        .collect()
}

/// The `<id>.tex` file plus its parsed meta (or id/`""` fallbacks).
fn template_source(id: &str) -> Result<(Entry, String), EngineError> {
    let dir = templates_dir().ok_or_else(no_templates_dir)?;
    let path = dir.join(format!("{id}.tex"));
    if !path.is_file() {
        return Err(EngineError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, format!("no template '{id}'"))));
    }
    if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(u64::MAX) > MAX_TEMPLATE_BYTES {
        return Err(EngineError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("template '{id}' too large"))));
    }
    let content = String::from_utf8_lossy(&std::fs::read(&path)?).into_owned();
    let meta = parse_meta(&dir.join(format!("{id}.meta.toml"))).unwrap_or_else(|| Meta { name: id.to_string(), ..Meta::default() });
    Ok((Entry { id: id.to_string(), name: meta.name, desc: meta.desc, lang: meta.lang, localized: meta.localized, path }, content))
}

/// `template.list {}` → `{"templates": [{"id","name","desc","lang","localized","path"}]}`.
/// Never fails on content — an absent or unreadable dir lists empty.
pub fn list() -> Result<Value, EngineError> {
    let templates = scan()
        .iter()
        .map(|e| {
            json!({"id": e.id, "name": e.name, "desc": e.desc, "lang": e.lang,
                   "localized": e.localized.iter().map(|(l, (n, d))| (l.clone(), json!({"name": n, "desc": d}))).collect::<serde_json::Map<_, _>>(),
                   "path": e.path.display().to_string()})
        })
        .collect::<Vec<_>>();
    Ok(json!({"templates": templates}))
}

/// `template.read {"id"}` → `{"id","name","desc","content"}`.
pub fn read(params: &Value) -> Result<Value, EngineError> {
    let id = params.get("id").and_then(Value::as_str).filter(|s| valid_id(s)).ok_or(EngineError::BadParam("id"))?;
    let (entry, content) = template_source(id)?;
    Ok(json!({"id": entry.id, "name": entry.name, "desc": entry.desc, "content": content}))
}

/// `template.new {"id","path","force"?}` → `{"created": "<path>"}`.
/// Refuses to overwrite an existing file unless `"force": true`;
/// parent directories are created.
pub fn new(params: &Value) -> Result<Value, EngineError> {
    let id = params.get("id").and_then(Value::as_str).filter(|s| valid_id(s)).ok_or(EngineError::BadParam("id"))?;
    let path = params.get("path").and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or(EngineError::BadParam("path"))?;
    let dest = Path::new(path);
    let force = params.get("force").and_then(Value::as_bool).unwrap_or(false);
    if dest.exists() && !force {
        return Err(EngineError::Io(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "file exists — pass \"force\": true to overwrite")));
    }
    let (_, content) = template_source(id)?;
    if let Some(parent) = dest.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    craft_atomic::atomic_write(dest, content.as_bytes())?;
    Ok(json!({"created": dest.display().to_string()}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serialise tests that flip WARRAQCRAFT_TEMPLATES — env is process-wide.
    static ENV_LOCK: Mutex<()> = Mutex::new(());
    static NEXT_DIR: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            let n = NEXT_DIR.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("warraqcraft-templates-test-{}-{tag}-{n}", std::process::id()));
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

    #[test]
    fn list_read_and_new_over_tempdir() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = TempDir::new("gallery");
        std::fs::write(dir.0.join("alpha.tex"), "\\documentclass{article}\n").unwrap();
        std::fs::write(dir.0.join("alpha.meta.toml"), "name = \"Alpha doc\"\ndesc = \"A starter.\"\n").unwrap();
        std::fs::write(dir.0.join("plain.tex"), "\\documentclass{report}\n").unwrap(); // no meta
        std::fs::write(dir.0.join("bad.tex"), "x\n").unwrap();
        std::fs::write(dir.0.join("bad.meta.toml"), "name = not-a-string\n").unwrap(); // malformed meta skipped
        std::env::set_var("WARRAQCRAFT_TEMPLATES", &dir.0);

        let mut s = crate::Session::new();
        let out = s.execute("template.list", &json!({})).unwrap();
        let list = out["templates"].as_array().unwrap();
        // `bad` lists via its .tex with id-fallback name (meta skipped).
        assert_eq!(list.len(), 3);
        let alpha = list.iter().find(|t| t["id"] == "alpha").unwrap();
        assert_eq!(alpha["name"], "Alpha doc");
        assert_eq!(alpha["desc"], "A starter.");
        let plain = list.iter().find(|t| t["id"] == "plain").unwrap();
        assert_eq!(plain["name"], "plain");
        assert_eq!(plain["desc"], "");
        assert_eq!(list.iter().find(|t| t["id"] == "bad").unwrap()["name"], "bad");

        let read = s.execute("template.read", &json!({"id": "alpha"})).unwrap();
        assert_eq!(read["name"], "Alpha doc");
        assert!(read["content"].as_str().unwrap().contains("\\documentclass"));

        // template.new: writes, refuses overwrite, honours force.
        let dest = dir.0.join("sub").join("doc.tex");
        let out = s.execute("template.new", &json!({"id": "alpha", "path": dest})).unwrap();
        assert!(out["created"].as_str().unwrap().ends_with("doc.tex"));
        assert!(dest.is_file());
        assert!(s.execute("template.new", &json!({"id": "alpha", "path": dest})).is_err());
        s.execute("template.new", &json!({"id": "alpha", "path": dest, "force": true})).unwrap();

        std::env::remove_var("WARRAQCRAFT_TEMPLATES");
    }

    #[test]
    fn hostile_ids_rejected() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = TempDir::new("ids");
        std::fs::write(dir.0.join("ok.tex"), "x\n").unwrap();
        std::env::set_var("WARRAQCRAFT_TEMPLATES", &dir.0);
        let mut s = crate::Session::new();
        for bad in ["../evil", "a/b", "a\\b", "", &"x".repeat(65)] {
            assert!(matches!(s.execute("template.read", &json!({"id": bad})), Err(EngineError::BadParam("id"))), "id {bad:?}");
            assert!(
                matches!(s.execute("template.new", &json!({"id": bad, "path": dir.0.join("o.tex")})), Err(EngineError::BadParam("id"))),
                "id {bad:?}"
            );
        }
        assert!(matches!(s.execute("template.read", &json!({"id": "ghost"})), Err(EngineError::Io(_))));
        assert!(matches!(s.execute("template.new", &json!({"id": "ok"})), Err(EngineError::BadParam("path"))));
        std::env::remove_var("WARRAQCRAFT_TEMPLATES");
    }
}
