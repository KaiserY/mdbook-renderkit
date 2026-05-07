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

[output.docx]
command = "mdbook-renderkit render-docx"
```

PDF templates can use these placeholders:

- `MDBOOK_RENDERKIT_TITLE`
- `/**** MDBOOK_RENDERKIT_CONTENT ****/`
