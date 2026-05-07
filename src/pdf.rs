use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use mdbook_renderer::RenderContext;
use mdbook_renderer::book::{BookItem, Chapter};
use pulldown_cmark::{Alignment, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use typst::Library;
use typst::diag::{FileError, FileResult, SourceDiagnostic};
use typst::foundations::{Bytes, Datetime, Smart};
use typst::layout::PagedDocument;
use typst::syntax::{FileId, Source, VirtualPath};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{LibraryExt, World};
use typst_kit::fonts::{FontSlot, Fonts};
use typst_pdf::PdfOptions;

const DEFAULT_TEMPLATE: &str = include_str!("assets/template.typ");
const TITLE_PLACEHOLDER: &str = "MDBOOK_RENDERKIT_TITLE";
const CONTENT_PLACEHOLDER: &str = "/**** MDBOOK_RENDERKIT_CONTENT ****/";

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default, rename_all = "kebab-case")]
struct PdfConfig {
  template: Option<PathBuf>,
  admonish: bool,
  section_number: bool,
}

pub fn render(ctx: &RenderContext) -> Result<()> {
  fs::create_dir_all(&ctx.destination)
    .with_context(|| format!("failed to create {}", ctx.destination.display()))?;

  let typst = render_typst(ctx)?;
  let typst_path = output_filename(ctx, "typ");
  let pdf_path = output_filename(ctx, "pdf");

  eprintln!(
    "renderkit: rendering {} chapters to {}",
    ctx.book.chapters().count(),
    pdf_path.display()
  );
  fs::write(&typst_path, &typst)
    .with_context(|| format!("failed to write {}", typst_path.display()))?;
  eprintln!("renderkit: wrote {}", typst_path.display());

  compile_pdf(typst_path, typst, &pdf_path)?;
  eprintln!("renderkit: wrote {}", pdf_path.display());

  Ok(())
}

fn render_typst(ctx: &RenderContext) -> Result<String> {
  let title = ctx.config.book.title.as_deref().unwrap_or("mdBook");
  let cfg = load_config(ctx)?;
  let template = load_template(ctx, &cfg)?;
  let mut content = String::new();

  for item in ctx.book.iter() {
    if let BookItem::Chapter(chapter) = item {
      render_chapter(ctx, &cfg, chapter, &mut content)?;
      content.push_str("\n#pagebreak(weak: true)\n\n");
    }
  }

  Ok(apply_template(&template, title, &content))
}

fn load_config(ctx: &RenderContext) -> Result<PdfConfig> {
  Ok(ctx.config.get("output.pdf")?.unwrap_or_default())
}

fn output_filename(ctx: &RenderContext, extension: &str) -> PathBuf {
  match ctx.config.book.title {
    Some(ref title) => ctx.destination.join(title).with_extension(extension),
    None => ctx.destination.join("book").with_extension(extension),
  }
}

fn load_template(ctx: &RenderContext, cfg: &PdfConfig) -> Result<String> {
  if let Some(template) = &cfg.template {
    let path = ctx.root.join(template);
    eprintln!("renderkit: using PDF template {}", path.display());
    fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))
  } else {
    eprintln!("renderkit: using built-in PDF template");
    Ok(DEFAULT_TEMPLATE.to_string())
  }
}

fn apply_template(template: &str, title: &str, content: &str) -> String {
  let rendered = template.replace(TITLE_PLACEHOLDER, &escape_typst(title));

  if rendered.contains(CONTENT_PLACEHOLDER) {
    rendered.replace(CONTENT_PLACEHOLDER, content)
  } else {
    let mut rendered = rendered;
    rendered.push_str("\n\n");
    rendered.push_str(content);
    rendered
  }
}

fn render_chapter(
  ctx: &RenderContext,
  cfg: &PdfConfig,
  chapter: &Chapter,
  output: &mut String,
) -> Result<()> {
  output.push_str(&markdown_to_typst(ctx, cfg, chapter)?);

  Ok(())
}

struct ChapterContext<'a> {
  source_dir: PathBuf,
  destination: &'a Path,
  chapter_dir: Option<PathBuf>,
  chapter_rel_dir: Option<&'a Path>,
  label: String,
}

impl<'a> ChapterContext<'a> {
  fn new(ctx: &'a RenderContext, chapter: &'a Chapter) -> Self {
    let source_dir = ctx.root.join(&ctx.config.book.src);
    let chapter_rel_dir = chapter.source_path.as_ref().and_then(|path| path.parent());
    let chapter_dir = chapter_rel_dir.map(|path| source_dir.join(path));
    let label = chapter
      .source_path
      .as_ref()
      .map(|path| label_for_output_path(&md_path_to_html(path)))
      .unwrap_or_else(|| "chapter".to_string());

    Self {
      source_dir,
      destination: &ctx.destination,
      chapter_dir,
      chapter_rel_dir,
      label,
    }
  }
}

#[derive(Debug, PartialEq)]
enum InlineState {
  Heading,
  Image,
  TableHead,
}

fn markdown_to_typst(ctx: &RenderContext, cfg: &PdfConfig, chapter: &Chapter) -> Result<String> {
  let parser = Parser::new_ext(
    &chapter.content,
    Options::ENABLE_SMART_PUNCTUATION
      | Options::ENABLE_TABLES
      | Options::ENABLE_STRIKETHROUGH
      | Options::ENABLE_TASKLISTS
      | Options::ENABLE_FOOTNOTES,
  );

  let chapter_ctx = ChapterContext::new(ctx, chapter);
  let mut output = String::new();
  let mut code_block: Option<(String, String)> = None;
  let mut admonish: Option<(String, String, String)> = None;
  let mut inline_stack = Vec::new();
  let mut heading = String::new();
  let mut wrote_invisible_heading = false;

  for event in parser {
    if let Some((_, _, body)) = &mut admonish {
      match event {
        Event::End(TagEnd::CodeBlock) => {
          let (kind, title, body) = admonish.take().expect("checked above");
          render_admonish(&mut output, &kind, &title, &body)?;
        }
        Event::Text(text) => body.push_str(&text),
        Event::SoftBreak | Event::HardBreak => body.push('\n'),
        _ => {}
      }
      continue;
    }

    if let Some((_, code)) = &mut code_block {
      match event {
        Event::End(TagEnd::CodeBlock) => {
          let (lang, code) = code_block.take().expect("checked above");
          render_raw(
            &mut output,
            &code,
            true,
            (!lang.is_empty()).then_some(&lang),
          );
          output.push_str("\n\n");
        }
        Event::Text(text) => code.push_str(&text),
        Event::SoftBreak | Event::HardBreak => code.push('\n'),
        _ => {}
      }
      continue;
    }

    match event {
      Event::Start(Tag::Paragraph) => {}
      Event::End(TagEnd::Paragraph) => output.push_str("\n\n"),
      Event::Start(Tag::Heading { level, .. }) => {
        inline_stack.push(InlineState::Heading);
        heading.clear();
        let level_usize = markdown_heading_level(cfg, chapter, level as usize);
        output.push_str("#heading(level: ");
        output.push_str(&level_usize.to_string());
        output.push_str(", outlined: false, bookmarked: false");
        output.push_str(")[");
      }
      Event::End(TagEnd::Heading(level)) => {
        inline_stack.pop();
        let level_usize = markdown_heading_level(cfg, chapter, level as usize);
        output.push_str("]\n");

        if !wrote_invisible_heading {
          render_invisible_chapter_heading(&mut output, cfg, chapter, &chapter_ctx, level_usize);
          wrote_invisible_heading = true;
        }

        output.push('\n');
      }
      Event::Start(Tag::Emphasis) => output.push_str("#emph["),
      Event::End(TagEnd::Emphasis) => output.push(']'),
      Event::Start(Tag::Strong) => output.push_str("#strong["),
      Event::End(TagEnd::Strong) => output.push(']'),
      Event::Start(Tag::Strikethrough) => output.push_str("#strike["),
      Event::End(TagEnd::Strikethrough) => output.push(']'),
      Event::Start(Tag::BlockQuote(_)) => output.push_str("#quote(block: true)[\n"),
      Event::End(TagEnd::BlockQuote(_)) => output.push_str("\n]\n\n"),
      Event::Start(Tag::Table(align)) => render_table_start(&mut output, &align),
      Event::End(TagEnd::Table) => output.push_str(")\n\n"),
      Event::Start(Tag::TableHead) => inline_stack.push(InlineState::TableHead),
      Event::End(TagEnd::TableHead) => {
        inline_stack.pop();
      }
      Event::Start(Tag::TableRow) => {}
      Event::End(TagEnd::TableRow) => output.push('\n'),
      Event::Start(Tag::TableCell) => output.push('['),
      Event::End(TagEnd::TableCell) => output.push_str("],\n"),
      Event::Start(Tag::List(start)) => {
        if start.is_some() {
          output.push_str("#enum(\n");
        } else {
          output.push_str("#list(\n");
        }
      }
      Event::End(TagEnd::List(_)) => {
        output.push_str(")\n\n");
      }
      Event::Start(Tag::Item) => output.push('['),
      Event::End(TagEnd::Item) => output.push_str("],\n"),
      Event::Start(Tag::CodeBlock(kind)) => {
        let lang = match kind {
          CodeBlockKind::Indented => String::new(),
          CodeBlockKind::Fenced(lang) => lang
            .split(',')
            .next()
            .unwrap_or_default()
            .trim()
            .to_string(),
        };
        if cfg.admonish && lang.starts_with("admonish") {
          let (kind, title) = parse_admonish_info(&lang);
          admonish = Some((kind, title, String::new()));
        } else {
          code_block = Some((lang, String::new()));
        }
      }
      Event::Start(Tag::Link { dest_url, .. }) => {
        render_link_start(&mut output, &chapter_ctx, &dest_url);
      }
      Event::End(TagEnd::Link) => output.push(']'),
      Event::Start(Tag::Image { dest_url, .. }) => {
        inline_stack.push(InlineState::Image);
        render_image(&mut output, &chapter_ctx, &dest_url)?;
      }
      Event::End(TagEnd::Image) => {
        inline_stack.pop();
        output.push('\n');
      }
      Event::Code(text) => {
        if inline_stack.contains(&InlineState::Heading) {
          heading.push_str(&text);
        }
        render_raw(&mut output, &text, false, None);
      }
      Event::Text(text) => {
        if inline_stack.contains(&InlineState::Image) {
          continue;
        }

        if inline_stack.contains(&InlineState::Heading) {
          heading.push_str(&text);
        }

        if inline_stack.last() == Some(&InlineState::TableHead) {
          output.push_str("#strong[");
          output.push_str(&escape_typst(&text));
          output.push(']');
        } else {
          output.push_str(&escape_typst(&text));
        }
      }
      Event::SoftBreak => output.push('\n'),
      Event::HardBreak => output.push_str("\\\n"),
      Event::Rule => output.push_str("\n#line(length: 100%)\n\n"),
      Event::Html(html) | Event::InlineHtml(html) => {
        render_html(&mut output, &chapter_ctx, &html)?;
      }
      Event::FootnoteReference(name) => {
        output.push_str("#super[");
        output.push_str(&escape_typst(&name));
        output.push(']');
      }
      Event::TaskListMarker(checked) => {
        if checked {
          render_raw(&mut output, "[x]", false, None);
        } else {
          render_raw(&mut output, "[ ]", false, None);
        }
        output.push(' ');
      }
      _ => {}
    }
  }

  Ok(output)
}

fn render_table_start(output: &mut String, align: &[Alignment]) {
  let typst_align = align
    .iter()
    .map(|alignment| match alignment {
      Alignment::None => "auto",
      Alignment::Left => "left + horizon",
      Alignment::Center => "center + horizon",
      Alignment::Right => "right + horizon",
    })
    .collect::<Vec<_>>()
    .join(", ");

  output.push_str("#table(\n  columns: ");
  output.push_str(&align.len().to_string());
  output.push_str(",\n  inset: 6pt,\n  align: (");
  output.push_str(&typst_align);
  output.push_str("),\n");
}

fn markdown_heading_level(cfg: &PdfConfig, chapter: &Chapter, markdown_level: usize) -> usize {
  if cfg.section_number
    && let Some(number) = &chapter.number
  {
    return number.len().max(1) + markdown_level.saturating_sub(1);
  }

  markdown_level
}

fn render_invisible_chapter_heading(
  output: &mut String,
  cfg: &PdfConfig,
  chapter: &Chapter,
  ctx: &ChapterContext<'_>,
  level_usize: usize,
) {
  output.push('\n');

  if let Some(number) = &chapter.number {
    if cfg.section_number {
      output.push_str("#{\n  show heading: none\n  heading(numbering: none, level: ");
      output.push_str(&number.len().to_string());
      output.push_str(", outlined: true, bookmarked: true)[#\"");
      output.push_str(&escape_typst_string(&format!("{number} {}", chapter.name)));
      output.push_str("\"]\n} <");
    } else {
      output.push_str("#{\n  show heading: none\n  heading(numbering: none, level: ");
      output.push_str(&level_usize.to_string());
      output.push_str(", outlined: true, bookmarked: true)[");
      output.push_str(&escape_typst(&chapter.name));
      output.push_str("]\n} <");
    }
  } else {
    output.push_str(
            "#{\n  show heading: none\n  heading(numbering: none, level: 1, outlined: true, bookmarked: true)[",
        );
    output.push_str(&escape_typst(&chapter.name));
    output.push_str("]\n} <");
  }

  output.push_str(&ctx.label);
  output.push_str(">\n");
}

fn render_link_start(output: &mut String, ctx: &ChapterContext<'_>, dest_url: &str) {
  if let Some(label) = local_md_link_label(ctx.chapter_rel_dir, dest_url) {
    output.push_str("#link(<");
    output.push_str(&label);
    output.push_str(">)[");
  } else {
    let url = if is_remote_url(dest_url) {
      dest_url.to_string()
    } else if looks_like_email(dest_url) {
      format!("mailto:{dest_url}")
    } else {
      normalize_relative_link(ctx.chapter_rel_dir, dest_url)
    };

    output.push_str("#link(\"");
    output.push_str(&escape_typst_string(&url));
    output.push_str("\")[");
  }
}

fn render_image(output: &mut String, ctx: &ChapterContext<'_>, dest_url: &str) -> Result<()> {
  if is_remote_url(dest_url) {
    output.push_str("#link(\"");
    output.push_str(&escape_typst_string(dest_url));
    output.push_str("\")[");
    output.push_str(&escape_typst(dest_url));
    output.push(']');
    return Ok(());
  }

  let output_path = normalize_output_path(ctx.chapter_rel_dir, dest_url);
  let src_path = resolve_source_path(ctx, dest_url);
  let dest_path = ctx.destination.join(&output_path);
  if let Some(dest_dir) = dest_path.parent() {
    fs::create_dir_all(dest_dir)
      .with_context(|| format!("failed to create {}", dest_dir.display()))?;
  }
  fs::copy(&src_path, &dest_path).with_context(|| {
    format!(
      "failed to copy {} to {}",
      src_path.display(),
      dest_path.display()
    )
  })?;

  output.push_str("#figure(image(\"");
  output.push_str(&escape_typst_string(&output_path.to_string_lossy()));
  output.push_str("\", width: 80%))");

  Ok(())
}

fn render_html(output: &mut String, ctx: &ChapterContext<'_>, html: &str) -> Result<()> {
  match html.trim() {
    "<sup>" => output.push_str("#super["),
    "</sup>" => output.push(']'),
    "<sub>" => output.push_str("#sub["),
    "</sub>" => output.push(']'),
    value if value.starts_with("<br") => output.push_str("\\\n"),
    value if value.starts_with("<img") => {
      if let Some(src) = html_attr(value, "src") {
        render_image(output, ctx, &src)?;
      }
    }
    _ => {}
  }

  Ok(())
}

fn render_raw(output: &mut String, text: &str, block: bool, lang: Option<&str>) {
  output.push_str("#raw(\"");
  output.push_str(&escape_typst_string(text));
  output.push('"');
  if block {
    output.push_str(", block: true");
  }
  if let Some(lang) = lang {
    output.push_str(", lang: \"");
    output.push_str(&escape_typst_string(lang));
    output.push('"');
  }
  output.push(')');
}

fn render_admonish(output: &mut String, kind: &str, title: &str, markdown: &str) -> Result<()> {
  let (fill, accent) = admonish_colors(kind);
  let body = markdown_to_typst_fragment(markdown)?;
  let title = if title.is_empty() {
    capitalize(kind)
  } else {
    title.to_string()
  };

  output.push_str("#block(width: 100%, fill: rgb(\"");
  output.push_str(fill);
  output.push_str("\"), inset: 10pt, radius: 4pt, stroke: (left: 3pt + rgb(\"");
  output.push_str(accent);
  output.push_str("\")))[\n#text(fill: rgb(\"");
  output.push_str(accent);
  output.push_str("\"), weight: \"bold\")[");
  output.push_str(&escape_typst(&title));
  output.push_str("]\n\n");
  output.push_str(body.trim_end());
  output.push_str("\n]\n\n");

  Ok(())
}

fn markdown_to_typst_fragment(markdown: &str) -> Result<String> {
  let fake_ctx = RenderContext::new(
    PathBuf::from("."),
    mdbook_renderer::book::Book::new(),
    mdbook_renderer::config::Config::default(),
    PathBuf::from("."),
  );
  let chapter = Chapter::new("fragment", markdown.to_string(), "fragment.md", Vec::new());
  markdown_to_typst(&fake_ctx, &PdfConfig::default(), &chapter)
}

fn resolve_source_path(ctx: &ChapterContext<'_>, target: &str) -> PathBuf {
  let target = strip_fragment(target);
  let source_root_path = ctx.source_dir.join(target);
  if source_root_path.exists() {
    return source_root_path;
  }

  if let Some(chapter_dir) = &ctx.chapter_dir {
    let chapter_path = chapter_dir.join(target);
    if chapter_path.exists() {
      return chapter_path;
    }
  }

  source_root_path
}

fn normalize_relative_link(chapter_rel_dir: Option<&Path>, dest_url: &str) -> String {
  if let Some((path, fragment)) = dest_url.split_once('#') {
    if path.is_empty() {
      return format!("#{fragment}");
    }

    let normalized_path = normalized_output_path_str(chapter_rel_dir, path);
    if let Some(stripped) = normalized_path.strip_suffix(".md") {
      return format!("{stripped}.html#{fragment}");
    } else {
      return format!("{normalized_path}#{fragment}");
    }
  }

  let normalized = normalized_output_path_str(chapter_rel_dir, dest_url);
  if let Some(stripped) = normalized.strip_suffix(".md") {
    format!("{stripped}.html")
  } else {
    normalized
  }
}

fn local_md_link_label(chapter_rel_dir: Option<&Path>, dest_url: &str) -> Option<String> {
  if is_remote_url(dest_url) || looks_like_email(dest_url) {
    return None;
  }

  let path = strip_fragment(dest_url);
  if path.is_empty() || !path.ends_with(".md") {
    return None;
  }

  let output_path = md_path_to_html(&normalize_output_path(chapter_rel_dir, path));
  Some(label_for_output_path(&output_path))
}

fn md_path_to_html(path: &Path) -> PathBuf {
  let mut output = path.to_path_buf();
  output.set_extension("html");
  output
}

fn normalized_output_path_str(chapter_rel_dir: Option<&Path>, target: &str) -> String {
  normalize_output_path(chapter_rel_dir, target)
    .to_string_lossy()
    .into_owned()
}

fn normalize_output_path(chapter_rel_dir: Option<&Path>, target: &str) -> PathBuf {
  let target = strip_fragment(target);
  let base = if Path::new(target).is_absolute() {
    PathBuf::new()
  } else {
    chapter_rel_dir.map(Path::to_path_buf).unwrap_or_default()
  };

  normalize_join(base.join(target))
}

fn normalize_join(path: PathBuf) -> PathBuf {
  let mut normalized = PathBuf::new();

  for component in path.components() {
    match component {
      std::path::Component::CurDir => {}
      std::path::Component::ParentDir => {
        normalized.pop();
      }
      std::path::Component::Normal(part) => normalized.push(part),
      std::path::Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
      std::path::Component::RootDir => {}
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

fn html_attr(html: &str, attr: &str) -> Option<String> {
  let attr_start = html.find(attr)?;
  let after_attr = html[attr_start + attr.len()..].trim_start();
  let after_equals = after_attr.strip_prefix('=')?.trim_start();

  if let Some(after_quote) = after_equals.strip_prefix('"') {
    return after_quote
      .split_once('"')
      .map(|(value, _)| value.to_string());
  }

  if let Some(after_quote) = after_equals.strip_prefix('\'') {
    return after_quote
      .split_once('\'')
      .map(|(value, _)| value.to_string());
  }

  Some(
    after_equals
      .split(|ch: char| ch.is_whitespace() || ch == '>')
      .next()
      .unwrap_or_default()
      .to_string(),
  )
}

fn admonish_colors(admonish_type: &str) -> (&str, &str) {
  match admonish_type {
    "note" => ("#e8f4fd", "#448aff"),
    "info" | "abstract" => ("#e0f7fa", "#00b8d4"),
    "tip" => ("#e0f2f1", "#00bfa5"),
    "success" | "question" => ("#e6f6e6", "#00c853"),
    "warning" => ("#fff8e1", "#ff9100"),
    "reference" => ("#fdf6e3", "#e8a317"),
    "danger" | "failure" => ("#fde8e8", "#ff1744"),
    "bug" => ("#fce4ec", "#f50057"),
    "example" => ("#f3e5f5", "#7c4dff"),
    "quote" => ("#f5f5f5", "#9e9e9e"),
    _ => ("#f5f5f5", "#9e9e9e"),
  }
}

fn parse_admonish_info(info: &str) -> (String, String) {
  let without_prefix = info.strip_prefix("admonish").unwrap_or(info).trim();
  let (kind, rest) = without_prefix
    .split_once(' ')
    .unwrap_or((without_prefix, ""));
  let title = rest
    .trim()
    .strip_prefix("title=\"")
    .and_then(|value| value.strip_suffix('"'))
    .unwrap_or_default();

  (
    if kind.is_empty() { "note" } else { kind }.to_string(),
    title.to_string(),
  )
}

fn capitalize(value: &str) -> String {
  let mut chars = value.chars();
  match chars.next() {
    Some(first) => first.to_uppercase().to_string() + chars.as_str(),
    None => String::new(),
  }
}

fn file_error(err: io::Error, path: &Path) -> FileError {
  match err.kind() {
    io::ErrorKind::NotFound => FileError::NotFound(path.to_path_buf()),
    io::ErrorKind::PermissionDenied => FileError::AccessDenied,
    io::ErrorKind::IsADirectory => FileError::IsDirectory,
    _ => FileError::Other(Some(err.to_string().into())),
  }
}

fn compile_pdf(typst_path: PathBuf, typst: String, pdf_path: &Path) -> Result<()> {
  let world = PdfWorld::new(typst_path, typst);
  let start = std::time::Instant::now();

  let warned = typst::compile::<PagedDocument>(&world);
  print_diagnostics("warning", &warned.warnings);

  let document = match warned.output {
    Ok(document) => document,
    Err(errors) => {
      print_diagnostics("error", &errors);
      return Err(anyhow!("typst compilation failed"));
    }
  };

  let pdf = typst_pdf::pdf(
    &document,
    &PdfOptions {
      ident: Smart::Auto,
      timestamp: None,
      page_ranges: None,
      standards: typst_pdf::PdfStandards::default(),
      tagged: true,
    },
  )
  .map_err(|errors| {
    print_diagnostics("error", &errors);
    anyhow!("typst pdf export failed")
  })?;

  fs::write(pdf_path, pdf).with_context(|| format!("failed to write {}", pdf_path.display()))?;
  eprintln!(
    "renderkit: typst compilation finished in {:?}",
    start.elapsed()
  );

  Ok(())
}

fn print_diagnostics(kind: &str, diagnostics: &[SourceDiagnostic]) {
  for diagnostic in diagnostics {
    eprintln!("renderkit: {kind}: {}", diagnostic.message);
    for hint in &diagnostic.hints {
      eprintln!("renderkit: hint: {hint}");
    }
  }
}

struct PdfWorld {
  root: PathBuf,
  main: FileId,
  source: Source,
  library: LazyHash<Library>,
  book: LazyHash<FontBook>,
  fonts: Vec<FontSlot>,
}

impl PdfWorld {
  fn new(path: PathBuf, typst: String) -> Self {
    let root = path
      .parent()
      .map(Path::to_path_buf)
      .unwrap_or_else(|| PathBuf::from("."));
    let main = FileId::new_fake(VirtualPath::new("book.typ"));
    let source = Source::new(main, typst);
    let fonts = Fonts::searcher().include_system_fonts(true).search();

    Self {
      root,
      main,
      source,
      library: LazyHash::new(Library::builder().build()),
      book: LazyHash::new(fonts.book),
      fonts: fonts.fonts,
    }
  }
}

impl World for PdfWorld {
  fn library(&self) -> &LazyHash<Library> {
    &self.library
  }

  fn book(&self) -> &LazyHash<FontBook> {
    &self.book
  }

  fn main(&self) -> FileId {
    self.main
  }

  fn source(&self, id: FileId) -> FileResult<Source> {
    if id == self.main {
      Ok(self.source.clone())
    } else {
      Err(FileError::NotFound(
        self.root.join(id.vpath().as_rootless_path()),
      ))
    }
  }

  fn file(&self, id: FileId) -> FileResult<Bytes> {
    let path = self.root.join(id.vpath().as_rootless_path());
    fs::read(&path)
      .map(Bytes::new)
      .map_err(|err| file_error(err, &path))
  }

  fn font(&self, index: usize) -> Option<Font> {
    self.fonts.get(index)?.get()
  }

  fn today(&self, _offset: Option<i64>) -> Option<Datetime> {
    Datetime::from_ymd(1970, 1, 1)
  }
}

fn escape_typst(text: &str) -> String {
  let mut escaped = String::with_capacity(text.len());
  for ch in text.chars() {
    match ch {
      '\\' | '#' | '$' | '%' | '&' | '~' | '_' | '^' | '*' | '@' | '<' | '>' | '[' | ']' => {
        escaped.push('\\');
        escaped.push(ch);
      }
      _ => escaped.push(ch),
    }
  }
  escaped
}

fn label_for_output_path(path: &Path) -> String {
  escape_typst_label(&path.to_string_lossy())
}

fn escape_typst_label(text: &str) -> String {
  let mut escaped = String::with_capacity(text.len());
  for ch in text.chars() {
    if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
      escaped.push(ch);
    } else {
      escaped.push('-');
    }
  }

  if escaped.is_empty() {
    "chapter".to_string()
  } else {
    escaped
  }
}

fn escape_typst_string(text: &str) -> String {
  let mut escaped = String::with_capacity(text.len());
  for ch in text.chars() {
    match ch {
      '\\' => escaped.push_str("\\\\"),
      '"' => escaped.push_str("\\\""),
      '\n' => escaped.push_str("\\n"),
      '\r' => escaped.push_str("\\r"),
      '\t' => escaped.push_str("\\t"),
      _ => escaped.push(ch),
    }
  }
  escaped
}
