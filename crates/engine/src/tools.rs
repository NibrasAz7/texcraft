//! TeXstudio-style user tools: named external commands runnable per
//! session through the `user_tool.*` commands.
//!
//! SAFETY: a tool is a fixed argv — `argv[0]` is the program and the
//! rest are passed verbatim to `Command::args`. No shell (`sh -c`,
//! `cmd /c`) is ever inserted, so no tool string is re-parsed: there
//! is no quoting, expansion or injection surface. Captured output is
//! capped per stream and every captured run has a timeout after which
//! the child is killed — a hung or runaway tool cannot block or OOM
//! the session.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{json, Value};

use crate::{EngineError, Session};

/// Max bytes captured per output stream before truncation.
const CAP: usize = 256 * 1024;
/// Default `user_tool.run` timeout.
const DEFAULT_TIMEOUT_S: u64 = 60;
/// Hard cap on the `timeout_s` parameter.
const MAX_TIMEOUT_S: u64 = 300;

/// A named external command the user can run from the session.
#[derive(Debug, Clone)]
pub struct UserTool {
    /// Display name, shown in menus.
    pub name: String,
    /// Program + args; `argv[0]` is the executable.
    pub argv: Vec<String>,
    /// Working dir; default = the session file's directory.
    pub cwd: Option<PathBuf>,
    /// Capture stdout+stderr (true) vs fire-and-forget (false).
    pub capture: bool,
}

/// `{"name"?, "argv": […], "cwd"?, "capture"?}` → `UserTool`.
/// `name` defaults to the program; `capture` defaults to true.
/// `argv` must be non-empty with a non-blank program.
pub fn from_json(v: &Value) -> Result<UserTool, EngineError> {
    let arr = v.get("argv").and_then(Value::as_array).ok_or(EngineError::BadParam("argv"))?;
    let mut argv = Vec::with_capacity(arr.len());
    for a in arr {
        argv.push(a.as_str().ok_or(EngineError::BadParam("argv"))?.to_string());
    }
    if argv.first().is_none_or(|p| p.trim().is_empty()) {
        return Err(EngineError::BadParam("argv"));
    }
    let name = v.get("name").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| argv.first().cloned().unwrap_or_default());
    let cwd = v.get("cwd").and_then(Value::as_str).filter(|s| !s.is_empty()).map(PathBuf::from);
    let capture = v.get("capture").and_then(Value::as_bool).unwrap_or(true);
    Ok(UserTool { name, argv, cwd, capture })
}

/// `UserTool` → JSON for `user_tool.list`.
pub fn to_json(t: &UserTool) -> Value {
    json!({
        "name": t.name,
        "argv": t.argv,
        "cwd": t.cwd.as_ref().map(|p| p.display().to_string()),
        "capture": t.capture,
    })
}

/// `user_tool.run`: `{"name"}` runs a configured tool; inline
/// `{"argv": […], "name"?, "cwd"?, "capture"?}` runs a one-shot.
/// Optional `timeout_s` (clamped 1–300, default 60).
#[allow(clippy::disallowed_methods)] // Instant: engine is desktop-only; std::process has no wasm target
pub fn run(session: &Session, params: &Value) -> Result<Value, EngineError> {
    let tool = if params.get("argv").is_some() {
        from_json(params)?
    } else {
        let name = params.get("name").and_then(Value::as_str).ok_or(EngineError::BadParam("name"))?;
        session.user_tools().iter().find(|t| t.name == name).cloned().ok_or_else(|| EngineError::Tool(format!("no user tool '{name}'")))?
    };
    let timeout = Duration::from_secs(params.get("timeout_s").and_then(Value::as_u64).unwrap_or(DEFAULT_TIMEOUT_S).clamp(1, MAX_TIMEOUT_S));

    let doc = session.path().and_then(|p| std::path::absolute(p).ok());
    let argv: Vec<String> = tool.argv.iter().map(|a| substitute(a, doc.as_deref())).collect();
    let prog = argv.first().filter(|p| !p.trim().is_empty()).ok_or(EngineError::BadParam("argv"))?;
    let cwd = tool.cwd.clone().or_else(|| doc.as_deref().and_then(Path::parent).map(Path::to_path_buf));

    let mut cmd = Command::new(prog);
    cmd.args(argv.iter().skip(1)).stdin(Stdio::null());
    if let Some(dir) = &cwd {
        cmd.current_dir(dir);
    }

    let started = std::time::Instant::now();
    if !tool.capture {
        // Fire-and-forget: spawn detached and reap on a thread so the
        // child can't linger as a zombie; return without waiting.
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
        let mut child = cmd.spawn()?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        return Ok(result_json(&tool.name, &argv, None, b"", b"", started.elapsed(), false));
    }

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    // One reader thread per pipe, each capped at CAP bytes — pipes
    // stay drained so a chatty child can't deadlock on a full buffer.
    let (out_tx, out_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let (err_tx, err_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    if let Some(p) = child.stdout.take() {
        std::thread::spawn(move || {
            let _ = out_tx.send(read_capped(p));
        });
    }
    if let Some(p) = child.stderr.take() {
        std::thread::spawn(move || {
            let _ = err_tx.send(read_capped(p));
        });
    }

    // Poll the child; on timeout kill + reap it, then collect whatever
    // the reader threads captured.
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            timed_out = true;
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    // Pipes normally close at child exit and the reads return at once;
    // a killed process can leave grandchildren holding them (e.g.
    // `cmd /c ping` on Windows) — bound the drain so the session never
    // blocks, falling back to empty output.
    let drain = Duration::from_secs(2);
    let stdout = out_rx.recv_timeout(drain).unwrap_or_default();
    let stderr = err_rx.recv_timeout(drain).unwrap_or_default();
    Ok(result_json(&tool.name, &argv, status.and_then(|s| s.code()), &stdout, &stderr, started.elapsed(), timed_out))
}

fn result_json(name: &str, argv: &[String], status: Option<i32>, stdout: &[u8], stderr: &[u8], dur: Duration, timed_out: bool) -> Value {
    json!({
        "name": name,
        "argv": argv,
        "status": status,
        "stdout": String::from_utf8_lossy(stdout),
        "stderr": String::from_utf8_lossy(stderr),
        "duration_ms": u64::try_from(dur.as_millis()).unwrap_or(u64::MAX),
        "timed_out": timed_out,
    })
}

/// Drain the pipe keeping at most CAP bytes; on overflow append a
/// `\n[truncated]` marker. Reads to EOF even past the cap so a chatty
/// child never blocks on a full pipe buffer.
fn read_capped<R: Read>(mut r: R) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    let mut overflow = false;
    loop {
        let n = match r.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        let room = CAP.saturating_sub(out.len());
        let keep = n.min(room);
        out.extend_from_slice(&buf[..keep]);
        overflow |= keep < n;
    }
    if overflow {
        out.extend_from_slice(b"\n[truncated]");
    }
    out
}

/// TeXstudio-style placeholders (braces, not `%`, to stay URL/path
/// safe): `{file}` absolute document path, `{dir}` its directory,
/// `{stem}` the file name without extension, `{basename}` with. No
/// open file → all substitute empty. Unknown `{…}` pass through.
fn substitute(arg: &str, doc: Option<&Path>) -> String {
    let file = doc.map(|p| p.display().to_string()).unwrap_or_default();
    let dir = doc.and_then(Path::parent).map(|p| p.display().to_string()).unwrap_or_default();
    let stem = doc.and_then(Path::file_stem).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let basename = doc.and_then(Path::file_name).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    arg.replace("{file}", &file).replace("{dir}", &dir).replace("{stem}", &stem).replace("{basename}", &basename)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Portable "echo" argv — the platform shell is fine inside a TEST;
    // the user-facing path is still fixed-argv with no shell inserted.
    #[cfg(windows)]
    const ECHO: &[&str] = &["cmd", "/c", "echo", "hello"];
    #[cfg(unix)]
    const ECHO: &[&str] = &["sh", "-c", "echo hello"];

    #[cfg(windows)]
    fn echo_of(text: &str) -> Value {
        json!(["cmd", "/c", "echo", text])
    }
    #[cfg(unix)]
    fn echo_of(text: &str) -> Value {
        json!(["sh", "-c", format!("echo {text}")])
    }

    #[test]
    fn set_list_clear_round_trip() {
        let mut s = Session::new();
        s.execute(
            "user_tool.set",
            &json!({"tools": [
                {"name": "lint", "argv": ["chktex", "{file}"], "capture": true},
                {"name": "commit", "argv": ["git", "commit", "-m", "wip"], "cwd": "/tmp", "capture": false},
            ]}),
        )
        .unwrap();
        let list = s.execute("user_tool.list", &json!({})).unwrap();
        assert_eq!(list["tools"].as_array().map(Vec::len), Some(2));
        assert_eq!(list["tools"][0]["name"], "lint");
        assert_eq!(list["tools"][0]["argv"], json!(["chktex", "{file}"]));
        assert_eq!(list["tools"][0]["cwd"], Value::Null);
        assert_eq!(list["tools"][1]["cwd"], "/tmp");
        assert_eq!(list["tools"][1]["capture"], false);
        assert_eq!(s.user_tools().len(), 2);
        s.execute("user_tool.clear", &json!({})).unwrap();
        assert_eq!(s.execute("user_tool.list", &json!({})).unwrap()["tools"], json!([]));
        assert!(s.user_tools().is_empty());
    }

    #[test]
    fn run_echo_named_and_one_shot() {
        let mut s = Session::new();
        s.execute("user_tool.set", &json!({"tools": [{"name": "hi", "argv": ECHO}]})).unwrap();
        let out = s.execute("user_tool.run", &json!({"name": "hi"})).unwrap();
        assert_eq!(out["status"], 0);
        assert_eq!(out["name"], "hi");
        assert!(out["stdout"].as_str().unwrap_or("").contains("hello"));
        assert_eq!(out["timed_out"], false);
        assert!(out["duration_ms"].as_u64().is_some());
        // One-shot tool that was never registered.
        let out = s.execute("user_tool.run", &json!({"argv": ECHO, "name": "ad-hoc"})).unwrap();
        assert_eq!(out["name"], "ad-hoc");
        assert_eq!(out["status"], 0);
        assert!(out["stdout"].as_str().unwrap_or("").contains("hello"));
    }

    #[test]
    fn placeholders_substitute() {
        let mut s = Session::new();
        let doc = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/minimal.tex");
        s.execute("document.open", &json!({"path": doc})).unwrap();
        let out = s.execute("user_tool.run", &json!({"argv": echo_of("{basename},{stem},{file},{dir},{nope}")})).unwrap();
        let stdout = out["stdout"].as_str().unwrap_or("");
        assert!(stdout.contains("minimal.tex,minimal,"), "stdout: {stdout}");
        assert!(stdout.contains("{nope}"), "unknown placeholders pass through: {stdout}");
        let sent = out["argv"].as_array().unwrap();
        assert!(sent.iter().all(|a| !a.as_str().unwrap_or("").contains("{basename}")));
    }

    #[test]
    fn placeholders_empty_without_file() {
        assert_eq!(substitute("{basename},{stem}", None), ",");
        let mut s = Session::new();
        let out = s.execute("user_tool.run", &json!({"argv": echo_of("[{basename}]")})).unwrap();
        assert!(out["stdout"].as_str().unwrap_or("").contains("[]"));
    }

    #[test]
    fn fire_and_forget_returns_null_status() {
        let mut s = Session::new();
        let out = s.execute("user_tool.run", &json!({"argv": ECHO, "capture": false})).unwrap();
        assert_eq!(out["status"], Value::Null);
        assert_eq!(out["stdout"], "");
        assert_eq!(out["timed_out"], false);
    }

    #[test]
    fn timeout_kills_child() {
        let mut s = Session::new();
        #[cfg(windows)]
        let argv = json!(["ping", "-n", "30", "127.0.0.1"]);
        #[cfg(unix)]
        let argv = json!(["sh", "-c", "sleep 30"]);
        let out = s.execute("user_tool.run", &json!({"argv": argv, "timeout_s": 1})).unwrap();
        assert_eq!(out["timed_out"], true);
        assert_eq!(out["status"], Value::Null);
        assert!(out["duration_ms"].as_u64().unwrap_or(0) >= 900);
    }

    #[test]
    fn read_capped_truncates() {
        let big = vec![b'x'; CAP + 10];
        let out = read_capped(&big[..]);
        assert_eq!(out.len(), CAP + b"\n[truncated]".len());
        assert!(out.ends_with(b"\n[truncated]"));
        assert_eq!(read_capped(&b"hi"[..]), b"hi");
    }

    #[test]
    fn bad_runs_never_panic() {
        let mut s = Session::new();
        assert!(matches!(s.execute("user_tool.run", &json!({})), Err(EngineError::BadParam("name"))));
        assert!(matches!(s.execute("user_tool.run", &json!({"argv": []})), Err(EngineError::BadParam("argv"))));
        assert!(matches!(s.execute("user_tool.run", &json!({"argv": ["   "]})), Err(EngineError::BadParam("argv"))));
        assert!(matches!(s.execute("user_tool.run", &json!({"argv": [42]})), Err(EngineError::BadParam("argv"))));
        assert!(matches!(s.execute("user_tool.run", &json!({"name": "ghost"})), Err(EngineError::Tool(_))));
        assert!(s.execute("user_tool.run", &json!({"argv": ["texcraft-no-such-exe-zzz"]})).is_err());
        assert!(matches!(s.execute("user_tool.set", &json!({"tools": [{"argv": []}]})), Err(EngineError::BadParam("argv"))));
        assert!(matches!(s.execute("user_tool.set", &json!({})), Err(EngineError::BadParam("tools"))));
        // The session stays usable after tool errors.
        assert_eq!(s.execute("source.get", &json!({})).unwrap()["text"], "");
    }
}
