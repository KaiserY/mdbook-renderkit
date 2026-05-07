# Repository Guidelines

## Project Structure & Module Organization

This Rust CLI integrates with mdBook as one preprocessor and two renderers.
`src/main.rs` owns `clap` commands and mdBook protocol plumbing. PDF output is
in `src/pdf.rs`, DOCX output in `src/docx.rs`, and the built-in Typst template
in `src/assets/template.typ`. Keep user-facing config examples in `README.md`.

## Build, Test, and Development Commands

- `cargo fmt --all`: format all Rust code.
- `cargo check`: type-check without producing a release binary.
- `cargo install --path .`: install the current checkout for testing with real
  mdBook projects.
- `mdbook build`: run from a configured book, such as `../ooxmlsdk-doc`, to
  smoke-test `preprocess`, `render-pdf`, and `render-docx`.

## Coding Style & Naming Conventions

Use `rustfmt`; do not hand-align code. Prefer small helper functions for
Markdown conversion rules. Use `snake_case` for functions and modules, and
`PascalCase` for types. Keep edits scoped to the renderer module they affect.

Avoid handwritten ZIP/XML package output. PDF generation should go through
Typst APIs, and DOCX generation should use `ooxmlsdk` schema and package APIs.

## PDF Renderer Notes

PDF and Typst filenames use `[book].title`, falling back to `book`. Example:
`title = "ooxmlsdk-doc"` produces `ooxmlsdk-doc.pdf` and `ooxmlsdk-doc.typ`.

PDF bookmarks use invisible Typst headings derived from mdBook chapters. The
visible Markdown heading uses `bookmarked: false`; the invisible chapter heading
carries the bookmark and a stable label. When
`section-number = true`, visible Markdown heading levels are offset by
`chapter.number.len()` so Typst can build a folded PDF outline.

Local `.md` links should target Typst labels, not string URLs. Convert
`general/overview.md` to a label such as `<general-overview.html>` and make the
matching chapter heading use the same label. Avoid Rust Book-specific heading
anchor logic unless a new config explicitly requires it.

## Testing Guidelines

There is no dedicated test suite yet. Run at least `cargo fmt --all` and
`cargo check`. For renderer behavior, also test against a real mdBook:

```bash
cargo install --path .
mdbook build
```

For PDF changes, inspect the generated `.typ` and verify PDF outline nesting
with a tool such as `strings book/pdf/<title>.pdf | rg "/Title|/Parent|/First"`.
For DOCX changes, verify output file creation.

## Commit & Pull Request Guidelines

Use short imperative summaries, for example `Implement init mdBook renderkit
backends`. Keep commits scoped and describe user-visible behavior changes.

Pull requests should include a short summary, commands run, and any known
renderer limitations. For PDF or DOCX changes, mention the mdBook fixture or
sample book used for verification.
