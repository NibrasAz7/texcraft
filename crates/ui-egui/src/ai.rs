//! AI assist panel — bring-your-own-provider chat over the current
//! document, backed by craft-ai's `OpenAiCompat` (any
//! `/v1/chat/completions` endpoint: Ollama, LM Studio, OpenAI, …).
//!
//! All state is plain data: `lib.rs` renders it in an `egui::Window`,
//! persists the provider fields via craft-settings, and calls
//! [`AiPanel::poll`] every frame. Requests run on a worker thread so a
//! slow endpoint never stalls the UI; failures land in `response` /
//! `status`, never a panic. The web build has no provider stack — a
//! same-shaped stub keeps the call sites unconditional.

#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc::{channel, Receiver, Sender};

#[cfg(not(target_arch = "wasm32"))]
use craft_ai::Provider;

/// Context bytes prepended to a prompt — the selection/document rides
/// in a fenced block; 24 KiB keeps small local models sane.
const MAX_CONTEXT: usize = 24 * 1024;

/// A finished worker job: an answer for [`AiPanel::ask`] or a probe
/// label for [`AiPanel::probe`].
#[cfg(not(target_arch = "wasm32"))]
enum AiReply {
    Answer(Result<String, String>),
    Probe(Result<String, String>),
}

/// The AI assist panel: provider fields (persisted by the parent via
/// craft-settings), a prompt box, a response, and the worker channel.
/// All state is plain data — the parent renders it in an `egui::Window`.
#[cfg(not(target_arch = "wasm32"))]
pub struct AiPanel {
    pub open: bool,
    /// Base URL of an OpenAI-compatible endpoint, e.g.
    /// `http://localhost:11434/v1`.
    pub endpoint: String,
    /// Optional bearer token — never echoed into errors or status.
    pub token: String,
    pub model: String,
    pub prompt: String,
    pub response: String,
    /// A request is in flight — `ask`/`probe` no-op while set.
    pub busy: bool,
    /// Whether the last `probe` succeeded, plus its label.
    pub status: Option<(bool, String)>,
    tx: Sender<AiReply>,
    rx: Receiver<AiReply>,
}

#[cfg(not(target_arch = "wasm32"))]
impl AiPanel {
    pub fn new() -> Self {
        let (tx, rx) = channel();
        Self {
            open: false,
            endpoint: "http://localhost:11434/v1".into(),
            token: String::new(),
            model: String::new(),
            prompt: String::new(),
            response: String::new(),
            busy: false,
            status: None,
            tx,
            rx,
        }
    }

    /// Send `context` + `prompt` to the provider on a worker thread —
    /// the context is prepended as a fenced block the model can
    /// reference. No-ops while a request is in flight; an empty
    /// endpoint short-circuits without spawning.
    pub fn ask(&mut self, context: &str) {
        if self.busy {
            return;
        }
        if self.endpoint.trim().is_empty() {
            self.response = "configure an endpoint first".into();
            return;
        }
        self.busy = true;
        let prompt = Self::build_prompt(context, &self.prompt);
        let (endpoint, token, model) = (self.endpoint.clone(), self.token.clone(), self.model.clone());
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let mut provider = provider(endpoint, token, model);
            let out = provider.complete(&prompt, &craft_ai::GenOpts::default()).map_err(|e| e.to_string());
            let _ = tx.send(AiReply::Answer(out));
        });
    }

    /// Probe the endpoint's `/v1/models` handshake on a worker →
    /// `status`. Sends no document text.
    pub fn probe(&mut self) {
        if self.busy {
            return;
        }
        if self.endpoint.trim().is_empty() {
            self.status = Some((false, "configure an endpoint first".into()));
            return;
        }
        self.busy = true;
        let (endpoint, token, model) = (self.endpoint.clone(), self.token.clone(), self.model.clone());
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let mut provider = provider(endpoint, token, model);
            let out = provider.test().map(|info| info.label).map_err(|e| e.to_string());
            let _ = tx.send(AiReply::Probe(out));
        });
    }

    /// Drain finished replies — call every frame.
    pub fn poll(&mut self) {
        while let Ok(reply) = self.rx.try_recv() {
            self.busy = false;
            match reply {
                AiReply::Answer(Ok(text)) => self.response = text,
                AiReply::Answer(Err(e)) => self.response = format!("AI: {e}"),
                AiReply::Probe(Ok(label)) => self.status = Some((true, label)),
                AiReply::Probe(Err(e)) => self.status = Some((false, e)),
            }
        }
    }

    /// The assembled outbound prompt (context block + question) —
    /// exposed for tests/debugging.
    pub fn build_prompt(context: &str, question: &str) -> String {
        assemble_prompt(context, question)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for AiPanel {
    fn default() -> Self {
        Self::new()
    }
}

/// The provider for the current fields — an empty bearer token means
/// no `Authorization` header (unauthenticated local endpoints).
#[cfg(not(target_arch = "wasm32"))]
fn provider(endpoint: String, token: String, model: String) -> craft_ai::OpenAiCompat {
    craft_ai::OpenAiCompat::new(endpoint, (!token.is_empty()).then_some(token), model)
}

/// The AI assist panel, wasm stub: same fields, no provider stack —
/// `ask`/`probe`/`poll` are no-ops so the call sites stay unconditional.
#[cfg(target_arch = "wasm32")]
pub struct AiPanel {
    pub open: bool,
    pub endpoint: String,
    pub token: String,
    pub model: String,
    pub prompt: String,
    pub response: String,
    pub busy: bool,
    pub status: Option<(bool, String)>,
}

#[cfg(target_arch = "wasm32")]
impl AiPanel {
    pub fn new() -> Self {
        Self {
            open: false,
            endpoint: "http://localhost:11434/v1".into(),
            token: String::new(),
            model: String::new(),
            prompt: String::new(),
            response: String::new(),
            busy: false,
            status: None,
        }
    }

    /// No provider stack on wasm — requests never leave the tab.
    pub fn ask(&mut self, _context: &str) {}

    pub fn probe(&mut self) {}

    pub fn poll(&mut self) {}

    /// The assembled outbound prompt (context block + question) —
    /// exposed for tests/debugging.
    pub fn build_prompt(context: &str, question: &str) -> String {
        assemble_prompt(context, question)
    }
}

#[cfg(target_arch = "wasm32")]
impl Default for AiPanel {
    fn default() -> Self {
        Self::new()
    }
}

/// The outbound prompt: `context` (clipped to [`MAX_CONTEXT`] bytes on
/// a char boundary) in a fenced `tex` block, then the question.
fn assemble_prompt(context: &str, question: &str) -> String {
    format!("Context (LaTeX):\n```tex\n{}\n```\n\n{}", clip_context(context), question)
}

/// `context` cut to at most [`MAX_CONTEXT`] bytes without splitting a
/// char.
fn clip_context(context: &str) -> &str {
    if context.len() <= MAX_CONTEXT {
        return context;
    }
    let mut end = MAX_CONTEXT;
    while end > 0 && !context.is_char_boundary(end) {
        end -= 1;
    }
    context.get(..end).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_prompt_shape() {
        let p = AiPanel::build_prompt("x = 1", "what does this do?");
        assert_eq!(p, "Context (LaTeX):\n```tex\nx = 1\n```\n\nwhat does this do?");
    }

    #[test]
    fn build_prompt_context_capped() {
        let big = "a".repeat(MAX_CONTEXT + 4096);
        let p = AiPanel::build_prompt(&big, "q");
        let wire = "Context (LaTeX):\n```tex\n\n```\n\nq".len() + MAX_CONTEXT;
        assert!(p.len() <= wire, "prompt {} bytes > cap {}", p.len(), wire);
        // A multi-byte char straddling the cap is cut on a boundary.
        let mut s = "a".repeat(MAX_CONTEXT - 1);
        s.push('汉'); // 3 bytes — the boundary lands before it
        let p = AiPanel::build_prompt(&s, "q");
        assert!(p.contains(&"a".repeat(MAX_CONTEXT - 1)));
        assert!(!p.contains('汉'));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn ask_without_endpoint_is_fast() {
        let mut panel = AiPanel::new();
        panel.endpoint.clear();
        panel.prompt = "hi".into();
        panel.ask("ctx");
        assert!(!panel.busy);
        assert!(panel.response.contains("configure an endpoint first"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn ask_while_busy_noops() {
        let mut panel = AiPanel::new();
        panel.busy = true;
        panel.ask("ctx");
        assert!(panel.response.is_empty());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn probe_without_endpoint_sets_status() {
        let mut panel = AiPanel::new();
        panel.endpoint.clear();
        panel.probe();
        assert!(!panel.busy);
        assert_eq!(panel.status, Some((false, "configure an endpoint first".to_string())));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn poll_collects_replies() {
        let mut panel = AiPanel::new();
        panel.busy = true;
        let _ = panel.tx.send(AiReply::Answer(Ok("answer!".into())));
        panel.poll();
        assert_eq!(panel.response, "answer!");
        assert!(!panel.busy);

        let _ = panel.tx.send(AiReply::Answer(Err("boom".into())));
        panel.poll();
        assert_eq!(panel.response, "AI: boom");

        let _ = panel.tx.send(AiReply::Probe(Ok("reachable: http://x".into())));
        panel.poll();
        assert_eq!(panel.status, Some((true, "reachable: http://x".to_string())));
    }
}
