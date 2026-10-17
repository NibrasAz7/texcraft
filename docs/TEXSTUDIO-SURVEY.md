# TeXstudio feature surface — observational survey and merge plan

Source: REA/Ghidra observation of the installed binary
`C:\Program Files\texstudio\texstudio.exe` (x86_64 PE, SHA-256
`4aec9aa9b5249697beecd2d215f1ab142f1819da2cd0395cf7019a79fa893c4e`),
plus its shipped `templates/` and `help/` directories. Evidence IDs from
the REA session: `ev_7477470f…`, `ev_707429bc…`, `ev_d4e2c9f8…`,
`ev_1bdf7dfd…`, `ev_ece41e4a…`.

**Method and boundary.** Strings, menu structures, settings keys and
symbol names were read statically; no GPL code or assets were copied.
Menu/tool concepts and observed *behaviour descriptions* inform the
gap list below; implementation is original. String-derived inventory
does not prove every runtime path.

## Observed feature surface

### Build & view

- Toolbar-level Build actions: *Compile & View*, *Build & View*,
  *Compile*, *View*, plus dedicated *Bibliography*, *Glossary*, *Index*.
- Engine matrix: latexmk, LaTeX, PDFLaTeX, XeLaTeX, LuaLaTeX, DVI→PS→PDF
  chain, MakeIndex, texindy, Xindex, BibTeX, BibTeX 8-bit, Biber,
  makeglossaries, MetaPost, Asymptote.
- User-defined tools (`updateUserToolMenu`, `removeDuplicateUserTools`)
  — arbitrary commands wired into menus.
- Log panel with click-to-source marks (`lineMarkToolTip`,
  `showMarkTooltipForLogMessage`).

### Editor

- QNFA grammar-based highlighting (embedded XML grammar —
  `(La)TeX` syntax plus a macro-layer language with `%<placeholders%>`).
- Folding (`foldBlockAt`, *Collapse/Expand Block*, *Nearest Block*),
  block navigation (next/previous block, select-to-block).
- Multi-cursor "mirrors" (`addCursorMirror`, `editAlignMirrors`,
  `generateMirror`, next/previous occurrence with mirrors).
- Line ops: move/duplicate/sort, hard line breaks, indent/unindent,
  comment/uncomment, erase word/command/environment.
- *Idefix* menu, paste/convert-to-LaTeX, *Analyse Text*, random text.
- Preview: selection/parentheses preview and **inline math preview**
  (render formulas as tooltip or inline — `PM_TOOLTIP`,
  `PM_TOOLTIP_AS_FALLBACK` modes).

### Completion & tooltips

- `.cwl` completion catalog embedded in the binary — per-package command
  lists with arg specs (`#keyvals`, `#include` resolution) driving
  context-aware completion of commands/environments/options.
- Tooltip help on commands, tooltip preview on formulas, image tooltip
  on `\includegraphics` paths, label/bibitem target-text tooltip,
  completer-description tooltip.

### Project & file management

- Explicit root document via **magic comments**: `% !TeX program`,
  `root`, `encoding`, `bibliography`, `spellcheck`, plus command
  definitions interpreted in magic comments.
- Multi-file project tree, root detection, file ops (new-from-template,
  rename/move/delete in-project, revert, permissions, copy filename).
- Templates: shipped `templates/` dir with article/Beamer/book/letter/
  ModernCV/report/KOMA/French variants — JSON + `.tex` + preview PNG.
- Version control: SVN/Git integration, revision compare,
  check-in/lock-on-PDF.
- Sessions/state: window layout, recent files.

### Wizards

Quick start, letter, Beamer, table/tabbing/array, graphics insertion,
bibliography insertion, math assistant, AI chat.

### Viewer

Internal PDF viewer (QSynctex): magnify, select text, page nav, zoom
modes, continuous mode, split view, grid (tile/stack/side-by-side),
SyncTeX both directions, go-to-source.

### Checking

Spell-check (Hunspell dictionaries, per-language, magic-comment
controlled), grammar via LanguageTool integration, word/line stats.

### Automation

Macros: user tags, JS/QtScript macros with `browseMacrosOnRepository`,
import/export. Command-line args incl. `--insert-cite` (Zotero push),
`--start-always`, `--auto-activate-hidden` etc. Update checker.

## Merge plan — mapped to WarraqCraft milestones

Prioritized by value ÷ effort for the current stack. Existing coverage
is omitted (editor/tree/preview/SyncTeX/log navigation already exist).

### Wave 1 — M2–M3 (command surface & editor depth)

| Feature | Notes |
| --- | --- |
| Magic comments | `texparse` pass over leading `% !TeX` lines → engine/root/encoding/bibliography/spellcheck override. Cheap, high value for multi-file docs. |
| User tools | `engine` command that runs a user-defined external tool per project (name + argv + cwd); surfaced in the Recompile chevron + control channel. Never a shell escape — fixed argv only. |
| `.cwl`-style completion | Completion catalog is *data*, not code: we can author our own curated `packages.toml`/`commands.toml` in `texpkg` for the core LaTeX set (document-structure, amsmath, graphicx, hyperref, bibtex/cite). Original content, CTAN-docs-derived. |
| Foldable structure | Outline already knows section ranges → editor block folding is a `ui-egui` feature on existing structure data. |
| Better line ops | duplicate/move/sort lines, comment/uncomment, erase-word/command/environment — pure editor commands, testable. |

### Wave 2 — M4 (documents, references, polish)

| Feature | Notes |
| --- | --- |
| Templates | Gallery format: `.tex` + `meta.toml` (+ optional raster preview we generate ourselves). Shipped curated set: article, report, letter, beamer, arabdoc. "New from template" in the file-tree header. |
| Root-document detection | Already probed via `\documentclass`; extend to project graph (`\input`/`\include`) in M3→M4. |
| Bibliography UI | Insert-cite dialog over the `.bib` index; bibliography tool button already routes through the rerun loop. |
| Log marks → gutter | Parse error/warning lines → gutter marks + tooltips (we have `CompileReport::issues`; needs line-accurate mapping). |
| History | `project.snapshot`/restore — our differentiator; supersedes their SVN integration. Git integration can be a user tool. |

### Wave 3 — later milestones

| Feature | Notes |
| --- | --- |
| Inline math preview | Render selected `$…$` region to a small PDF via the engine, show as tooltip/popup. Real differentiator; needs fast incremental compile — ties to A′. |
| Spell-check | `hunspell-rs`/`nuspell`-class dep (LGPL risk — prefer `symphonia`-style pure-Rust or a custom dictionary pass); per-language magic comment already designed above. |
| Word/line stats ("Analyse Text") | `texparse` already tokenizes — counts are trivial. |
| Cursor mirrors / multi-cursor | egui TextEdit doesn't support multiple carets; needs the custom editor milestone. Defer. |
| Source→HTML export | `texparse` tokens → highlighted HTML; easy standalone command. |
| DVI/PS chain, MetaPost/Asymptote | System-TeX track only; expose as named user tools, not built-ins. |
| Multi-window/split viewers | M5+ UX work. |
| Update checker | Family infrastructure (ArtCraft Toolbox already handles updates). |
| AI chat wizard | Deliberately out of scope for the local-first brief (documented decision). |

### Declined (won't merge)

- **Sweave/Pweave/Asymptote embedded blocks** — niche literate-programming
  support; revisit if templates demand it.
- **QtScript user macros** — scripting surface is our `--cmd`/TCP/MCP
  command channel instead; a JS engine conflicts with the Rust-only rule.
- **MakeIndex/texindy/xindex built-ins** — remain external tools via
  system-TeX; `texpkg` index pass only if a built-in index engine lands.
