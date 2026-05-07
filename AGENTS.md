# Repository Guidelines

## Project Structure & Module Organization

This Rust CLI integrates with mdBook as one preprocessor and two renderers.
`src/main.rs` owns `clap` commands and mdBook protocol plumbing. PDF output is
in `src/pdf.rs`, DOCX output in `src/docx.rs`, and the built-in Typst template
in `src/assets/template.typ`.

## Build, Test, and Development Commands

- `cargo fmt --all`: format all Rust code.
- `cargo check`: type-check without producing a release binary.
- `cargo clippy --all-targets --all-features -- -D warnings`: lint with
  warnings treated as errors.
- `cargo install --path .`: install the current checkout for real mdBook tests.
- `mdbook build`: run in `../ooxmlsdk-doc` to smoke-test all three commands.

## Coding Style & Naming Conventions

Use `rustfmt`; do not hand-align code. Prefer small helper functions for
Markdown conversion rules. Use `snake_case` for functions and modules, and
`PascalCase` for types.

## PDF Renderer Notes

PDF and Typst filenames use `[book].title`, falling back to `book`.

PDF bookmarks use invisible Typst headings derived from mdBook chapters. The
visible Markdown heading uses `bookmarked: false`; the invisible chapter heading
carries the bookmark and a stable label. When
`section-number = true`, visible Markdown heading levels are offset by
`chapter.number.len()` so Typst can build a folded PDF outline.

Local `.md` links should target Typst labels, not string URLs, for example
`general/overview.md` -> `<general-overview.html>`. Avoid Rust Book-specific
heading anchor logic unless a new config explicitly requires it.

## DOCX Renderer Notes

Build DOCX output with `ooxmlsdk` WordprocessingML types, not XML strings.
When `[output.docx].section-number = true`, offset Markdown headings by the
mdBook chapter depth. Lists, code blocks, quotes, and tables should use
structures such as `w:numPr`, `NoSpacing`, `Quote`, and `w:tbl`. The default
DOCX output includes a Word TOC field; keep it configurable with `toc` and
`toc-depth`.

## Testing Guidelines

There is no dedicated test suite yet. Run `cargo fmt --all`, `cargo check`,
`cargo clippy --all-targets --all-features -- -D warnings`, and test against a
real mdBook:

```bash
cargo install --path .
mdbook build
```

For PDF changes, inspect the generated `.typ` and PDF outline nesting. For
DOCX changes, unzip `word/document.xml` and check `w:pStyle`, `w:tbl`, and
`w:tc` elements.

## Commit & Pull Request Guidelines

Use short imperative summaries, for example `Implement init mdBook renderkit
backends`. Keep commits scoped and describe user-visible behavior changes.

Pull requests should include a summary, commands run, known renderer limits,
and the mdBook fixture used for verification.
