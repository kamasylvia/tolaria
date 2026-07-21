---
type: ADR
id: "0171"
title: "Typst note preview via in-process typst crate"
status: active
date: 2026-07-17
---

## Context

Tolaria vaults already hold Markdown, plain text, sheets, images, PDFs, and standalone HTML. Authors who write technical, scientific, or layout-rich content increasingly reach for [Typst](https://typst.app), a modern typesetting system whose source files (`.typ`) currently fall through Tolaria's file classification to `"binary"` and open in the system viewer, breaking the in-app read/edit loop.

A Typst note may be a single self-contained file or a small multi-file project that imports siblings via `#import "lib.typ"` and `#include "chapter.typ"` relative paths. A useful preview must therefore (a) render inline in the editor pane like the HTML preview (ADR-0168), (b) re-render on save, and (c) honor a chosen entry file so `#import` resolution anchors at the project root rather than at whichever file happens to be open.

## Decision

**Tolaria compiles Typst notes to SVG inside `src-tauri` via the `typst` crate and renders the returned SVG inline in the editor pane.** The frontend invokes a new `render_typst` Tauri command, receives a merged-page SVG string, sanitizes it defensively with `DOMPurify`, and injects it into the main webview. The `typst::World` trait implementation anchors file resolution at a chosen root directory and exposes the entry file through `main()`, so `#import` and `#include` resolve relative to that root.

Typst files are classified as a new `fileKind: "typst"` (alongside `"markdown"`, `"text"`, `"binary"`) so the editor dispatch routes them to a dedicated `TypstPreview` component instead of the raw CodeMirror editor or the binary `FilePreview` fallback. The breadcrumb source toggle and `Cmd/Ctrl+\` switch between preview and raw Typst source, mirroring the HTML preview contract from ADR-0168.

Entry-file resolution follows four layers, in priority order: an explicit `main_path` parameter (the future "Pin entry file" affordance), an auto-detected `main.typ` sibling, then the open file itself as a single-file document. A frontmatter `typst_root` hint is reserved for a later iteration.

## Alternatives considered

- **tinymist as a sidecar binary:** the most faithful "live preview like VS Code" experience, but ships a ~20-30 MB native binary per OS/arch, runs a local HTTP+WebSocket server, requires a CSP change for `ws://127.0.0.1:23635`, and brings the full LSP surface (completion, diagnostics, jump-to-definition) that a notes app does not need.
- **typst.ts WASM in the React frontend:** keeps compilation in the main webview and offers a ready React renderer, but ships a ~5-10 MB WASM blob into the JS bundle and conflicts with the project's "keep the app lean" constraint. The `mapShadow` virtual filesystem would also duplicate the file-resolution logic that the Rust `World` trait already gives us natively.
- **Leave `.typ` as `"binary"` and detect purely by extension (ADR-0168 mirror):** minimizes Rust changes but loses a meaningful file kind that future features (search indexing, type-aware actions) would want, and leaves the binary `FilePreview` fallback one bug away from surfacing for `.typ` notes.

## Consequences

- Typst notes gain inline preview and raw-source editing without leaving Tolaria.
- The app binary grows by the `typst` + `typst-svg` + `typst-assets` compile time and linked code size, but the JS bundle stays unchanged; no WASM payload ships to the webview.
- First-touch compilation of a Typst note crosses the Tauri IPC boundary (sub-second for small documents); subsequent saves reuse the in-process compiler instance.
- `#import` and `#include` resolve against the `World` root, so multi-file projects work when the entry file is pinned or auto-detected; single-file notes compile standalone.
- Compilation errors surface as an inline error panel with the diagnostic text, leaving the raw source accessible through the source toggle.
- The SVG output is trusted compiler markup but is still passed through `DOMPurify` defensively; no `allow-scripts`/`allow-same-origin` sandbox attribute is needed because the SVG is injected directly into the existing editor DOM behind Tolaria's CSP.
