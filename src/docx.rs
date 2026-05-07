use std::collections::HashMap;
use std::fs::{self, File};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};
use mdbook_renderer::RenderContext;
use mdbook_renderer::book::Chapter;
use ooxmlsdk::common::XmlNamespaceDecl;
use ooxmlsdk::parts::main_document_part::MainDocumentPart;
use ooxmlsdk::parts::numbering_definitions_part::NumberingDefinitionsPart;
use ooxmlsdk::parts::wordprocessing_document::WordprocessingDocument;
use ooxmlsdk::schemas::opc_relationships::TargetMode;
use ooxmlsdk::schemas::schemas_openxmlformats_org_wordprocessingml_2006_main::{
  AbstractNum, AbstractNumId, Body, BodyChoice, Bold, BookmarkEnd, BookmarkStart, Break,
  BreakValues, Color, Document, FieldChar, FieldCharValues, FieldCode, Hyperlink, HyperlinkChoice,
  Indentation, Italic, Level, LevelJustification, LevelJustificationValues, LevelSuffix,
  LevelSuffixValues, LevelText, MultiLevelType, MultiLevelValues, NumberFormatValues, Numbering,
  NumberingFormat, NumberingId, NumberingInstance, NumberingLevelReference, NumberingProperties,
  Paragraph, ParagraphChoice, ParagraphChoice2, ParagraphProperties, ParagraphStyleId, Run,
  RunChoice, RunFonts, RunProperties, RunStyle as WordRunStyle, Shading, ShadingPatternValues,
  StartNumberingValue, Strike, TabStop, TabStopLeaderCharValues, TabStopValues, Table, TableCell,
  TableCellChoice, TableCellProperties, TableCellWidth, TableChoice2, TableProperties, TableRow,
  TableRowChoice, TableStyle, TableWidth, TableWidthUnitValues, Tabs, Text,
};
use ooxmlsdk::sdk::WordprocessingDocumentType;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

const BULLET_NUM_ID: i32 = 1;
const ORDERED_NUM_ID: i32 = 2;
const TASK_DONE_NUM_ID: i32 = 3;
const TASK_TODO_NUM_ID: i32 = 4;

#[derive(Debug, serde::Deserialize)]
#[serde(default, rename_all = "kebab-case")]
struct DocxConfig {
  section_number: bool,
  toc: bool,
  toc_depth: usize,
}

impl Default for DocxConfig {
  fn default() -> Self {
    Self {
      section_number: false,
      toc: true,
      toc_depth: 3,
    }
  }
}

impl DocxConfig {
  fn toc_depth(&self) -> usize {
    self.toc_depth.clamp(1, 9)
  }
}

pub fn render(ctx: &RenderContext) -> Result<()> {
  fs::create_dir_all(&ctx.destination)
    .with_context(|| format!("failed to create {}", ctx.destination.display()))?;

  let output = output_filename(ctx, "docx");
  eprintln!(
    "renderkit: rendering {} chapters to {}",
    ctx.book.chapters().count(),
    output.display()
  );

  write_docx(ctx, &output)?;
  eprintln!("renderkit: wrote {}", output.display());

  Ok(())
}

fn output_filename(ctx: &RenderContext, extension: &str) -> PathBuf {
  match ctx.config.book.title {
    Some(ref title) => ctx.destination.join(title).with_extension(extension),
    None => ctx.destination.join("book").with_extension(extension),
  }
}

fn write_docx(ctx: &RenderContext, path: &Path) -> Result<()> {
  let mut package = WordprocessingDocument::create(WordprocessingDocumentType::Document);
  let main_part = package.add_main_document_part()?;
  let cfg = load_config(ctx)?;
  let numbering_part = package.add_new_part_auto_id::<NumberingDefinitionsPart>()?;
  let numbering_part = main_part.add_part(&mut package, numbering_part)?;
  numbering_part.set_root_element(&mut package, numbering_definitions())?;

  let document = {
    let mut docx = DocxRenderContext::new(&mut package, &main_part);
    document(ctx, &cfg, &mut docx)?
  };
  main_part.set_root_element(&mut package, document)?;

  let file = File::create(path).with_context(|| format!("failed to create {}", path.display()))?;
  package.save(file)?;

  Ok(())
}

fn load_config(ctx: &RenderContext) -> Result<DocxConfig> {
  let mut cfg: DocxConfig = ctx.config.get("output.docx")?.unwrap_or_default();
  if cfg.toc_depth == 0 {
    cfg.toc_depth = 3;
  }
  Ok(cfg)
}

fn document(
  ctx: &RenderContext,
  cfg: &DocxConfig,
  docx: &mut DocxRenderContext<'_>,
) -> Result<Document> {
  let mut body = Body::default();

  if let Some(title) = &ctx.config.book.title {
    body
      .body_choice
      .push(BodyChoice::WP(Box::new(title_paragraph(title))));
  }

  if cfg.toc {
    body.body_choice.extend(toc_block(ctx, cfg, docx));
  }

  let mut chapters = ctx.book.chapters().peekable();
  while let Some(chapter) = chapters.next() {
    let before = body.body_choice.len();
    body.body_choice.extend(chapter_body(cfg, docx, chapter)?);
    if body.body_choice.len() == before {
      let bookmark = docx.chapter_bookmark(chapter);
      body
        .body_choice
        .push(BodyChoice::WP(Box::new(heading_paragraph_with_bookmark(
          chapter_level(chapter),
          &chapter.name,
          bookmark,
        ))));
    }
    if chapters.peek().is_some() {
      body
        .body_choice
        .push(BodyChoice::WP(Box::new(page_break_paragraph())));
    }
  }

  Ok(Document {
    xmlns: vec![
      XmlNamespaceDecl::new(
        "w",
        "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
      ),
      XmlNamespaceDecl::new(
        "r",
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
      ),
    ],
    body: Some(Box::new(body)),
    ..Default::default()
  })
}

fn chapter_level(chapter: &Chapter) -> usize {
  chapter
    .number
    .as_ref()
    .map_or(1, |number| number.len().max(1))
}

fn chapter_body(
  cfg: &DocxConfig,
  docx: &mut DocxRenderContext<'_>,
  chapter: &Chapter,
) -> Result<Vec<BodyChoice>> {
  let parser = Parser::new_ext(
    &chapter.content,
    Options::ENABLE_SMART_PUNCTUATION
      | Options::ENABLE_TABLES
      | Options::ENABLE_STRIKETHROUGH
      | Options::ENABLE_TASKLISTS
      | Options::ENABLE_FOOTNOTES,
  );

  let mut out = Vec::new();
  let mut paragraph = ParagraphBuilder::default();
  let mut code_block: Option<(String, String)> = None;
  let mut lists: Vec<ListState> = Vec::new();
  let mut quote_depth = 0usize;
  let mut table: Option<TableBuilder> = None;
  let mut chapter_bookmark = docx.chapter_bookmark(chapter);

  for event in parser {
    if let Some((_, code)) = &mut code_block {
      match event {
        Event::End(TagEnd::CodeBlock) => {
          let (lang, code) = code_block.take().expect("checked above");
          push_code_block(&mut out, &mut table, &lang, &code);
        }
        Event::Text(text) => code.push_str(&text),
        Event::SoftBreak | Event::HardBreak => code.push('\n'),
        _ => {}
      }
      continue;
    }

    match event {
      Event::Start(Tag::Paragraph) => {
        let kind = if quote_depth > 0 {
          ParagraphKind::Quote
        } else {
          ParagraphKind::Normal
        };
        paragraph = ParagraphBuilder::for_block(kind, quote_depth, None)
      }
      Event::End(TagEnd::Paragraph) => paragraph.flush_to(&mut out, &mut table),
      Event::Start(Tag::Heading { level, .. }) => {
        let markdown_level = heading_level(level);
        let level = if cfg.section_number {
          chapter_level(chapter) + markdown_level - 1
        } else {
          markdown_level
        };
        paragraph = ParagraphBuilder::with_heading(level);
        paragraph.bookmark = chapter_bookmark.take();
      }
      Event::End(TagEnd::Heading(_)) => paragraph.flush_to(&mut out, &mut table),
      Event::Start(Tag::Emphasis) => paragraph.style.italic += 1,
      Event::End(TagEnd::Emphasis) => paragraph.style.italic -= 1,
      Event::Start(Tag::Strong) => paragraph.style.bold += 1,
      Event::End(TagEnd::Strong) => paragraph.style.bold -= 1,
      Event::Start(Tag::Strikethrough) => paragraph.style.strike += 1,
      Event::End(TagEnd::Strikethrough) => paragraph.style.strike -= 1,
      Event::Start(Tag::BlockQuote(_)) => {
        quote_depth += 1;
      }
      Event::End(TagEnd::BlockQuote(_)) => quote_depth = quote_depth.saturating_sub(1),
      Event::Start(Tag::List(start)) => lists.push(ListState::new(start)),
      Event::End(TagEnd::List(_)) => {
        lists.pop();
      }
      Event::Start(Tag::Item) => {
        let depth = lists.len().max(1);
        let marker = lists
          .last_mut()
          .map(|list| list.numbering_id())
          .unwrap_or(BULLET_NUM_ID);
        paragraph = ParagraphBuilder::for_block(ParagraphKind::List, quote_depth, Some(depth));
        paragraph.list_numbering_id = Some(marker);
      }
      Event::End(TagEnd::Item) => paragraph.flush_to(&mut out, &mut table),
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
        code_block = Some((lang, String::new()));
      }
      Event::Start(Tag::Table(_)) => {
        table = Some(TableBuilder::default());
      }
      Event::End(TagEnd::Table) => {
        if let Some(table) = table.take() {
          out.push(BodyChoice::WTbl(Box::new(table.into_table())));
        }
      }
      Event::Start(Tag::TableHead) => {
        if let Some(table) = &mut table {
          table.in_head = true;
        }
      }
      Event::End(TagEnd::TableHead) => {
        if let Some(table) = &mut table {
          table.in_head = false;
        }
      }
      Event::Start(Tag::TableRow) => {
        if let Some(table) = &mut table {
          table.start_row();
        }
      }
      Event::End(TagEnd::TableRow) => {
        paragraph.flush_to(&mut out, &mut table);
        if let Some(table) = &mut table {
          table.end_row();
        }
      }
      Event::Start(Tag::TableCell) => {
        if let Some(table) = &mut table {
          table.start_cell();
        }
        paragraph = ParagraphBuilder::default();
      }
      Event::End(TagEnd::TableCell) => {
        paragraph.flush_to(&mut out, &mut table);
        if let Some(table) = &mut table {
          table.end_cell();
        }
      }
      Event::Start(Tag::Link { dest_url, .. }) => {
        paragraph
          .link_stack
          .push(docx.link_target(chapter, &dest_url)?);
      }
      Event::End(TagEnd::Link) => {
        paragraph.link_stack.pop();
      }
      Event::Start(Tag::Image { dest_url, .. }) => {
        paragraph.push_text("[image: ");
        paragraph.push_text(&dest_url);
        paragraph.push_text("]");
      }
      Event::Code(text) => {
        paragraph.push_run(
          &text,
          RunStyle {
            code: true,
            ..paragraph.current_run_style()
          },
        );
      }
      Event::Text(text) => paragraph.push_text(&text),
      Event::SoftBreak | Event::HardBreak => paragraph.push_text("\n"),
      Event::Rule => {
        paragraph.flush_to(&mut out, &mut table);
        out.push(BodyChoice::WP(Box::new(paragraph_from_text(
          "----------------------------------------",
          RunStyle::default(),
        ))));
      }
      Event::TaskListMarker(checked) => {
        paragraph.list_numbering_id = Some(if checked {
          TASK_DONE_NUM_ID
        } else {
          TASK_TODO_NUM_ID
        });
      }
      Event::Html(html) | Event::InlineHtml(html) => paragraph.push_text(html.trim()),
      Event::FootnoteReference(name) => {
        paragraph.push_text("[");
        paragraph.push_text(&name);
        paragraph.push_text("]");
      }
      _ => {}
    }
  }

  paragraph.flush_to(&mut out, &mut table);
  Ok(out)
}

struct DocxRenderContext<'a> {
  package: &'a mut WordprocessingDocument,
  main_part: &'a MainDocumentPart,
  hyperlinks: HashMap<String, String>,
  bookmarks: HashMap<PathBuf, String>,
  bookmark_ids: HashMap<String, String>,
  next_bookmark_id: usize,
}

impl<'a> DocxRenderContext<'a> {
  fn new(package: &'a mut WordprocessingDocument, main_part: &'a MainDocumentPart) -> Self {
    Self {
      package,
      main_part,
      hyperlinks: HashMap::new(),
      bookmarks: HashMap::new(),
      bookmark_ids: HashMap::new(),
      next_bookmark_id: 1,
    }
  }

  fn link_target(&mut self, chapter: &Chapter, url: &str) -> Result<LinkTarget> {
    if let Some(anchor) = url.strip_prefix('#') {
      return Ok(LinkTarget::Anchor(bookmark_name(anchor)));
    }

    if let Some(path) = local_md_path(chapter, url) {
      return Ok(LinkTarget::Anchor(self.bookmark_for_path(&path)));
    }

    if let Some(id) = self.hyperlinks.get(url) {
      return Ok(LinkTarget::External(id.clone()));
    }

    let id = self
      .main_part
      .add_hyperlink_relationship_auto_id(self.package, url, TargetMode::External)?
      .id()
      .to_string();
    self.hyperlinks.insert(url.to_string(), id.clone());
    Ok(LinkTarget::External(id))
  }

  fn chapter_bookmark(&mut self, chapter: &Chapter) -> Option<BookmarkRef> {
    let source_path = chapter.source_path.as_ref()?;
    let name = self.bookmark_for_path(source_path);
    let id = self.bookmark_id(&name);
    Some(BookmarkRef { id, name })
  }

  fn bookmark_for_path(&mut self, path: &Path) -> String {
    let normalized = normalize_path(path);
    if let Some(name) = self.bookmarks.get(&normalized) {
      return name.clone();
    }

    let mut name = format!("md_{}", bookmark_name(&normalized.to_string_lossy()));
    if name.len() > 36 {
      name.truncate(36);
    }
    let candidate = if self.bookmark_ids.contains_key(&name) {
      format!("{}_{}", name, self.bookmarks.len() + 1)
    } else {
      name
    };
    let candidate = candidate.chars().take(40).collect::<String>();
    self.bookmarks.insert(normalized, candidate.clone());
    candidate
  }

  fn bookmark_id(&mut self, name: &str) -> String {
    if let Some(id) = self.bookmark_ids.get(name) {
      return id.clone();
    }
    let id = self.next_bookmark_id.to_string();
    self.next_bookmark_id += 1;
    self.bookmark_ids.insert(name.to_string(), id.clone());
    id
  }
}

#[derive(Clone, Debug)]
struct BookmarkRef {
  id: String,
  name: String,
}

#[derive(Clone, Debug)]
struct ListState {
  numbering_id: i32,
}

impl ListState {
  fn new(start: Option<u64>) -> Self {
    Self {
      numbering_id: if start.is_some() {
        ORDERED_NUM_ID
      } else {
        BULLET_NUM_ID
      },
    }
  }

  fn numbering_id(&self) -> i32 {
    self.numbering_id
  }
}

#[derive(Clone, Debug)]
enum LinkTarget {
  External(String),
  Anchor(String),
}

#[derive(Clone, Copy, Debug, Default)]
enum ParagraphKind {
  #[default]
  Normal,
  Code,
  List,
  Quote,
}

#[derive(Clone, Copy, Debug, Default)]
struct StyleDepth {
  bold: usize,
  italic: usize,
  strike: usize,
}

#[derive(Clone, Copy, Debug, Default)]
struct RunStyle {
  bold: bool,
  italic: bool,
  strike: bool,
  code: bool,
  hyperlink: bool,
}

#[derive(Debug, Default)]
struct ParagraphBuilder {
  runs: Vec<ParagraphChoice>,
  style: StyleDepth,
  paragraph_style: Option<String>,
  kind: ParagraphKind,
  quote_depth: usize,
  list_depth: Option<usize>,
  list_numbering_id: Option<i32>,
  link_stack: Vec<LinkTarget>,
  bookmark: Option<BookmarkRef>,
}

impl ParagraphBuilder {
  fn with_heading(level: usize) -> Self {
    Self {
      paragraph_style: Some(format!("Heading{}", level.clamp(1, 9))),
      ..Default::default()
    }
  }

  fn for_block(kind: ParagraphKind, quote_depth: usize, list_depth: Option<usize>) -> Self {
    Self {
      paragraph_style: paragraph_style_for_kind(kind).map(str::to_string),
      kind,
      quote_depth,
      list_depth,
      ..Default::default()
    }
  }

  fn push_text(&mut self, text: &str) {
    self.push_run(text, self.current_run_style());
  }

  fn current_run_style(&self) -> RunStyle {
    RunStyle {
      bold: self.style.bold > 0,
      italic: self.style.italic > 0,
      strike: self.style.strike > 0,
      code: matches!(self.kind, ParagraphKind::Code),
      hyperlink: !self.link_stack.is_empty(),
    }
  }

  fn push_run(&mut self, text: &str, style: RunStyle) {
    if text.is_empty() {
      return;
    }

    for (index, line) in text.split('\n').enumerate() {
      if index > 0 {
        self.runs.push(ParagraphChoice::WR(Box::new(Run {
          run_choice: vec![RunChoice::WBr(Box::new(Break::default()))],
          ..Default::default()
        })));
      }
      if !line.is_empty() {
        self.push_text_run(line, style);
      }
    }
  }

  fn push_text_run(&mut self, text: &str, style: RunStyle) {
    let run = text_run(text, style);
    if let Some(link) = self.link_stack.last() {
      self
        .runs
        .push(ParagraphChoice::WHyperlink(Box::new(hyperlink(link, run))));
    } else {
      self.runs.push(ParagraphChoice::WR(Box::new(run)));
    }
  }

  fn flush_to(&mut self, out: &mut Vec<BodyChoice>, table: &mut Option<TableBuilder>) {
    if self.runs.is_empty() {
      return;
    }

    let mut paragraph_choice = Vec::new();
    if let Some(bookmark) = self.bookmark.take() {
      paragraph_choice.push(ParagraphChoice::Choice(Box::new(
        ParagraphChoice2::WBookmarkStart(Box::new(BookmarkStart {
          name: bookmark.name,
          id: bookmark.id.clone(),
          ..Default::default()
        })),
      )));
      paragraph_choice.append(&mut self.runs);
      paragraph_choice.push(ParagraphChoice::Choice(Box::new(
        ParagraphChoice2::WBookmarkEnd(Box::new(BookmarkEnd {
          id: bookmark.id,
          ..Default::default()
        })),
      )));
    } else {
      paragraph_choice.append(&mut self.runs);
    }

    let paragraph = Paragraph {
      paragraph_properties: Some(Box::new(block_properties(
        self.paragraph_style.take().as_deref(),
        self.kind,
        self.quote_depth,
        self.list_depth,
        self.list_numbering_id,
      ))),
      paragraph_choice,
      ..Default::default()
    };

    push_paragraph(out, table, paragraph);
  }
}

#[derive(Debug, Default)]
struct TableBuilder {
  rows: Vec<TableRowData>,
  current_row: Vec<TableCellData>,
  current_cell: Vec<Paragraph>,
  in_head: bool,
  current_head: bool,
}

#[derive(Debug, Default)]
struct TableRowData {
  cells: Vec<TableCellData>,
  is_head: bool,
}

#[derive(Debug, Default)]
struct TableCellData {
  paragraphs: Vec<Paragraph>,
  is_head: bool,
}

impl TableBuilder {
  fn start_row(&mut self) {
    self.current_row.clear();
    self.current_head = self.in_head;
  }

  fn end_row(&mut self) {
    if !self.current_cell.is_empty() {
      self.end_cell();
    }
    if !self.current_row.is_empty() {
      self.rows.push(TableRowData {
        cells: std::mem::take(&mut self.current_row),
        is_head: self.current_head,
      });
    }
  }

  fn start_cell(&mut self) {
    self.current_cell.clear();
  }

  fn end_cell(&mut self) {
    if self.current_cell.is_empty() {
      self.current_cell.push(Paragraph::default());
    }
    self.current_row.push(TableCellData {
      paragraphs: std::mem::take(&mut self.current_cell),
      is_head: self.current_head,
    });
  }

  fn push_paragraph(&mut self, paragraph: Paragraph) {
    self.current_cell.push(paragraph);
  }

  fn into_table(self) -> Table {
    let column_count = self
      .rows
      .iter()
      .map(|row| row.cells.len())
      .max()
      .unwrap_or(1)
      .max(1);
    let cell_width = (8640 / column_count).max(720);

    Table {
      w_tbl_pr: Some(Box::new(TableProperties {
        table_style: Some(TableStyle {
          val: "TableGrid".to_string(),
        }),
        table_width: Some(TableWidth {
          width: Some("5000".to_string()),
          r#type: Some(TableWidthUnitValues::Pct),
        }),
        ..Default::default()
      })),
      table_choice2: self
        .rows
        .into_iter()
        .map(|row| {
          TableChoice2::WTr(Box::new(TableRow {
            table_row_choice: row
              .cells
              .into_iter()
              .map(|cell| {
                TableRowChoice::WTc(Box::new(TableCell {
                  table_cell_properties: Some(Box::new(TableCellProperties {
                    table_cell_width: Some(TableCellWidth {
                      width: Some(cell_width.to_string()),
                      r#type: Some(TableWidthUnitValues::Dxa),
                    }),
                    shading: (cell.is_head || row.is_head).then(table_shading),
                    ..Default::default()
                  })),
                  table_cell_choice: cell
                    .paragraphs
                    .into_iter()
                    .map(|paragraph| TableCellChoice::WP(Box::new(paragraph)))
                    .collect(),
                  ..Default::default()
                }))
              })
              .collect(),
            ..Default::default()
          }))
        })
        .collect(),
      ..Default::default()
    }
  }
}

fn push_paragraph(
  out: &mut Vec<BodyChoice>,
  table: &mut Option<TableBuilder>,
  paragraph: Paragraph,
) {
  if let Some(table) = table {
    table.push_paragraph(paragraph);
  } else {
    out.push(BodyChoice::WP(Box::new(paragraph)));
  }
}

fn push_code_block(
  out: &mut Vec<BodyChoice>,
  table: &mut Option<TableBuilder>,
  lang: &str,
  code: &str,
) {
  let mut label = ParagraphBuilder::for_block(ParagraphKind::Normal, 0, None);
  label.style.bold += 1;
  label.push_text(if lang.is_empty() { "code" } else { lang });
  label.flush_to(out, table);

  for line in code.lines() {
    let mut paragraph = ParagraphBuilder::for_block(ParagraphKind::Code, 0, None);
    paragraph.push_text(line);
    paragraph.flush_to(out, table);
  }
}

fn heading_paragraph_with_bookmark(
  level: usize,
  text: &str,
  bookmark: Option<BookmarkRef>,
) -> Paragraph {
  let mut paragraph_choice = Vec::new();
  if let Some(bookmark) = bookmark {
    paragraph_choice.push(ParagraphChoice::Choice(Box::new(
      ParagraphChoice2::WBookmarkStart(Box::new(BookmarkStart {
        name: bookmark.name,
        id: bookmark.id.clone(),
        ..Default::default()
      })),
    )));
    paragraph_choice.push(ParagraphChoice::WR(Box::new(text_run(
      text,
      RunStyle {
        bold: true,
        ..Default::default()
      },
    ))));
    paragraph_choice.push(ParagraphChoice::Choice(Box::new(
      ParagraphChoice2::WBookmarkEnd(Box::new(BookmarkEnd {
        id: bookmark.id,
        ..Default::default()
      })),
    )));
  } else {
    paragraph_choice.push(ParagraphChoice::WR(Box::new(text_run(
      text,
      RunStyle {
        bold: true,
        ..Default::default()
      },
    ))));
  }

  Paragraph {
    paragraph_properties: Some(Box::new(paragraph_properties(&format!(
      "Heading{}",
      level.clamp(1, 9)
    )))),
    paragraph_choice,
    ..Default::default()
  }
}

fn title_paragraph(text: &str) -> Paragraph {
  Paragraph {
    paragraph_properties: Some(Box::new(paragraph_properties("Title"))),
    paragraph_choice: vec![ParagraphChoice::WR(Box::new(text_run(
      text,
      RunStyle {
        bold: true,
        ..Default::default()
      },
    )))],
    ..Default::default()
  }
}

fn toc_heading_paragraph(text: &str) -> Paragraph {
  Paragraph {
    paragraph_properties: Some(Box::new(paragraph_properties("TOCHeading"))),
    paragraph_choice: vec![ParagraphChoice::WR(Box::new(text_run(
      text,
      RunStyle {
        bold: true,
        ..Default::default()
      },
    )))],
    ..Default::default()
  }
}

fn paragraph_from_text(text: &str, style: RunStyle) -> Paragraph {
  Paragraph {
    paragraph_choice: vec![ParagraphChoice::WR(Box::new(text_run(text, style)))],
    ..Default::default()
  }
}

fn paragraph_properties(style: &str) -> ParagraphProperties {
  ParagraphProperties {
    paragraph_style_id: Some(ParagraphStyleId {
      val: style.to_string(),
    }),
    ..Default::default()
  }
}

fn toc_block(
  ctx: &RenderContext,
  cfg: &DocxConfig,
  docx: &mut DocxRenderContext<'_>,
) -> Vec<BodyChoice> {
  let mut out = vec![
    BodyChoice::WP(Box::new(toc_heading_paragraph("Contents"))),
    BodyChoice::WP(Box::new(Paragraph {
      paragraph_choice: vec![
        ParagraphChoice::WR(Box::new(field_char_run(FieldCharValues::Begin, true))),
        ParagraphChoice::WR(Box::new(field_code_run(&format!(
          r#" TOC \o "1-{}" \h \z \u "#,
          cfg.toc_depth()
        )))),
        ParagraphChoice::WR(Box::new(field_char_run(FieldCharValues::Separate, false))),
      ],
      ..Default::default()
    })),
  ];

  for chapter in ctx
    .book
    .chapters()
    .filter(|chapter| chapter_level(chapter) <= cfg.toc_depth())
  {
    if let Some(paragraph) = toc_entry_paragraph(cfg, docx, chapter) {
      out.push(BodyChoice::WP(Box::new(paragraph)));
    }
  }

  out.push(BodyChoice::WP(Box::new(Paragraph {
    paragraph_choice: vec![ParagraphChoice::WR(Box::new(field_char_run(
      FieldCharValues::End,
      false,
    )))],
    ..Default::default()
  })));
  out.push(BodyChoice::WP(Box::new(page_break_paragraph())));
  out
}

fn toc_entry_paragraph(
  cfg: &DocxConfig,
  docx: &mut DocxRenderContext<'_>,
  chapter: &Chapter,
) -> Option<Paragraph> {
  let bookmark = chapter
    .source_path
    .as_ref()
    .map(|path| docx.bookmark_for_path(path))?;
  let level = chapter_level(chapter).clamp(1, 9);
  let title = if cfg.section_number {
    chapter.number.as_ref().map_or_else(
      || chapter.name.clone(),
      |number| format!("{number} {}", chapter.name),
    )
  } else {
    chapter.name.clone()
  };

  Some(Paragraph {
    paragraph_properties: Some(Box::new(toc_entry_properties(level))),
    paragraph_choice: vec![ParagraphChoice::WHyperlink(Box::new(Hyperlink {
      anchor: Some(bookmark),
      history: Some(true),
      hyperlink_choice: vec![
        HyperlinkChoice::WR(Box::new(text_run(
          &title,
          RunStyle {
            hyperlink: true,
            ..Default::default()
          },
        ))),
        HyperlinkChoice::WR(Box::new(tab_run())),
      ],
      ..Default::default()
    }))],
    ..Default::default()
  })
}

fn toc_entry_properties(level: usize) -> ParagraphProperties {
  let mut properties = paragraph_properties(&format!("TOC{}", level.clamp(1, 9)));
  properties.indentation = Some(Indentation {
    left: Some(((level.saturating_sub(1)) * 420).to_string()),
    ..Default::default()
  });
  properties.tabs = Some(Tabs {
    w_tab: vec![TabStop {
      val: TabStopValues::Right,
      leader: Some(TabStopLeaderCharValues::Dot),
      position: 9000,
    }],
  });
  properties
}

fn field_char_run(field_char_type: FieldCharValues, dirty: bool) -> Run {
  Run {
    run_choice: vec![RunChoice::WFldChar(Box::new(FieldChar {
      field_char_type,
      dirty: dirty.then_some(true),
      ..Default::default()
    }))],
    ..Default::default()
  }
}

fn field_code_run(instruction: &str) -> Run {
  Run {
    run_choice: vec![RunChoice::WInstrText(Box::new(FieldCode {
      xml_content: Some(instruction.to_string()),
      ..Default::default()
    }))],
    ..Default::default()
  }
}

fn page_break_paragraph() -> Paragraph {
  Paragraph {
    paragraph_choice: vec![ParagraphChoice::WR(Box::new(Run {
      run_choice: vec![RunChoice::WBr(Box::new(Break {
        r#type: Some(BreakValues::Page),
        ..Default::default()
      }))],
      ..Default::default()
    }))],
    ..Default::default()
  }
}

fn hyperlink(target: &LinkTarget, run: Run) -> Hyperlink {
  match target {
    LinkTarget::External(id) => Hyperlink {
      id: Some(id.clone()),
      history: Some(true),
      hyperlink_choice: vec![HyperlinkChoice::WR(Box::new(run))],
      ..Default::default()
    },
    LinkTarget::Anchor(anchor) => Hyperlink {
      anchor: Some(anchor.clone()),
      history: Some(true),
      hyperlink_choice: vec![HyperlinkChoice::WR(Box::new(run))],
      ..Default::default()
    },
  }
}

fn bookmark_name(anchor: &str) -> String {
  let mut out = String::new();
  for ch in anchor.chars() {
    if ch.is_ascii_alphanumeric() || ch == '_' {
      out.push(ch);
    } else if ch == '-' || ch == ' ' {
      out.push('_');
    }
  }
  if out.is_empty() {
    "bookmark".to_string()
  } else {
    out
  }
}

fn local_md_path(chapter: &Chapter, url: &str) -> Option<PathBuf> {
  if is_remote_url(url) {
    return None;
  }

  let path = url.split('#').next().unwrap_or(url);
  if !path.ends_with(".md") {
    return None;
  }

  let mut resolved = chapter
    .source_path
    .as_ref()
    .and_then(|path| path.parent())
    .map_or_else(PathBuf::new, Path::to_path_buf);
  resolved.push(path);
  Some(normalize_path(&resolved))
}

fn is_remote_url(url: &str) -> bool {
  url.starts_with("http://")
    || url.starts_with("https://")
    || url.starts_with("mailto:")
    || url.starts_with("tel:")
}

fn normalize_path(path: &Path) -> PathBuf {
  let mut out = PathBuf::new();
  for component in path.components() {
    match component {
      Component::CurDir => {}
      Component::ParentDir => {
        out.pop();
      }
      Component::Normal(part) => out.push(part),
      Component::RootDir | Component::Prefix(_) => {}
    }
  }
  out
}

fn numbering_definitions() -> Numbering {
  Numbering {
    xmlns: vec![XmlNamespaceDecl::new(
      "w",
      "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
    )],
    w_abstract_num: vec![
      abstract_numbering(1, NumberFormatValues::Bullet, "-"),
      abstract_numbering(2, NumberFormatValues::Decimal, "%1."),
      abstract_numbering(3, NumberFormatValues::Bullet, "[x]"),
      abstract_numbering(4, NumberFormatValues::Bullet, "[ ]"),
    ],
    w_num: vec![
      numbering_instance(BULLET_NUM_ID, 1),
      numbering_instance(ORDERED_NUM_ID, 2),
      numbering_instance(TASK_DONE_NUM_ID, 3),
      numbering_instance(TASK_TODO_NUM_ID, 4),
    ],
    ..Default::default()
  }
}

fn abstract_numbering(id: i32, format: NumberFormatValues, text: &str) -> AbstractNum {
  AbstractNum {
    abstract_number_id: id,
    multi_level_type: Some(MultiLevelType {
      val: MultiLevelValues::Multilevel,
    }),
    w_lvl: (0..9)
      .map(|level| numbering_level(level, format, text))
      .collect(),
    ..Default::default()
  }
}

fn numbering_level(level: i32, format: NumberFormatValues, text: &str) -> Level {
  let level_text = if matches!(format, NumberFormatValues::Decimal) {
    format!("%{}.", level + 1)
  } else {
    text.to_string()
  };
  Level {
        level_index: level,
        start_numbering_value: Some(StartNumberingValue { val: 1 }),
        numbering_format: Some(NumberingFormat {
            val: format,
            ..Default::default()
        }),
        level_suffix: Some(LevelSuffix {
            val: LevelSuffixValues::Tab,
        }),
        level_text: Some(LevelText {
            val: Some(level_text),
            ..Default::default()
        }),
        level_justification: Some(LevelJustification {
            w_val: LevelJustificationValues::Left,
        }),
        previous_paragraph_properties: Some(Box::new(
            ooxmlsdk::schemas::schemas_openxmlformats_org_wordprocessingml_2006_main::PreviousParagraphProperties {
                indentation: Some(Indentation {
                    left: Some(((level as usize + 1) * 420).to_string()),
                    hanging: Some("240".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )),
        ..Default::default()
    }
}

fn numbering_instance(number_id: i32, abstract_id: i32) -> NumberingInstance {
  NumberingInstance {
    number_id,
    abstract_num_id: Box::new(AbstractNumId { val: abstract_id }),
    ..Default::default()
  }
}

fn block_properties(
  style: Option<&str>,
  kind: ParagraphKind,
  quote_depth: usize,
  list_depth: Option<usize>,
  numbering_id: Option<i32>,
) -> ParagraphProperties {
  let mut properties = style.map_or_else(ParagraphProperties::default, paragraph_properties);

  let indent_depth = list_depth.unwrap_or(0) + quote_depth;
  if indent_depth > 0 {
    properties.indentation = Some(Indentation {
      left: Some((indent_depth * 420).to_string()),
      ..Default::default()
    });
  }

  if let (Some(depth), Some(numbering_id)) = (list_depth, numbering_id) {
    properties.numbering_properties = Some(Box::new(NumberingProperties {
      numbering_level_reference: Some(NumberingLevelReference {
        val: depth.saturating_sub(1).min(8) as i32,
      }),
      numbering_id: Some(NumberingId { val: numbering_id }),
      ..Default::default()
    }));
  }

  if matches!(kind, ParagraphKind::Code) {
    properties.shading = Some(code_shading());
  }

  properties
}

fn paragraph_style_for_kind(kind: ParagraphKind) -> Option<&'static str> {
  match kind {
    ParagraphKind::Code => Some("NoSpacing"),
    ParagraphKind::List => Some("ListParagraph"),
    ParagraphKind::Quote => Some("Quote"),
    ParagraphKind::Normal => None,
  }
}

fn text_run(text: &str, style: RunStyle) -> Run {
  Run {
    run_properties: run_properties(style).map(Box::new),
    run_choice: vec![RunChoice::WT(Box::new(Text {
      xml_content: Some(text.to_string()),
      ..Default::default()
    }))],
    ..Default::default()
  }
}

fn tab_run() -> Run {
  Run {
    run_choice: vec![RunChoice::WTab],
    ..Default::default()
  }
}

fn run_properties(style: RunStyle) -> Option<RunProperties> {
  if !(style.bold || style.italic || style.strike || style.code || style.hyperlink) {
    return None;
  }

  Some(RunProperties {
    bold: style.bold.then(Bold::default),
    italic: style.italic.then(Italic::default),
    strike: style.strike.then(Strike::default),
    run_style: style.hyperlink.then(|| WordRunStyle {
      val: "Hyperlink".to_string(),
    }),
    color: style.hyperlink.then(|| Color {
      val: "0563C1".to_string(),
      ..Default::default()
    }),
    run_fonts: style.code.then(|| RunFonts {
      ascii: Some("Consolas".to_string()),
      high_ansi: Some("Consolas".to_string()),
      east_asia: Some("Consolas".to_string()),
      complex_script: Some("Consolas".to_string()),
      ..Default::default()
    }),
    ..Default::default()
  })
}

fn code_shading() -> Shading {
  Shading {
    val: ShadingPatternValues::Clear,
    fill: Some("F6F8FA".to_string()),
    ..Default::default()
  }
}

fn table_shading() -> Shading {
  Shading {
    val: ShadingPatternValues::Clear,
    fill: Some("EDEFF3".to_string()),
    ..Default::default()
  }
}

fn heading_level(level: HeadingLevel) -> usize {
  match level {
    HeadingLevel::H1 => 1,
    HeadingLevel::H2 => 2,
    HeadingLevel::H3 => 3,
    HeadingLevel::H4 => 4,
    HeadingLevel::H5 => 5,
    HeadingLevel::H6 => 6,
  }
}
