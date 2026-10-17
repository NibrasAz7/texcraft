//! The TCP control channel (`--control FILE`) — an agent-facing JSON
//! command socket into the running app's [`Session`].
//!
//! **Opt-in only.** Nothing here runs unless the app is started with
//! `--control FILE`: [`Control::start`] binds `127.0.0.1` on a random
//! port, generates a fresh 128-bit token, and writes
//! `{"port":…,"token":"…","pid":…}` to `FILE` (owner-only on Unix).
//! The token is never logged; the file is removed on drop.
//!
//! Protocol: newline-delimited JSON, one request and one response per
//! line. Every request carries the token — there is no handshake:
//!
//! ```json
//! {"token": "…", "id": <any non-null json>, "cmd": "text.stats", "params": {…}}
//! ```
//!
//! Replies echo `id`:
//! `{"id":…,"ok":true,"result":<json>}` or `{"id":…,"ok":false,"error":<string>}`.
//!
//! - Missing/wrong `token` → `ok:false,"unauthorized"`, no execute.
//! - Missing/`null` `id` → `ok:false,"missing id"` — every request is
//!   a request; there are no notifications.
//! - Missing `cmd` → `ok:false,"missing cmd"`.
//! - A line over [`MAX_LINE`] bytes → `{"id":null,"ok":false,"error":
//!   "request too large"}` and the connection closes (the rest of the
//!   line is still in the stream and can't be resynchronized).
//! - [`MAX_IN_FLIGHT`] requests may wait for a frame; beyond that the
//!   sender sheds the load with an error reply instead of letting the
//!   queue grow without bound.
//!
//! Requests reach the UI thread through a bounded channel; [`Control::poll`]
//! drains it in `WarraqApp::logic` with `try_recv` only, so accepting work
//! never blocks the frame. **Known v1 limit:** the command itself runs
//! on the UI thread — `document.compile` freezes the window while TeX
//! runs (the GUI's own compile uses a worker; unifying that is future
//! work).

use std::io::{self, BufRead, BufReader, ErrorKind, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TrySendError};
use std::sync::{Arc, OnceLock};

use serde_json::{json, Value};
use warraqcraft_engine::Session;

/// Longest request line, 4 MiB — a generous ceiling for `source.set` on
/// a large document; longer lines are refused, not buffered.
const MAX_LINE: usize = 4 * 1024 * 1024;

/// Requests that may be waiting for a frame before the sender sheds
/// load — a flooded client can't grow memory without bound.
const MAX_IN_FLIGHT: usize = 64;

/// One authenticated request queued for the UI thread, answered on
/// `reply` with the complete response object.
struct CtrlRequest {
    id: Value,
    cmd: String,
    params: Value,
    reply: Sender<Value>,
}

/// The app half of the control channel: the request queue plus the
/// endpoint bookkeeping for cleanup. The accept and connection threads
/// are detached — they exit when their channels close.
pub struct Control {
    rx: Receiver<CtrlRequest>,
    port: u16,
    /// The bearer token — private, never logged (it lives only here and
    /// in the owner-only control file).
    token: Arc<str>,
    /// Wakes the UI when a request lands so an idle/minimized app still
    /// answers; set by `wake_on` once the egui context exists.
    ctx: Arc<OnceLock<egui::Context>>,
    /// The published control file, removed on drop.
    file: Option<PathBuf>,
    /// Tells the accept loop to stop; a loopback nudge unblocks it.
    stop: Arc<AtomicBool>,
}

impl Control {
    /// Bind `127.0.0.1` on a random port, spawn the accept loop, and
    /// publish `{"port","token","pid"}` to `path` (atomically: a staged
    /// file replaces `path`, so a planted link is never written through
    /// and a reader never sees a half-written file).
    ///
    /// A failed write fails `start` — the channel is pointless when a
    /// client can't learn the endpoint, and a file that couldn't be
    /// replaced may be another user's planted at a shared path.
    pub fn start(path: &Path) -> io::Result<Control> {
        let mut c = Self::bind()?;
        if let Err(e) = write_control_file(path, c.port, &c.token) {
            c.stop_listen();
            return Err(e);
        }
        c.file = Some(path.to_path_buf());
        Ok(c)
    }

    /// Bind + spawn, without publishing — the test seam.
    #[cfg(test)]
    fn start_no_file() -> io::Result<Control> {
        Self::bind()
    }

    fn bind() -> io::Result<Control> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        let token: Arc<str> = random_token().into();
        let (tx, rx) = mpsc::sync_channel(MAX_IN_FLIGHT);
        let stop = Arc::new(AtomicBool::new(false));
        let ctx = Arc::new(OnceLock::new());
        {
            let (tx, token, stop, ctx) = (tx.clone(), token.clone(), stop.clone(), ctx.clone());
            std::thread::Builder::new().name("warraqcraft-control".into()).spawn(move || accept_loop(listener, tx, token, stop, ctx))?;
        }
        Ok(Control { rx, port, token, ctx, file: None, stop })
    }

    /// The loopback port the channel listens on (for the launcher's
    /// "connection details in FILE" log line — never the token).
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Let request threads wake the UI (`Context::request_repaint` is
    /// thread-safe) so a reply doesn't wait for the next input event.
    pub(crate) fn wake_on(&mut self, ctx: egui::Context) {
        let _ = self.ctx.set(ctx);
    }

    /// Drain pending requests, executing each on `session` and replying
    /// through its oneshot. Called from `WarraqApp::logic` every frame;
    /// `try_recv` only — it never waits.
    pub fn poll(&self, session: &mut Session) {
        while let Ok(req) = self.rx.try_recv() {
            let resp = match session.execute(&req.cmd, &req.params) {
                Ok(result) => json!({"id": req.id, "ok": true, "result": result}),
                Err(e) => json!({"id": req.id, "ok": false, "error": e.to_string()}),
            };
            let _ = req.reply.send(resp);
        }
    }

    /// Stop the accept loop: set the flag, then nudge the listener with
    /// one loopback connection so `accept()` wakes up and sees it.
    fn stop_listen(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}

impl Drop for Control {
    fn drop(&mut self) {
        self.stop_listen();
        if let Some(f) = self.file.take() {
            let _ = std::fs::remove_file(f);
        }
    }
}

/// Accept connections until `stop`; each gets its own thread.
fn accept_loop(listener: TcpListener, tx: SyncSender<CtrlRequest>, token: Arc<str>, stop: Arc<AtomicBool>, ctx: Arc<OnceLock<egui::Context>>) {
    for stream in listener.incoming() {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        match stream {
            Ok(s) => {
                let (tx, token, ctx) = (tx.clone(), token.clone(), ctx.clone());
                if let Err(e) = std::thread::Builder::new().name("warraqcraft-control-conn".into()).spawn(move || conn(s, tx, token, ctx)) {
                    eprintln!("warraqcraft control: cannot spawn a connection thread: {e}");
                }
            }
            // A transient accept error (e.g. fd pressure) shouldn't kill
            // the channel — log and keep accepting.
            Err(e) => eprintln!("warraqcraft control: accept failed: {e}"),
        }
    }
}

/// One connection: read a request line, answer a reply line, repeat.
/// Replies for well-formed authenticated requests come from the UI
/// thread via the request's oneshot; everything else is answered here.
/// Every error ends the connection quietly — never a panic.
fn conn(stream: TcpStream, tx: SyncSender<CtrlRequest>, token: Arc<str>, ctx: Arc<OnceLock<egui::Context>>) {
    let Ok(read) = stream.try_clone() else { return };
    let mut write = stream;
    let mut reader = BufReader::new(read);
    let mut buf = Vec::new();
    loop {
        match read_line_limited(&mut reader, &mut buf, MAX_LINE) {
            Ok(LineRead::Line) => {}
            // End of stream or a failed read ends the connection.
            Ok(LineRead::End) | Err(_) => break,
            Ok(LineRead::TooLong) => {
                let _ = reply(&mut write, &err(Value::Null, "request too large"));
                graceful_close(&mut write, &mut reader);
                break;
            }
        }
        // Bytes that aren't UTF-8 end the connection, as `BufRead::lines` would.
        let Ok(line) = std::str::from_utf8(&buf) else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Some(resp) = handle(line, &tx, &token, &ctx) else { break };
        if reply(&mut write, &resp).is_err() {
            break;
        }
    }
}

/// Close a connection we answered on, without RST-ing the reply away:
/// FIN our write side, then drain whatever the peer still had in
/// flight (bounded) — a plain drop with unread peer data turns into a
/// reset that can eat the reply we just wrote.
fn graceful_close(write: &mut TcpStream, reader: &mut BufReader<TcpStream>) {
    let _ = write.shutdown(Shutdown::Write);
    let _ = reader.get_ref().set_read_timeout(Some(std::time::Duration::from_millis(200)));
    let mut sink = [0u8; 8192];
    let mut drained = 0usize;
    while drained < MAX_LINE {
        match reader.read(&mut sink) {
            Ok(0) | Err(_) => break,
            Ok(k) => drained += k,
        }
    }
}

/// Turn one request line into one response value. `None` closes the
/// connection (the app went away — there is no one left to answer).
fn handle(line: &str, tx: &SyncSender<CtrlRequest>, token: &str, ctx: &OnceLock<egui::Context>) -> Option<Value> {
    let msg: Value = match serde_json::from_str(line) {
        Ok(m) => m,
        Err(e) => return Some(err(Value::Null, &format!("parse error: {e}"))),
    };
    let id = msg.get("id").cloned().unwrap_or(Value::Null);
    if !msg.get("token").and_then(Value::as_str).is_some_and(|t| constant_time_eq(t, token)) {
        return Some(err(id, "unauthorized"));
    }
    if id.is_null() {
        return Some(err(Value::Null, "missing id"));
    }
    let Some(cmd) = msg.get("cmd").and_then(Value::as_str) else {
        return Some(err(id, "missing cmd"));
    };
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let (reply_tx, reply_rx) = mpsc::channel();
    match tx.try_send(CtrlRequest { id: id.clone(), cmd: cmd.into(), params, reply: reply_tx }) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => return Some(err(id, "too many requests in flight — retry")),
        // Nobody will ever poll again — close, don't hang the client.
        Err(TrySendError::Disconnected(_)) => return None,
    }
    if let Some(c) = ctx.get() {
        c.request_repaint();
    }
    // Block until `poll` answers; a dropped sender means the app is
    // gone, and there is no reply worth sending after that.
    reply_rx.recv().ok()
}

fn reply(w: &mut impl Write, v: &Value) -> io::Result<()> {
    writeln!(w, "{v}").and_then(|()| w.flush())
}

fn err(id: Value, msg: &str) -> Value {
    json!({"id": id, "ok": false, "error": msg})
}

/// What [`read_line_limited`] found.
#[derive(Debug, PartialEq, Eq)]
enum LineRead {
    /// A line, now in the buffer (newline and a trailing `\r` stripped).
    Line,
    /// End of stream.
    End,
    /// The line is longer than the limit; at most limit + 1 bytes of it were read.
    TooLong,
}

/// Read one `\n`-terminated line of at most `max` bytes into `buf`.
fn read_line_limited(reader: &mut impl BufRead, buf: &mut Vec<u8>, max: usize) -> io::Result<LineRead> {
    use std::io::Read;
    buf.clear();
    // `take` bounds what is buffered for one line: never more than `max` + 1 bytes.
    let limit = u64::try_from(max).unwrap_or(u64::MAX).saturating_add(1);
    if reader.take(limit).read_until(b'\n', buf)? == 0 {
        return Ok(LineRead::End);
    }
    if buf.last() == Some(&b'\n') {
        buf.pop();
        if buf.last() == Some(&b'\r') {
            buf.pop();
        }
        return Ok(LineRead::Line);
    }
    // No newline: either the limit was passed, or this is a last line before end of stream.
    Ok(if buf.len() > max { LineRead::TooLong } else { LineRead::Line })
}

/// Byte-wise compare — the token check shouldn't leak its length or a
/// matching prefix through timing on a local socket.
fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// A 128-bit token, hex-encoded.
///
/// `RandomState` keys are drawn by std from the operating system's
/// random source (it needs them for HashMap DoS resistance); each
/// `RandomState::new()` re-keys. Mixing the keyed hash with the clock,
/// pid and a stack address gives an unguessable loopback bearer token
/// without a crypto dependency. If a CSPRNG ever lands in the
/// dependency tree, switch to it.
#[allow(clippy::disallowed_methods)] // Instant::now — this module is native-only
fn random_token() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::BuildHasher;
    let marker = 0u8;
    let mut hex = String::with_capacity(32);
    for i in 0u64..2 {
        let bits = RandomState::new().hash_one((std::time::Instant::now(), std::process::id(), &marker as *const u8 as usize, i));
        hex.push_str(&format!("{bits:016x}"));
    }
    hex
}

/// Write the control endpoint so that only the current user can read
/// the token.
///
/// The JSON goes to a new file next to `path`, created fresh (owner-only
/// on Unix), which then replaces `path`. Opening `path` itself would
/// follow a link planted there and truncate whatever it points at — and
/// `craft_atomic::atomic_write` deliberately resolves links (a save
/// keeps the link and rewrites its target), which is the wrong semantic
/// for a token file at a user-chosen path. A rename replaces the link
/// and leaves its target alone; a reader polling for the file also
/// never sees it half-written.
fn write_control_file(path: &Path, port: u16, token: &str) -> io::Result<()> {
    let json =
        serde_json::to_string_pretty(&json!({ "port": port, "token": token, "pid": std::process::id() })).unwrap_or_else(|_| String::from("{}"));
    let name = path.file_name().ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "not a file name"))?;
    // Unpredictable suffixes, so the name can't be planted in advance.
    // `RandomState` is keyed from the operating system's random source.
    let random = std::collections::hash_map::RandomState::new();
    for i in 0..16u32 {
        let mut staged = std::ffi::OsString::from(".");
        staged.push(name);
        staged.push(format!(".{:016x}.tmp", std::hash::BuildHasher::hash_one(&random, i)));
        let staged = path.with_file_name(staged);
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let file = match opts.open(&staged) {
            Ok(f) => f,
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            // Windows refuses `create_new` on a folder with "access denied"; the name is taken all
            // the same. A folder we can't write to, with nothing at the name, still fails here.
            Err(e) if e.kind() == ErrorKind::PermissionDenied && staged.symlink_metadata().is_ok() => continue,
            Err(e) => return Err(e),
        };
        // The block closes the file before it is renamed.
        let written = {
            let mut file = file;
            file.write_all(json.as_bytes())
        };
        let done = written.and_then(|()| std::fs::rename(&staged, path));
        if done.is_err() {
            let _ = std::fs::remove_file(&staged);
        }
        return done;
    }
    Err(io::Error::new(ErrorKind::AlreadyExists, "no free name for the control file's temporary copy"))
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)] // Instant::now for reply deadlines — native-only module
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// A started channel and its token — the app-side `Session` the
    /// tests drive with `poll`.
    fn start() -> (Control, String) {
        let c = Control::start_no_file().unwrap();
        let token = c.token.to_string();
        (c, token)
    }

    fn client(port: u16) -> (TcpStream, BufReader<TcpStream>) {
        let s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
        let w = s.try_clone().unwrap();
        (w, BufReader::new(s))
    }

    /// Write `line` + `\n`, then keep polling the session until one
    /// reply line arrives (or the deadline passes).
    fn roundtrip(control: &Control, session: &mut Session, w: &mut TcpStream, r: &mut BufReader<TcpStream>, line: &str) -> Value {
        writeln!(w, "{line}").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut text = String::new();
        loop {
            control.poll(session);
            match r.read_line(&mut text) {
                Ok(0) => panic!("connection closed without a reply"),
                Ok(_) => return serde_json::from_str(&text).unwrap(),
                Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => {
                    assert!(Instant::now() < deadline, "no reply within 5 s");
                    std::thread::yield_now();
                }
                Err(e) => panic!("read failed: {e}"),
            }
        }
    }

    /// A request that fails before queueing is answered by the
    /// connection thread — no `poll` needed.
    fn expect_reply(r: &mut BufReader<TcpStream>) -> Value {
        let mut text = String::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match r.read_line(&mut text) {
                Ok(0) => panic!("connection closed without a reply"),
                Ok(_) => return serde_json::from_str(&text).unwrap(),
                Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => {
                    assert!(Instant::now() < deadline, "no reply within 5 s");
                    std::thread::yield_now();
                }
                Err(e) => panic!("read failed: {e}"),
            }
        }
    }

    #[test]
    fn a_wrong_token_is_unauthorized_and_never_executes() {
        let (c, _token) = start();
        let mut session = Session::with_source("keep me".into());
        let (mut w, mut r) = client(c.port());
        // `source.set` would change the buffer if it ran — it must not.
        let v = roundtrip(&c, &mut session, &mut w, &mut r, r#"{"token":"nope","id":7,"cmd":"source.set","params":{"text":"hacked"}}"#);
        assert_eq!(v, json!({"id": 7, "ok": false, "error": "unauthorized"}));
        assert_eq!(session.source(), "keep me");
    }

    #[test]
    fn a_missing_token_is_unauthorized() {
        let (c, _token) = start();
        let mut session = Session::new();
        let (mut w, mut r) = client(c.port());
        let v = roundtrip(&c, &mut session, &mut w, &mut r, r#"{"id":1,"cmd":"source.get"}"#);
        assert_eq!(v, json!({"id": 1, "ok": false, "error": "unauthorized"}));
    }

    #[test]
    fn a_good_request_executes_and_echoes_the_id() {
        let (c, token) = start();
        let mut session = Session::with_source("hello tex".into());
        let (mut w, mut r) = client(c.port());
        let v = roundtrip(&c, &mut session, &mut w, &mut r, &format!(r#"{{"token":"{token}","id":"abc","cmd":"source.get","params":{{}}}}"#));
        assert_eq!(v, json!({"id": "abc", "ok": true, "result": {"text": "hello tex"}}));
    }

    #[test]
    fn a_missing_cmd_is_an_error() {
        let (c, token) = start();
        let mut session = Session::new();
        let (mut w, mut r) = client(c.port());
        let v = roundtrip(&c, &mut session, &mut w, &mut r, &format!(r#"{{"token":"{token}","id":2}}"#));
        assert_eq!(v, json!({"id": 2, "ok": false, "error": "missing cmd"}));
    }

    #[test]
    fn a_missing_id_is_an_error_no_notifications() {
        let (c, token) = start();
        let mut session = Session::new();
        let (mut w, mut r) = client(c.port());
        let v = roundtrip(&c, &mut session, &mut w, &mut r, &format!(r#"{{"token":"{token}","cmd":"source.get"}}"#));
        assert_eq!(v, json!({"id": null, "ok": false, "error": "missing id"}));
    }

    #[test]
    fn an_unknown_command_surfaces_the_engine_error() {
        let (c, token) = start();
        let mut session = Session::new();
        let (mut w, mut r) = client(c.port());
        let v = roundtrip(&c, &mut session, &mut w, &mut r, &format!(r#"{{"token":"{token}","id":3,"cmd":"no.such"}}"#));
        assert_eq!(v["id"], 3);
        assert_eq!(v["ok"], false);
        assert!(v["error"].as_str().unwrap().contains("unknown command"), "{v}");
    }

    #[test]
    fn malformed_json_gets_a_parse_error_not_a_close() {
        let (c, _token) = start();
        let (mut w, mut r) = client(c.port());
        writeln!(w, "not json").unwrap();
        let v = expect_reply(&mut r);
        assert_eq!(v["id"], Value::Null);
        assert_eq!(v["ok"], false);
        assert!(v["error"].as_str().unwrap().contains("parse error"), "{v}");
    }

    #[test]
    fn an_overlong_line_is_refused_and_closes() {
        let (c, _token) = start();
        let (mut w, mut r) = client(c.port());
        w.write_all(&vec![b'x'; MAX_LINE + 1]).unwrap();
        let v = expect_reply(&mut r);
        assert_eq!(v, json!({"id": null, "ok": false, "error": "request too large"}));
        // Then the connection closes — the unread rest of the line
        // can't be resynchronized.
        let deadline = Instant::now() + Duration::from_secs(5);
        let closed = loop {
            let mut next = String::new();
            match r.read_line(&mut next) {
                Ok(0) => break true,
                Ok(_) => panic!("unexpected extra reply: {next:?}"),
                Err(e) if matches!(e.kind(), ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted) => break true,
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    if Instant::now() >= deadline {
                        break false;
                    }
                    std::thread::yield_now();
                }
                Err(e) => panic!("read failed: {e}"),
            }
        };
        assert!(closed, "the connection stayed open");
    }

    #[test]
    fn several_requests_on_one_connection_are_served_in_order() {
        let (c, token) = start();
        let mut session = Session::new();
        let (mut w, mut r) = client(c.port());
        for i in 0..3 {
            let v = roundtrip(
                &c,
                &mut session,
                &mut w,
                &mut r,
                &format!(r#"{{"token":"{token}","id":{i},"cmd":"source.set","params":{{"text":"doc {i}"}}}}"#),
            );
            assert_eq!(v["ok"], true, "{v}");
            assert_eq!(v["id"], i);
        }
        assert_eq!(session.source(), "doc 2");
    }

    /// `start(path)` publishes the endpoint JSON owner-only and removes
    /// it on drop.
    #[test]
    fn the_control_file_is_published_then_removed() {
        let dir = std::env::temp_dir().join(format!("warraqcraft-control-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("ctl.json");
        let port;
        {
            let c = Control::start(&file).unwrap();
            port = c.port();
            let published: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
            assert_eq!(published["port"], port);
            assert_eq!(published["token"].as_str().unwrap().len(), 32);
            assert!(published["pid"].is_number());
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
            }
        }
        assert!(!file.exists(), "the control file is removed on drop");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A leftover control file is replaced, not written through — and a
    /// planted link's target is left alone.
    #[cfg(unix)]
    #[test]
    fn the_control_file_replaces_a_symlink_instead_of_writing_through_it() {
        use std::os::unix::fs::symlink;
        let dir = std::env::temp_dir().join(format!("warraqcraft-control-link-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("target.txt");
        std::fs::write(&target, b"original").unwrap();
        let link = dir.join("ctl.json");
        symlink(&target, &link).unwrap();
        let c = Control::start(&link).unwrap();
        assert!(!link.symlink_metadata().unwrap().file_type().is_symlink(), "the link was replaced");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "original", "the link target was not touched");
        drop(c);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The bounded line reader accepts a line exactly at the limit,
    /// strips CRLF, and reports what is left after a refusal.
    #[test]
    fn lines_are_read_up_to_the_limit() {
        let mut buf = Vec::new();
        let mut input: &[u8] = b"aaaaaaaaaa\nbb\r\ncc";
        assert_eq!(read_line_limited(&mut input, &mut buf, 10).unwrap(), LineRead::Line);
        assert_eq!(buf, b"aaaaaaaaaa", "a line exactly at the limit is accepted");
        assert_eq!(read_line_limited(&mut input, &mut buf, 10).unwrap(), LineRead::Line);
        assert_eq!(buf, b"bb", "CRLF is stripped");
        assert_eq!(read_line_limited(&mut input, &mut buf, 10).unwrap(), LineRead::Line);
        assert_eq!(buf, b"cc", "a last line without a newline is still read");
        assert_eq!(read_line_limited(&mut input, &mut buf, 10).unwrap(), LineRead::End, "then end of stream");
        let long = [b'x'; 100];
        let mut input: &[u8] = &long;
        assert_eq!(read_line_limited(&mut input, &mut buf, 10).unwrap(), LineRead::TooLong);
        assert_eq!(input.len(), 100 - 11, "at most limit + 1 bytes are read; the rest is left");
    }
}
