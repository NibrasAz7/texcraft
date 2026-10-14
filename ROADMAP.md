# TexCraft roadmap

**Stage: M0 — scaffold and parsing core.** The crate sketch and product
description are in `README.md`. Open decisions (engine option B vs A,
bibliography backend, history backend, editor widget) are tracked there —
nothing in M0 depends on them.

## Milestones

| Id | Scope | Status |
|---|---|---|
| M0 | Repo scaffold (workspace, lints, xtask, AGENTS) + `texparse` (.tex lexer, structure scan, .bib parser) + `texcraft-cli parse/bib/check` | in progress |
| M1 | Engine decision landed; `texpkg` curated package set; minimal compile pipeline (`document.compile`) | not started |
| M2 | `engine` command registry + Session; CLI `--cmd` surface | not started |
| M3 | `ui-egui` studio: project tree, editor, PDF preview, SyncTeX nav | not started |
| M4 | History (snapshots/diff/restore), reference-manager integration (Zotero/CSL), arabdoc + template gallery | not started |

## Progress log

- 2026-10-14 — product description written (`README.md`); repo scaffold +
  `texparse` started.
