//! Typst note preview — compiles `.typ` source to SVG inline in the editor pane.
//!
//! See ADR-0171 for the design rationale. The compilation pipeline delegates to
//! [`tinymist_world`]: its `CompileOnceArgs::resolve_system()` builds a
//! `typst::World` with system font search (fontdb, with mmap mappings released
//! once each face's `FontInfo` is computed) and an HTTP package registry for
//! `@preview`/`@local` packages. We then run `typst::compile` and render the
//! resulting paged document as a single merged SVG string.

use std::path::PathBuf;

use tinymist_world::args::CompileOnceArgs;
use typst::diag::{EcoVec, SourceDiagnostic};
use typst::layout::Abs;
use typst_layout::PagedDocument;
use typst_svg::{svg_merged, SvgOptions};

/// Render a Typst note (and any `#import`/`#include` siblings, plus
/// `@preview`/`@local` packages) to a single merged-page SVG string.
///
/// Frontend invokes this as `invoke('render_typst', { path, vaultPath,
/// mainPath })`. The returned SVG is safe to inject into the editor DOM after
/// a defensive DOMPurify pass client-side (see ADR-0171).
#[tauri::command]
pub fn render_typst(
    path: PathBuf,
    vault_path: Option<PathBuf>,
    main_path: Option<PathBuf>,
) -> Result<String, String> {
    // Entry anchor: an explicit `main_path` wins; otherwise the open file is
    // the single-file entry. tinymist resolves the project root to the entry
    // file's parent directory by default (see CompileOnceArgs::resolve_sys_entry_opts).
    let entry = main_path.unwrap_or_else(|| path.clone());
    if !entry.is_file() {
        return Err(format!("Typst entry not found: {}", entry.display()));
    }
    if let Some(vp) = vault_path.as_ref() {
        if !vp.is_dir() {
            return Err(format!("Vault path is not a directory: {}", vp.display()));
        }
    }
    // tinymist's SystemAccessModel reads from the real filesystem; we do not
    // constrain reads to the vault, mirroring how the Typst CLI operates.

    let args = CompileOnceArgs {
        input: Some(entry.to_string_lossy().into_owned()),
        ..CompileOnceArgs::default()
    };

    let universe = args
        .resolve_system()
        .map_err(|err| format!("failed to resolve Typst world: {err}"))?;
    let world = universe.snapshot();

    let document = typst::compile::<PagedDocument>(&world)
        .output
        .map_err(format_diagnostics)?;

    // A non-zero gap leaves vertical space between pages; combined with the
    // iframe's grey background (see TypstPreview.tsx) the gap reads as a page
    // separator instead of continuous content.
    Ok(svg_merged(&document, &SvgOptions::default(), Abs::pt(12.0)).to_string())
}

/// Render compile diagnostics as a single human-readable block.
///
/// `SourceDiagnostic` has no `Display` impl, so we render the severity, the
/// message, and each hint. Span location is omitted in this first cut.
pub(crate) fn format_diagnostics(errors: EcoVec<SourceDiagnostic>) -> String {
    if errors.is_empty() {
        return "Typst compilation failed with no diagnostics".to_string();
    }
    let mut out = String::new();
    for diag in &errors {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&format_diagnostic(diag));
    }
    out
}

/// Format a single Typst diagnostic for inline display in the editor.
fn format_diagnostic(diag: &SourceDiagnostic) -> String {
    use typst::diag::Severity;
    let severity = match diag.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    };
    let mut out = format!("{severity}: {}", diag.message);
    for hint in &diag.hints {
        out.push_str(&format!("\n  hint: {}", hint.v));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn write_file(dir: &Path, name: &str, contents: &str) -> PathBuf {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, contents).unwrap();
        path
    }

    /// Resolve a world via tinymist and compile, returning the merged SVG.
    fn compile_to_svg(entry: &Path) -> String {
        let args = CompileOnceArgs {
            input: Some(entry.to_string_lossy().into_owned()),
            ..CompileOnceArgs::default()
        };
        let universe = args.resolve_system().expect("universe resolves");
        let world = universe.snapshot();
        let document = typst::compile::<PagedDocument>(&world)
            .output
            .expect("compile should succeed");
        svg_merged(&document, &SvgOptions::default(), Abs::zero()).to_string()
    }

    #[test]
    fn compile_single_file_renders_svg_with_text() {
        let tmp = TempDir::new().unwrap();
        let note = write_file(tmp.path(), "note.typ", "Hello Typst!");
        let svg = compile_to_svg(&note);
        assert!(
            svg.starts_with("<svg"),
            "svg should start with <svg: {}",
            &svg[..50]
        );
        // Typst renders glyphs as vector paths, so the SVG must carry non-trivial
        // rendering payload (paths and/or groups).
        assert!(
            svg.contains("<path") || svg.contains("<g"),
            "svg should contain rendered content: {}",
            &svg[..200]
        );
    }

    #[test]
    fn compile_cjk_text_renders_distinct_glyphs() {
        // Regression: the embedded `typst-assets` font set has no CJK families,
        // so Chinese rendered as tofu before system fonts were scanned. tinymist
        // resolves system fonts (notably Source Han / CJK), so each of the four
        // glyphs below must map to a distinct glyph id rather than one tofu box.
        let tmp = TempDir::new().unwrap();
        let note = write_file(tmp.path(), "note.typ", "你好世界");
        let svg = compile_to_svg(&note);

        let distinct_glyphs: std::collections::HashSet<&str> = svg
            .as_str()
            .split("<use")
            .skip(1)
            .filter_map(|rest| rest.split('"').nth(1))
            .collect();
        assert!(
            distinct_glyphs.len() >= 2,
            "CJK glyphs collapsed to {} distinct id(s): tofu regression",
            distinct_glyphs.len()
        );
    }

    #[test]
    fn compile_error_surfaces_message() {
        let tmp = TempDir::new().unwrap();
        let note = write_file(tmp.path(), "note.typ", "#let x = ;");
        let args = CompileOnceArgs {
            input: Some(note.to_string_lossy().into_owned()),
            ..CompileOnceArgs::default()
        };
        let universe = args.resolve_system().expect("universe resolves");
        let world = universe.snapshot();

        let errors = typst::compile::<PagedDocument>(&world)
            .output
            .expect_err("broken source should not compile");
        let formatted = format_diagnostics(errors);
        assert!(
            formatted.to_lowercase().contains("error"),
            "formatted diagnostics should mention error: {formatted}"
        );
    }
}
