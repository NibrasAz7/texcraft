//! TexCraft parsing core (L0).
//!
//! Three independent, allocation-capped, never-panicking readers for
//! untrusted input:
//!
//! - [`lex`] — a `.tex` tokenizer producing positioned tokens (commands,
//!   braces, math shifts, comments). TeX is Turing-complete, so this is a
//!   *surface* lexer in the default catcode regime — the same level real
//!   editors use for highlighting and structure.
//! - [`structure`] — a structural scan over the token stream: document
//!   class, packages, the heading tree, environments, labels, citations,
//!   references, included files and unmatched-brace/environment issues.
//! - [`bib`] — a `.bib` (BibTeX) file parser: entries, fields, `@string`
//!   expansion, `@preamble`/`@comment`, with issues collected instead of
//!   hard failures.
//! - [`magic`] — editor "magic comment" header directives
//!   (`% !TeX program = xelatex`, `% !BIB program = biber`): build-engine,
//!   root, encoding, spellcheck and jobname hints read from the leading
//!   comment block the way TeXstudio/TeXShop honor them.
//! - [`graph`] — project graph: file-level dependency extraction
//!   (`\input`, `\include`, graphics, bibliography, class, packages),
//!   on-disk resolution, and a cycle-safe BFS from an entry file into
//!   roots, reachable sources, resources and missing references.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod bib;
pub mod graph;
mod lex;
mod magic;
mod structure;

pub use bib::{parse_bib, BibEntry, BibFile};
pub use graph::{build, declares_document_class, extract_deps, resolve, Dep, DepKind, ProjectGraph};
pub use lex::{lex, Pos, Tok, Token};
pub use magic::{parse as parse_magic, Magic};
pub use structure::{parse_structure, DocStructure, Env, Heading, Issue};

/// Hard caps so hostile input can't exhaust memory or stack.
pub(crate) mod caps {
    /// Longest command/identifier name kept; longer names are truncated.
    pub const MAX_NAME: usize = 256;
    /// Longest captured argument text (title, filename, bib value).
    pub const MAX_ARG: usize = 64 * 1024;
    /// Deepest brace/environment nesting tracked; deeper input is
    /// reported as an issue and skipped.
    pub const MAX_DEPTH: usize = 512;
}
