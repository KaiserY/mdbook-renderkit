use std::collections::HashSet;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};
use epub_builder::{EpubBuilder, EpubContent, EpubVersion, ReferenceType, ZipLibrary};
use mdbook_renderer::RenderContext;
use mdbook_renderer::book::Chapter;
use pulldown_cmark::{CowStr, Event, Options, Parser, Tag, html};

const DEFAULT_CSS: &str = r#"
body {
  color: #24292f;
  font-family: serif;
  line-height: 1.55;
}

h1, h2, h3, h4, h5, h6 {
  color: #111827;
  font-family: sans-serif;
  line-height: 1.25;
}

a {
  color: #0969da;
}

code {
  background: #f6f8fa;
  border-radius: 3px;
  font-family: monospace;
  font-size: 0.92em;
  padding: 0.1em 0.25em;
}

pre {
  background: #f6f8fa;
  border: 1px solid #d0d7de;
  border-radius: 4px;
  line-height: 1.45;
  overflow-x: auto;
  padding: 0.8em;
}

pre code {
  background: transparent;
  border-radius: 0;
  padding: 0;
}

blockquote {
  border-left: 0.25em solid #d0d7de;
  color: #57606a;
  margin-left: 0;
  padding-left: 1em;
}

table {
  border-collapse: collapse;
  margin: 1em 0;
  width: 100%;
}

th, td {
  border: 1px solid #d0d7de;
  padding: 0.4em 0.6em;
}

th {
  background: #edeff3;
}

img {
  max-width: 100%;
}
"#;

#[derive(Debug, serde::Deserialize)]
#[serde(default, rename_all = "kebab-case")]
struct EpubConfig {
  epub_version: u8,
  no_section_label: bool,
  use_default_css: bool,
  additional_css: Vec<PathBuf>,
  cover_image: Option<PathBuf>,
}

impl Default for EpubConfig {
  fn default() -> Self {
    Self {
      epub_version: 3,
      no_section_label: false,
      use_default_css: true,
      additional_css: Vec::new(),
      cover_image: None,
    }
  }
}

pub fn render(ctx: &RenderContext) -> Result<()> {
  fs::create_dir_all(&ctx.destination)
    .with_context(|| format!("failed to create {}", ctx.destination.display()))?;

  let output = output_filename(ctx);
  eprintln!(
    "renderkit: rendering {} chapters to {}",
    ctx.book.chapters().count(),
    output.display()
  );

  write_epub(ctx, &output)?;
  eprintln!("renderkit: wrote {}", output.display());

  Ok(())
}

fn output_filename(ctx: &RenderContext) -> PathBuf {
  match ctx.config.book.title {
    Some(ref title) => ctx.destination.join(title).with_extension("epub"),
    None => ctx.destination.join("book.epub"),
  }
}

fn write_epub(ctx: &RenderContext, path: &Path) -> Result<()> {
  let cfg = load_config(ctx)?;
  let mut builder = EpubBuilder::new(ZipLibrary::new()?)?;
  builder.epub_version(match cfg.epub_version {
    2 => EpubVersion::V20,
    _ => EpubVersion::V30,
  });

  populate_metadata(ctx, &mut builder)?;
  add_stylesheets(ctx, &cfg, &mut builder)?;
  add_cover_image(ctx, &cfg, &mut builder)?;

  let mut added_resources = HashSet::new();
  for (index, chapter) in ctx.book.chapters().enumerate() {
    add_chapter(
      ctx,
      &cfg,
      &mut builder,
      &mut added_resources,
      chapter,
      index == 0,
    )?;
  }

  let file = File::create(path).with_context(|| format!("failed to create {}", path.display()))?;
  builder.generate(file)?;

  Ok(())
}

fn load_config(ctx: &RenderContext) -> Result<EpubConfig> {
  Ok(ctx.config.get("output.epub")?.unwrap_or_default())
}

fn populate_metadata(ctx: &RenderContext, builder: &mut EpubBuilder<ZipLibrary>) -> Result<()> {
  builder.metadata("generator", "mdbook-renderkit")?;
  builder.metadata("lang", ctx.config.book.language.as_deref().unwrap_or("en"))?;

  if let Some(title) = &ctx.config.book.title {
    builder.metadata("title", title)?;
  }
  if let Some(description) = &ctx.config.book.description {
    builder.metadata("description", description)?;
  }
  if !ctx.config.book.authors.is_empty() {
    builder.metadata("author", ctx.config.book.authors.join(", "))?;
  }

  Ok(())
}

fn add_stylesheets(
  ctx: &RenderContext,
  cfg: &EpubConfig,
  builder: &mut EpubBuilder<ZipLibrary>,
) -> Result<()> {
  let mut css = Vec::new();
  if cfg.use_default_css {
    css.extend_from_slice(DEFAULT_CSS.as_bytes());
  }

  for stylesheet in &cfg.additional_css {
    let path = resolve_book_path(ctx, stylesheet)?;
    let mut file =
      File::open(&path).with_context(|| format!("failed to open {}", path.display()))?;
    file
      .read_to_end(&mut css)
      .with_context(|| format!("failed to read {}", path.display()))?;
  }

  builder.stylesheet(css.as_slice())?;
  Ok(())
}

fn add_cover_image(
  ctx: &RenderContext,
  cfg: &EpubConfig,
  builder: &mut EpubBuilder<ZipLibrary>,
) -> Result<()> {
  let Some(cover_image) = &cfg.cover_image else {
    return Ok(());
  };

  let path = resolve_book_path(ctx, cover_image)?;
  let bytes = fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
  let media_type = mime_guess::from_path(&path)
    .first_or_octet_stream()
    .to_string();
  builder.add_cover_image(cover_image, bytes.as_slice(), media_type)?;

  Ok(())
}

fn add_chapter(
  ctx: &RenderContext,
  cfg: &EpubConfig,
  builder: &mut EpubBuilder<ZipLibrary>,
  added_resources: &mut HashSet<PathBuf>,
  chapter: &Chapter,
  first: bool,
) -> Result<()> {
  let Some(source_path) = chapter.source_path.as_ref().or(chapter.path.as_ref()) else {
    return Ok(());
  };

  let chapter_path = md_path_to_html(source_path);
  let chapter_dir = source_path.parent();
  let content = render_chapter(ctx, builder, added_resources, chapter, chapter_dir)?;
  let title = chapter_title(cfg, chapter);
  let level = chapter
    .number
    .as_ref()
    .map_or(0, |number| number.len().saturating_sub(1) as i32);

  let mut content = EpubContent::new(chapter_path.to_string_lossy(), content.as_bytes())
    .title(title)
    .level(level);
  if first {
    content = content.reftype(ReferenceType::Text);
  }
  builder.add_content(content)?;

  Ok(())
}

fn render_chapter(
  ctx: &RenderContext,
  builder: &mut EpubBuilder<ZipLibrary>,
  added_resources: &mut HashSet<PathBuf>,
  chapter: &Chapter,
  chapter_dir: Option<&Path>,
) -> Result<String> {
  let parser = Parser::new_ext(
    &chapter.content,
    Options::ENABLE_SMART_PUNCTUATION
      | Options::ENABLE_TABLES
      | Options::ENABLE_STRIKETHROUGH
      | Options::ENABLE_TASKLISTS
      | Options::ENABLE_FOOTNOTES,
  );

  let mut body = String::new();
  let mut events = Vec::new();
  for event in parser {
    events.push(rewrite_event(
      ctx,
      builder,
      added_resources,
      chapter_dir,
      event,
    )?);
  }
  html::push_html(&mut body, events.into_iter());

  let title = escape_html(&chapter.name);
  Ok(format!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="no"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" xml:lang="en" lang="en">
<head>
<meta http-equiv="Content-Type" content="text/html; charset=utf-8" />
<title>{title}</title>
<link rel="stylesheet" href="{stylesheet}" />
</head>
<body>
{body}
</body>
</html>
"#,
    stylesheet = stylesheet_path(chapter_dir),
  ))
}

fn rewrite_event<'a>(
  ctx: &RenderContext,
  builder: &mut EpubBuilder<ZipLibrary>,
  added_resources: &mut HashSet<PathBuf>,
  chapter_dir: Option<&Path>,
  event: Event<'a>,
) -> Result<Event<'a>> {
  match event {
    Event::Start(Tag::Link {
      link_type,
      dest_url,
      title,
      id,
    }) => Ok(Event::Start(Tag::Link {
      link_type,
      dest_url: CowStr::from(rewrite_link(chapter_dir, &dest_url)),
      title,
      id,
    })),
    Event::Start(Tag::Image {
      link_type,
      dest_url,
      title,
      id,
    }) => {
      let rewritten = rewrite_image(ctx, builder, added_resources, chapter_dir, &dest_url)?;
      Ok(Event::Start(Tag::Image {
        link_type,
        dest_url: CowStr::from(rewritten),
        title,
        id,
      }))
    }
    _ => Ok(event),
  }
}

fn rewrite_link(chapter_dir: Option<&Path>, dest_url: &str) -> String {
  if is_remote_url(dest_url) || looks_like_email(dest_url) {
    return dest_url.to_string();
  }

  if let Some((path, fragment)) = dest_url.split_once('#') {
    if path.is_empty() {
      return format!("#{fragment}");
    }

    let path = md_link_to_html(path);
    return format!("{path}#{fragment}");
  }

  let _ = chapter_dir;
  md_link_to_html(dest_url)
}

fn rewrite_image(
  ctx: &RenderContext,
  builder: &mut EpubBuilder<ZipLibrary>,
  added_resources: &mut HashSet<PathBuf>,
  chapter_dir: Option<&Path>,
  dest_url: &str,
) -> Result<String> {
  if is_remote_url(dest_url) || dest_url.starts_with("data:") {
    return Ok(dest_url.to_string());
  }

  let path = strip_fragment(dest_url);
  if path.is_empty() {
    return Ok(dest_url.to_string());
  }

  let output_path = normalize_output_path(chapter_dir, path);
  if added_resources.insert(output_path.clone()) {
    add_local_resource(ctx, builder, &output_path)?;
  }

  Ok(dest_url.to_string())
}

fn add_local_resource(
  ctx: &RenderContext,
  builder: &mut EpubBuilder<ZipLibrary>,
  output_path: &Path,
) -> Result<()> {
  let source_dir = ctx.root.join(&ctx.config.book.src);
  let source_path = source_dir.join(output_path);
  let bytes =
    fs::read(&source_path).with_context(|| format!("failed to read {}", source_path.display()))?;
  let media_type = mime_guess::from_path(output_path)
    .first_or_octet_stream()
    .to_string();
  builder.add_resource(output_path, bytes.as_slice(), media_type)?;
  Ok(())
}

fn chapter_title(cfg: &EpubConfig, chapter: &Chapter) -> String {
  if cfg.no_section_label {
    chapter.name.clone()
  } else if let Some(number) = &chapter.number {
    format!("{number} {}", chapter.name)
  } else {
    chapter.name.clone()
  }
}

fn stylesheet_path(chapter_dir: Option<&Path>) -> String {
  chapter_dir
    .map(|path| {
      path
        .components()
        .map(|_| "..")
        .chain(std::iter::once("stylesheet.css"))
        .collect::<Vec<_>>()
        .join("/")
    })
    .unwrap_or_else(|| "stylesheet.css".to_string())
}

fn resolve_book_path(ctx: &RenderContext, path: &Path) -> Result<PathBuf> {
  let source_path = ctx.root.join(&ctx.config.book.src).join(path);
  if source_path.exists() {
    return Ok(source_path);
  }

  let root_path = ctx.root.join(path);
  if root_path.exists() {
    return Ok(root_path);
  }

  Ok(source_path)
}

fn md_link_to_html(path: &str) -> String {
  if let Some(stripped) = path.strip_suffix(".md") {
    format!("{stripped}.html")
  } else {
    path.to_string()
  }
}

fn md_path_to_html(path: &Path) -> PathBuf {
  let mut output = path.to_path_buf();
  output.set_extension("html");
  output
}

fn normalize_output_path(chapter_rel_dir: Option<&Path>, target: &str) -> PathBuf {
  let target = strip_fragment(target);
  let base = if Path::new(target).is_absolute() {
    PathBuf::new()
  } else {
    chapter_rel_dir.map(Path::to_path_buf).unwrap_or_default()
  };

  normalize_path(&base.join(target))
}

fn normalize_path(path: &Path) -> PathBuf {
  let mut normalized = PathBuf::new();

  for component in path.components() {
    match component {
      Component::CurDir => {}
      Component::ParentDir => {
        normalized.pop();
      }
      Component::Normal(part) => normalized.push(part),
      Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
      Component::RootDir => {}
    }
  }

  normalized
}

fn strip_fragment(target: &str) -> &str {
  target.split_once('#').map_or(target, |(path, _)| path)
}

fn is_remote_url(value: &str) -> bool {
  value.starts_with("http://") || value.starts_with("https://")
}

fn looks_like_email(value: &str) -> bool {
  let Some((local, domain)) = value.split_once('@') else {
    return false;
  };

  !local.is_empty() && domain.contains('.') && !domain.ends_with('.')
}

fn escape_html(text: &str) -> String {
  let mut escaped = String::with_capacity(text.len());
  for ch in text.chars() {
    match ch {
      '&' => escaped.push_str("&amp;"),
      '<' => escaped.push_str("&lt;"),
      '>' => escaped.push_str("&gt;"),
      '"' => escaped.push_str("&quot;"),
      '\'' => escaped.push_str("&#39;"),
      _ => escaped.push(ch),
    }
  }
  escaped
}
