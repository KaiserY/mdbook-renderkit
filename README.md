# mdbook-renderkit

mdBook preprocessor plus PDF/DOCX backends.

```toml
[preprocessor.renderkit]
command = "mdbook-renderkit preprocess"

[output.pdf]
command = "mdbook-renderkit render-pdf"
# Optional, relative to the book root.
# template = "theme/pdf.typ"
# Optional, converts ```admonish ... fences into styled blocks.
# admonish = false
# Optional, use mdBook's chapter numbers for PDF bookmark titles and levels.
# section-number = false
# Optional, do not force a page break after every chapter.
# chapter-no-pagebreak = false

[output.docx]
command = "mdbook-renderkit render-docx"
# Optional, use mdBook's chapter numbers for DOCX heading levels.
# section-number = false
# Optional, do not force a page break between chapters.
# chapter-no-pagebreak = false
# Optional, insert a Word TOC field after the title.
# toc = true
# Optional, TOC heading depth.
# toc-depth = 3
```

PDF templates can use these placeholders:

- `MDBOOK_RENDERKIT_TITLE`
- `/**** MDBOOK_RENDERKIT_CONTENT ****/`
