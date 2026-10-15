# TexCraft roadmap

**Stage: M0 — scaffold and parsing core.** The crate sketch and product
description are in `README.md`. Open decisions (engine option B vs A′ —
A′ now means building on MIT-licensed `texlang`/`boxworks` from
jamespfennell/texcraft, bibliography backend, history backend, editor
widget) are tracked there — nothing in M0 depends on them.

## Milestones

| Id | Scope | Status |
| --- | --- | --- |
| M0 | Repo scaffold (workspace, lints, xtask, AGENTS) + `texparse` (.tex lexer, structure scan, .bib parser) + `texcraft-cli parse/bib/check` | in progress |
| M1 | Engine decision landed; `texpkg` curated package set; minimal compile pipeline (`document.compile`) | in progress — system-TeX pipeline done, Tectonic + texpkg open |
| M2 | `engine` command registry + Session; CLI `--cmd` surface | in progress — Session + execute() landed; `--cmd`/control next |
| M3 | `ui-egui` studio: project tree, editor, PDF preview, SyncTeX nav | in progress — editor + PDF preview done; project tree and SyncTeX next |
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
