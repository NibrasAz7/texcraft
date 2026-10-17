# TexCraft roadmap

**Stage: M0 — scaffold and parsing core.** The crate sketch and product
description are in `README.md`. Open decisions (engine option B vs A′ —
A′ now means building on MIT-licensed `texlang`/`boxworks` from
jamespfennell/texcraft, bibliography backend, history backend, editor
widget) are tracked there — nothing in M0 depends on them.

## Milestones

| Id | Scope | Status |
| --- | --- | --- |
| M0 | Repo scaffold (workspace, lints, xtask, AGENTS) + `texparse` (.tex lexer, structure scan, .bib parser) + `texcraft-cli parse/bib/check` | done |
| M1 | Engine decision landed; `texpkg` curated package set; minimal compile pipeline (`document.compile`) | in progress — system-TeX + bundled tectonic (B-lite) done; `texpkg` open |
| M2 | `engine` command registry + Session; CLI `--cmd` surface | in progress — Session + execute() landed; `--cmd`/control next |
| M3 | `ui-egui` studio: project tree, editor, PDF preview, SyncTeX nav | in progress — IDE shell, editor, PDF preview and SyncTeX click-through done; project-wide awareness next |
| M4 | History (snapshots/diff/restore), reference-manager integration (Zotero/CSL), arabdoc + template gallery | not started |

## Progress log

- 2026-10-14 — product description written (`README.md`); repo scaffold +
  `texparse` started.
- M0.5 — minimal egui studio: editor, live outline/issues, open/save,
  theme toggle.
- M0.6 — `crates/engine` (Session + `execute()`, first commands) and a
  working PDF preview: system-TeX compile (`xelatex`/`lualatex`/`pdflatex`
  on PATH, `.texcraft/` build dir, no shell escape, bibtex pass when a
  bibliography exists), hayro rasterizes pages in the preview pane.
  `texcraft-cli compile` added. Repo published to `NibrasAz7/texcraft`.
- M0.7 — reconnaissance: observation-level REA/Ghidra survey of an
  installed TeXstudio (feature/subsystem inventory — BuildManager command
  constants, LatexDocument/LatexParser IDE model, QSynctex, BibTeXDialog;
  no implementation copied) and analysis of jamespfennell/texcraft (MIT),
  validated by a spike that embedded `texlang` via git dep and ran real
  TeX semantics (def/registers/ifs/csname/input, caret diagnostics; one
  gap: unbounded expansion stack → worker thread required). Plan updated:
  C live, B shipping, A′ = own typesetter on texlang/boxworks.
- M0.8 — LocalLeaf recon (MIT, github.com/leoaliperti/LocalLeaf — a
  shipped local-first Overleaf-style editor, `texcraft-core` Rust core +
  Tauri/Vue/CodeMirror/PDF.js frontend). Technique-level adoptions into
  `engine`: log-driven rerun loop (cross-refs resolve without a
  bibliography now), artifact-driven bibliography (`.bcf`→biber,
  `.aux \bibdata`→bibtex, `BIBINPUTS`/`BSTINPUTS`), and a `synctex`
  module wrapping the distro `synctex` CLI for source↔PDF jumps
  (verified on MiKTeX both directions). Their MCP surface
  (compile/read/write/list-engines) informs M2. Name note: "texcraft"
  is now taken twice (JF crate + LocalLeaf's `texcraft-core`).
- M3.1 — LocalLeaf-style IDE shell: dark palette, file-tree sidebar with
  main-doc star, guttered lexer-driven syntax highlighting, PDF preview
  toolbar (pager, zoom, fit), togglable outline + logs panels, status
  strip, engine picker on Recompile.
- M3.2 — SyncTeX click-through wired both directions (Ctrl+click source
  → PDF highlight + scroll; PDF click → caret, opens target file) with
  pt↔px mapping per page; opt-in auto-compile after 5 s idle (attempt-
  stamped, no failure hot-loop).
- M1.1 — B-lite: bundled `bin/tectonic.exe` (MIT, subprocess) detected
  before system TeX — zero-install compile incl. its internal
  bibtex/rerun loop; one `--keep-logs --synctex` invocation. Light/dark
  themes via `theme::Palette` + toolbar toggle.
- M0.9 — TeXstudio feature survey (REA/Ghidra, observational) →
  `docs/TEXSTUDIO-SURVEY.md` merge plan: magic comments, user tools,
  `.cwl`-style curated completion, folding, templates, log marks,
  inline math preview scheduled across M2–M4+.
- M3.3 — Painted sun/moon theme icon (pure Painter geometry, no font
  glyphs) replacing the text toggle.
- M1.2 — craft-libs adoption: `craft-atomic` crash-safe saves (tmp→
  fsync→rename) in Session::save + new-file dialog; `craft-quit`
  platform-correct quit hook; compile/tree polling moved into eframe
  `logic` so minimized windows keep working. Pinned rev `457a078`.
- M2/M3 Wave-1 — TeXstudio surface, clean-room (all four streams
  parallel-built, disjoint crates):
  * texparse `magic.rs` — `% !TeX program/root/encoding/bibliography/
    spellcheck/jobname` + `% !BIB program` header directives,
    `engine_hint()` normalization; engine consumes program→engine
    pick, root→master-file compile, BIB→bib-tool override; `magic.get`.
  * `texpkg` (new L3 crate) — curated 271-entry command/environment/
    package catalog (own data, not copied `.cwl`).
  * engine `tools.rs` — `user_tool.set/list/run/clear`: fixed-argv
    spawn (no shell), `{file}/{dir}/{stem}/{basename}` placeholders,
    60 s timeout + kill, 256 KiB drained output cap. UI loads
    `texcraft-tools.json` per project; demo file in examples/.
  * ui-egui — full block folding (heading/env regions, derived folded
    buffer, source-coordinate remapping); line ops Ctrl+D/Alt+↑↓/
    Ctrl+//Ctrl+Shift+K/Ctrl+Shift+Bksp; compile errors→gutter dots+
    tooltips; completion popup (\cmd ≥2 chars, \begin/\end envs, ↑↓
    Enter/Tab Esc); magic directives in status bar.
