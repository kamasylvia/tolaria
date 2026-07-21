//! Typst note preview — compiles `.typ` source to SVG inline in the editor pane.
//!
//! See ADR-0171 for the design rationale. In short: the `typst` crate lives in
//! `src-tauri`, a thin `World` implementation anchors file resolution at the
//! note's directory (or a chosen entry file's directory), and the frontend
//! invokes [`render_typst`] to receive a merged-page SVG string.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use chrono::Datelike;
use typst::diag::{EcoVec, FileError, FileResult, SourceDiagnostic};
use typst::foundations::{Bytes, Datetime};
use typst::layout::Abs;
use typst::syntax::{FileId, RealizeError, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};
use typst_layout::PagedDocument;
use typst_svg::{svg_merged, SvgOptions};

/// Compiled-once default font book plus retained fonts.
///
/// `typst-assets::fonts()` ships the same embedded font set the official Typst
/// CLI bundles by default. We build the book lazily on first use and reuse it
/// across compilations — fonts never change between renders.
static FONTS: LazyLock<FontStore> = LazyLock::new(|| {
    let mut book = FontBook::new();
    let mut fonts = Vec::new();
    for data in typst_assets::fonts() {
        for font in Font::iter(Bytes::new(data)) {
            book.push(font.info().clone());
            fonts.push(font);
        }
    }
    FontStore {
        book: LazyHash::new(book),
        fonts,
    }
});

struct FontStore {
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
}

/// The default standard library. Inputs and features are empty, matching the
/// single-file compile mode the Typst CLI uses without `--inputs`.
static LIBRARY: LazyLock<LazyHash<Library>> = LazyLock::new(|| LazyHash::new(Library::default()));

/// [`World`] implementation rooted at a single on-disk directory.
///
/// `root_dir` is the project root (the parent of the entry file). Every
/// `FileId` the compiler requests is realized relative to that directory and
/// read straight from disk. The main source is identified by its
/// vault-relative virtual path so that imports resolve consistently.
pub(crate) struct TypstWorld {
    root_dir: PathBuf,
    main_id: FileId,
}

impl TypstWorld {
    /// Build a world anchored at `root_dir`, with `main_relative` as the entry
    /// file path relative to that root (forward slashes).
    ///
    /// Returns an error if `main_relative` cannot be expressed as a virtual
    /// path (e.g. it escapes the root).
    fn new(root_dir: PathBuf, main_relative: &str) -> Result<Self, String> {
        let vpath = VirtualPath::new(main_relative)
            .map_err(|err| format!("entry path {main_relative:?} is invalid: {err}"))?;
        let main_id = FileId::new(RootedPath::new(VirtualRoot::Project, vpath));
        Ok(Self { root_dir, main_id })
    }

    /// Resolve a [`FileId`] to an absolute on-disk path and read it as text.
    fn read_source(&self, id: FileId) -> FileResult<Source> {
        let path = self.realize(id)?;
        let text = std::fs::read_to_string(&path).map_err(|err| FileError::from_io(err, &path))?;
        Ok(Source::new(id, text))
    }

    /// Resolve a [`FileId`] to an absolute on-disk path and read it as bytes.
    fn read_file(&self, id: FileId) -> FileResult<Bytes> {
        let path = self.realize(id)?;
        let bytes = std::fs::read(&path).map_err(|err| FileError::from_io(err, &path))?;
        Ok(Bytes::new(bytes))
    }

    /// Map a virtual [`FileId`] back to an absolute filesystem path under the
    /// configured root directory.
    fn realize(&self, id: FileId) -> FileResult<PathBuf> {
        id.vpath()
            .realize(&self.root_dir)
            .map_err(map_realize_error)
    }
}

impl World for TypstWorld {
    fn library(&self) -> &LazyHash<Library> {
        &LIBRARY
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &FONTS.book
    }

    fn main(&self) -> FileId {
        self.main_id
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        self.read_source(id)
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.read_file(id)
    }

    fn font(&self, index: usize) -> Option<Font> {
        FONTS.fonts.get(index).cloned()
    }

    fn today(&self, offset: Option<typst::foundations::Duration>) -> Option<Datetime> {
        // Typst's `today(None)` expects the local date; we use the system local
        // date with a zeroed time. Offsets are not supported in this minimal
        // implementation.
        let _ = offset;
        let now = chrono::Local::now();
        Datetime::from_ymd_hms(
            now.year(),
            now.month().try_into().ok()?,
            now.day().try_into().ok()?,
            0,
            0,
            0,
        )
    }
}

fn map_realize_error(err: RealizeError) -> FileError {
    FileError::Other(Some(format!("could not resolve path: {err}").into()))
}

/// Format a single Typst diagnostic for inline display in the editor.
///
/// `SourceDiagnostic` has no `Display` impl, so we render the severity, the
/// message, and each hint. Span location is intentionally omitted in this
/// first cut — Typst's span resolution requires source mapping helpers that
/// would more than double the surface of this module.
pub(crate) fn format_diagnostic(diag: &SourceDiagnostic) -> String {
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

/// Resolve the entry file and project root for a Typst note, applying the
/// four-layer anchor strategy from ADR-0171.
///
/// Layers, in priority order:
/// 1. `main_path` — explicit entry file (the future "Pin entry file" affordance).
/// 2. A `main.typ` sibling of the open file (auto-detected project entry).
/// 3. The open file itself as a single-file document.
///
/// Returns `(root_dir, main_relative_path)` where `main_relative_path` uses
/// forward slashes relative to `root_dir`.
fn resolve_entry(note_path: &Path, main_path: Option<&Path>) -> Result<(PathBuf, String), String> {
    let (entry_abs, root_dir) = match main_path {
        Some(explicit) => {
            let abs = if explicit.is_absolute() {
                explicit.to_path_buf()
            } else {
                let dir = note_path
                    .parent()
                    .ok_or_else(|| "note has no parent directory for relative entry".to_string())?;
                dir.join(explicit)
            };
            let root_dir = abs
                .parent()
                .ok_or_else(|| "explicit entry has no parent directory".to_string())?
                .to_path_buf();
            (abs, root_dir)
        }
        None => {
            // Auto-detect a `main.typ` sibling unless the note is itself main.typ.
            let dir = note_path
                .parent()
                .ok_or_else(|| "note path has no parent directory".to_string())?;
            let is_main = note_path
                .file_name()
                .map(|name| name.eq_ignore_ascii_case("main.typ"))
                .unwrap_or(false);
            let sibling_main = (!is_main).then(|| dir.join("main.typ"));
            let auto_main = sibling_main.filter(|p| p.is_file());
            match auto_main {
                Some(sibling) => {
                    let root_dir = sibling
                        .parent()
                        .ok_or_else(|| {
                            "auto-detected main.typ has no parent directory".to_string()
                        })?
                        .to_path_buf();
                    (sibling, root_dir)
                }
                None => {
                    // Single-file mode: the note is its own entry, and the root
                    // is its parent directory so sibling imports can still resolve.
                    (note_path.to_path_buf(), dir.to_path_buf())
                }
            }
        }
    };

    // Express the entry relative to the root using forward slashes.
    let main_relative = entry_abs
        .strip_prefix(&root_dir)
        .map_err(|err| format!("entry is not under root: {err}"))?
        .to_string_lossy()
        .replace('\\', "/");

    Ok((root_dir, main_relative))
}

/// Render a Typst note (and any `#import`/`#include` siblings) to a single
/// merged-page SVG string.
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
    // Basic existence checks. Tolaria's strict vault-boundary validation lives
    // behind `pub(crate)` APIs we cannot reach from this module, so we do the
    // minimal check ourselves and rely on the compiler reading only what the
    // World implementation serves from under `root_dir`.
    if !path.is_file() {
        return Err(format!("Typst note not found: {}", path.display()));
    }
    if let Some(vp) = vault_path.as_ref() {
        if !vp.is_dir() {
            return Err(format!("Vault path is not a directory: {}", vp.display()));
        }
    }
    let validated_main = match main_path.as_deref() {
        Some(main) => {
            if !main.is_file() {
                return Err(format!("Typst entry not found: {}", main.display()));
            }
            Some(main.to_path_buf())
        }
        None => None,
    };

    let (root_dir, main_relative) = resolve_entry(&path, validated_main.as_deref())?;
    let world = TypstWorld::new(root_dir, &main_relative)?;

    let compiled = typst::compile::<PagedDocument>(&world);
    let document = compiled.output.map_err(format_diagnostics)?;

    Ok(svg_merged(&document, &SvgOptions::default(), Abs::zero()))
}

/// Render compile diagnostics as a single human-readable block.
fn format_diagnostics(errors: EcoVec<SourceDiagnostic>) -> String {
    if errors.is_empty() {
        return "Typst compilation failed with no diagnostics".to_string();
    }
    let mut out = String::new();
    for diag in &errors {
        if !out.is_empty() {
            out.push('\n')
        }
        out.push_str(&format_diagnostic(diag))
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_file(dir: &Path, name: &str, contents: &str) -> PathBuf {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn entry_resolution_single_file_uses_note_directory_as_root() {
        let tmp = TempDir::new().unwrap();
        let note = write_file(tmp.path(), "report.typ", "#hi");
        let (root, main_rel) = resolve_entry(&note, None).unwrap();
        assert_eq!(root, tmp.path());
        assert_eq!(main_rel, "report.typ");
    }

    #[test]
    fn entry_resolution_auto_detects_sibling_main_typ() {
        let tmp = TempDir::new().unwrap();
        let main = write_file(tmp.path(), "main.typ", "#import \"lib.typ\": *");
        write_file(tmp.path(), "lib.typ", "#hi");
        let (root, main_rel) = resolve_entry(&main, None).unwrap();
        assert_eq!(root, tmp.path());
        assert_eq!(main_rel, "main.typ");

        // Opening a sibling of main.typ still anchors at main.typ.
        let lib_note = tmp.path().join("lib.typ");
        let (root2, main_rel2) = resolve_entry(&lib_note, None).unwrap();
        assert_eq!(root2, tmp.path());
        assert_eq!(main_rel2, "main.typ");
    }

    #[test]
    fn entry_resolution_explicit_main_path_wins() {
        let tmp = TempDir::new().unwrap();
        let nested_main = write_file(tmp.path(), "project/main.typ", "hi");
        let note = write_file(tmp.path(), "project/chapter.typ", "chapter");
        let (root, main_rel) = resolve_entry(&note, Some(&nested_main)).unwrap();
        assert_eq!(root, tmp.path().join("project"));
        assert_eq!(main_rel, "main.typ");
    }

    #[test]
    fn compile_single_file_renders_svg_with_text() {
        let tmp = TempDir::new().unwrap();
        let note = write_file(tmp.path(), "note.typ", "Hello Typst!");
        let (root, main_rel) = resolve_entry(&note, None).unwrap();
        let world = TypstWorld::new(root, &main_rel).unwrap();

        let compiled = typst::compile::<PagedDocument>(&world);
        let document = compiled.output.expect("simple compile should succeed");
        let svg = svg_merged(&document, &SvgOptions::default(), Abs::zero());

        assert!(
            svg.starts_with("<svg"),
            "svg output should start with <svg: {}",
            &svg[..50]
        );
        // Typst renders glyphs as vector paths by default, so the source text
        // itself is not preserved as a string; what we can assert is that the
        // SVG carries non-trivial rendering payload (paths + groups).
        assert!(
            svg.contains("<path") || svg.contains("<g"),
            "svg should contain rendered content: {}",
            &svg[..200]
        );
    }

    #[test]
    fn compile_multi_file_project_resolves_imports() {
        let tmp = TempDir::new().unwrap();
        write_file(
            tmp.path(),
            "main.typ",
            "#import \"lib.typ\": greeting\n#greeting()",
        );
        write_file(
            tmp.path(),
            "lib.typ",
            "#let greeting = () => [Hello from lib]",
        );
        let main_path = tmp.path().join("main.typ");

        let (root, main_rel) = resolve_entry(&main_path, None).unwrap();
        let world = TypstWorld::new(root, &main_rel).unwrap();

        let compiled = typst::compile::<PagedDocument>(&world);
        // The meaningful assertion is that compilation succeeded at all with
        // an `#import` that resolved to a sibling file — a missing resolution
        // surfaces as a compile error, not empty SVG.
        let document = compiled.output.expect("import compile should succeed");
        let svg = svg_merged(&document, &SvgOptions::default(), Abs::zero());
        assert!(
            svg.contains("<path") || svg.contains("<g"),
            "svg should contain rendered content: {}",
            &svg[..200]
        );
    }

    #[test]
    fn compile_error_surfaces_message_and_hints() {
        let tmp = TempDir::new().unwrap();
        let note = write_file(tmp.path(), "note.typ", "#let x = ;");
        let (root, main_rel) = resolve_entry(&note, None).unwrap();
        let world = TypstWorld::new(root, &main_rel).unwrap();

        let compiled = typst::compile::<PagedDocument>(&world);
        let errors = compiled
            .output
            .expect_err("broken source should not compile");
        let formatted = format_diagnostics(errors);
        assert!(
            formatted.to_lowercase().contains("error"),
            "formatted diagnostics should mention error: {formatted}"
        );
    }

    #[test]
    fn virtual_path_normalizes_forward_slash_form() {
        let tmp = TempDir::new().unwrap();
        let note = write_file(tmp.path(), "note.typ", "hi");
        let (root, main_rel) = resolve_entry(&note, None).unwrap();
        let vpath =
            VirtualPath::new(&main_rel).expect("entry relative path should be virtualizable");
        let _id = FileId::new(RootedPath::new(VirtualRoot::Project, vpath));
        // main_rel must not contain a leading slash (VirtualPath normalizes).
        assert!(!main_rel.starts_with('/'));
        let _ = root;
    }

    #[test]
    fn virtualize_and_realize_roundtrip() {
        let root = Path::new("/tmp/example");
        let abs = Path::new("/tmp/example/sub/main.typ");
        let vpath = VirtualPath::virtualize(root, abs).expect("virtualize should succeed");
        let realized = vpath.realize(root).expect("realize should round-trip");
        assert_eq!(realized, abs);
    }
}
