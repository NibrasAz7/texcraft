# TexCraft — instructions for agents

TexCraft is a clean-room, open-source, Rust-native LaTeX authoring and
typesetting environment — Overleaf-class UX on the desktop, local-first.
It is the LaTeX-class member of the ArtCraft ecosystem and will run natively
on macOS, Windows, Linux and BSD, and on the web via WASM. Siblings with the
same conventions: `../matcraft` (numerical computing), `../wordcraft`
(word processor), `../gridcraft` (spreadsheet), `../pdfcraft` (Acrobat).
Shared rules and learnings: `../../craftrules` (`storytold/craftrules`).

The product description and open decisions live in [`README.md`](README.md).

## Start every session here

1. Read `ROADMAP.md` (stage, next task) and `plan/STATUS.md` if it exists.
2. Read `README.md` for scope and the open engine/bibliography decisions.

## Clean room

- Behaviour and semantics come from **public sources only**: The TeXbook's
  published algorithms (Knuth–Plass line breaking is published; appendix G
  math layout is published), the LaTeX2e/3 public documentation, CTAN
  package docs, and files users send us that they own.
- **Never copy GPL/LGPL code.** pdfTeX, LuaTeX, BibTeX, biber, kpathsea and
  GNU TeXmacs stay banned from this repo. Tectonic's MIT-licensed core is
  the one allowed engine dependency (decision B in README.md); MiKTeX/
  TeX Live may only be invoked as an optional external "system TeX".
- Never copy assets, icons, screenshots or wording from TeXstudio, Overleaf,
  TeXmaker or any other TeX editor beyond feature names.
- `\write18`-style shell escape is **always disabled**, regardless of input.
- Name rule: TexCraft is "LaTeX-compatible", never claims to be TeX.

## Never crash

LaTeX input is famously adversarial. **This outranks feature work.**

- **No panics in non-test code:** no `unwrap()`, `expect()`, `panic!`,
  `unreachable!`, `todo!`, `unimplemented!`; no `unsafe` (`forbid`).
- **Errors are `Result<T, E>`** through the crate's error type and `?`.
- **Input-derived data is hostile:** `.tex`/`.bib` files are untrusted —
  `get()` instead of `[i]`, cap input-sized allocations, bound recursion
  and nesting depth, never slice strings off char boundaries.
- Crate roots carry `#![deny(clippy::unwrap_used, clippy::expect_used,
  clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`.
  Tests may `unwrap` (`clippy.toml`).

## Assets

- Every non-code file is original, public domain / CC0, or open source /
  Creative Commons that allows redistribution. Fonts go to
  `storytold/craft-fonts` (`CRAFT_FONTS_DIR`), never this repo.

## Architecture (mirrors MatCraft's layering; see README.md crate sketch)

| Layer | Crate | Job |
| --- | --- | --- |
| L0 | `texparse` | .tex lexer/structure scan, .bib parser, SyncTeX data |
| L1 | `typeset` | boxes/glue, Knuth–Plass line breaking, math layout |
| L2 | `texpdf`, `texbib` | PDF writer; BibTeX/CSL-JSON/RIS import & render |
| L3 | `texpkg` | package/class registry + loader (arabdoc built in) |
| L5 | `engine` | command registry + Session guard |
| L6 | `mcp`, `ui-egui` | JSON-RPC server; egui studio UI |
| L7 | `apps/*` | `texcraft-cli`, `texcraft` studio + control server |

- **Everything is a command** once `engine` lands: `document.compile`,
  `cite.insert`, `figure.insert`, `project.snapshot`, `history.diff` —
  reachable identically from CLI, the TCP control channel and MCP.
- **Layering** is enforced by `cargo xtask layers`. Nothing below L6
  depends on egui/eframe/winit/rfd. The UI is thin; colours come from
  `theme::Tokens`.
- **Rust only** (no handwritten JS/TS). **Never break wasm.**

## Quality gates

Before every commit: `cargo xtask ci` (fmt, clippy -D warnings, tests,
layers). One task id per commit (`M0.1: .tex lexer`).

## Roadmap

`ROADMAP.md` is the one-page summary (stage, progress log). Detail lives in
`README.md` (product description, open questions) and `docs/` as it lands.
