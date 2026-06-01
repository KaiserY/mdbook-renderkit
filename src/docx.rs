use std::collections::HashMap;
use std::fs::{self, File};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use mdbook_renderer::RenderContext;
use mdbook_renderer::book::Chapter;
use ooxmlsdk::common::{XmlNamespace as XmlNamespaceDecl, XmlNamespaceUri, XmlPrefix};
use ooxmlsdk::namespaces::XmlKnownNamespace;
use ooxmlsdk::parts::document_settings_part::DocumentSettingsPart;
use ooxmlsdk::parts::main_document_part::MainDocumentPart;
use ooxmlsdk::parts::numbering_definitions_part::NumberingDefinitionsPart;
use ooxmlsdk::parts::wordprocessing_document::WordprocessingDocument;
use ooxmlsdk::schemas::opc_relationships::TargetMode;
use ooxmlsdk::schemas::schemas_openxmlformats_org_drawingml_2006_main as a;
use ooxmlsdk::schemas::schemas_openxmlformats_org_drawingml_2006_picture as pic;
use ooxmlsdk::schemas::schemas_openxmlformats_org_drawingml_2006_wordprocessing_drawing as wp;
use ooxmlsdk::schemas::schemas_openxmlformats_org_wordprocessingml_2006_main::{
  AbstractNum, AbstractNumId, Body, BodyChoice, BookmarkEnd, BookmarkStart, Break, BreakValues,
  Color, Document, Drawing, DrawingChoice, FieldChar, FieldCharValues, FieldCode, GridColumn,
  Hyperlink, HyperlinkChoice, Indentation, Justification, JustificationValues, Level,
  LevelJustification, LevelJustificationValues, LevelOverride, LevelSuffix, LevelSuffixValues,
  LevelText, MultiLevelType, MultiLevelValues, NumberFormatValues, Numbering, NumberingFormat,
  NumberingId, NumberingInstance, NumberingLevelReference, NumberingProperties, Paragraph,
  ParagraphChoice, ParagraphProperties, ParagraphStyleId, Run, RunChoice, RunProperties,
  RunPropertiesChoice, RunStyle as WordRunStyle, SectionProperties, Settings, StartNumberingValue,
  StartOverrideNumberingValue, Style as WordStyle, TabStop, TabStopLeaderCharValues, TabStopValues,
  Table, TableCell, TableCellChoice, TableCellProperties, TableCellWidth, TableChoice2, TableGrid,
  TableLayout, TableLayoutValues, TableLook, TableProperties, TableRow, TableRowChoice,
  TableRowProperties, TableRowPropertiesChoice, TableStyle, TableWidth, TableWidthUnitValues, Tabs,
  Text, TextType, Underline, UnderlineValues, UpdateFieldsOnOpen,
};
use ooxmlsdk::schemas::www_w3_org_xml_1998_namespace::SpaceProcessingModeValues;
use ooxmlsdk::sdk::{SdkPart, WordprocessingDocumentType};
use ooxmlsdk::simple_type::{
  BooleanValue, CoordinateValue, MeasurementOrPercentValue, OnOffValue, SignedTwipsMeasureValue,
  TwipsMeasureValue,
};
use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Color as SyntectColor, FontStyle as SyntectFontStyle, Style, Theme};
use syntect::parsing::SyntaxSet;

const BULLET_NUM_ID: i32 = 1;
const TASK_DONE_NUM_ID: i32 = 3;
const TASK_TODO_NUM_ID: i32 = 4;
const FIRST_ORDERED_NUM_ID: i32 = 10;
const DEFAULT_CONTENT_WIDTH_TWIPS: i64 = 8220;
const TEMPLATE_CONTENT_MARKER: &str = "MDBOOK_RENDERKIT_CONTENT";
const TEMPLATE_TITLE_MARKER: &str = "MDBOOK_RENDERKIT_TITLE";
const TEMPLATE_AUTHOR_MARKER: &str = "MDBOOK_RENDERKIT_AUTHOR";
const TEMPLATE_MARKER_PREFIX: &str = "MDBOOK_RENDERKIT_";
const RENDERKIT_TITLE_STYLE: &str = "RenderkitTitle";
const RENDERKIT_BODY_STYLE: &str = "RenderkitBody";
const RENDERKIT_TOC_HEADING_STYLE: &str = "RenderkitTocHeading";
const RENDERKIT_CODE_STYLE: &str = "RenderkitCode";
const RENDERKIT_INLINE_CODE_STYLE: &str = "RenderkitInlineCode";
const RENDERKIT_IMAGE_STYLE: &str = "RenderkitImage";
const RENDERKIT_LIST_STYLE: &str = "RenderkitList";
const RENDERKIT_QUOTE_STYLE: &str = "RenderkitQuote";
const RENDERKIT_TABLE_STYLE: &str = "RenderkitTable";
const RENDERKIT_HYPERLINK_STYLE: &str = "RenderkitHyperlink";

fn text_node(text: &str, preserve_space: bool) -> Text {
  Text(TextType {
    space: preserve_space.then_some(SpaceProcessingModeValues::Preserve),
    xml_content: Some(text.to_string()),
    ..Default::default()
  })
}

fn field_code_node(text: &str) -> FieldCode {
  FieldCode {
    space: Some(SpaceProcessingModeValues::Preserve),
    xml_content: Some(text.to_string()),
  }
}

fn pct(value: i64) -> MeasurementOrPercentValue {
  MeasurementOrPercentValue::from_bytes(value.to_string().as_bytes())
    .expect("static measurement value is valid")
}

fn twips(value: u64) -> TwipsMeasureValue {
  TwipsMeasureValue::Twips(value)
}

fn dxa(value: i64) -> TwipsMeasureValue {
  TwipsMeasureValue::Twips(value.max(0) as u64)
}

fn signed_twips(value: i64) -> SignedTwipsMeasureValue {
  SignedTwipsMeasureValue::Twips(value)
}

fn twips_to_emu(value: i64) -> i64 {
  value.saturating_mul(635)
}

#[derive(Debug, serde::Deserialize)]
#[serde(default, rename_all = "kebab-case")]
struct DocxConfig {
  template: Option<PathBuf>,
  section_number: bool,
  code_highlight: bool,
  code_theme: String,
}

impl Default for DocxConfig {
  fn default() -> Self {
    Self {
      template: None,
      section_number: false,
      code_highlight: true,
      code_theme: "InspiredGitHub".to_string(),
    }
  }
}

#[derive(Clone, Debug)]
struct DocxStyles {
  headings: [Option<String>; 9],
  toc_entries: [Option<String>; 9],
  title: Option<String>,
  body: Option<String>,
  content_width_twips: i64,
  image_max_width_emu: i64,
  code: Option<String>,
  inline_code: Option<String>,
  image: Option<String>,
  list: Option<String>,
  quote: Option<String>,
  table: Option<String>,
  hyperlink: Option<String>,
}

struct DocxPackageParts<'a> {
  main_part: &'a MainDocumentPart,
  numbering_part: &'a NumberingDefinitionsPart,
  styles: &'a DocxStyles,
  template: Option<TemplateProfile>,
}

#[derive(Clone, Debug, Default)]
struct TemplateProfile {
  body_choices: Vec<BodyChoice>,
  content_index: usize,
  toc_index: Option<usize>,
  body_section_properties: Option<Box<SectionProperties>>,
}

impl Default for DocxStyles {
  fn default() -> Self {
    Self {
      headings: std::array::from_fn(|_| None),
      toc_entries: std::array::from_fn(|_| None),
      title: None,
      body: None,
      content_width_twips: DEFAULT_CONTENT_WIDTH_TWIPS,
      image_max_width_emu: twips_to_emu(DEFAULT_CONTENT_WIDTH_TWIPS),
      code: None,
      inline_code: None,
      image: None,
      list: None,
      quote: None,
      table: None,
      hyperlink: None,
    }
  }
}

impl DocxStyles {
  fn heading(&self, level: usize) -> Option<&str> {
    self.headings[level.clamp(1, 9) - 1].as_deref()
  }

  fn toc_entry(&self, level: usize) -> Option<&str> {
    self.toc_entries[level.clamp(1, 9) - 1].as_deref()
  }

  fn block_style(&self, kind: ParagraphKind) -> Option<&str> {
    match kind {
      ParagraphKind::Code => self.code.as_deref(),
      ParagraphKind::List => self.list.as_deref(),
      ParagraphKind::Quote => self.quote.as_deref(),
      ParagraphKind::Normal => self.body.as_deref(),
      ParagraphKind::Heading => None,
    }
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
  let cfg = load_config(ctx)?;
  if let Some(template) = &cfg.template {
    write_docx_from_template(ctx, &cfg, template, path)
  } else {
    write_docx_from_scratch(ctx, &cfg, path)
  }
}

fn write_docx_from_scratch(ctx: &RenderContext, cfg: &DocxConfig, path: &Path) -> Result<()> {
  let mut package = WordprocessingDocument::create(WordprocessingDocumentType::Document);
  let main_part = package.add_main_document_part()?;
  let numbering_part = ensure_numbering_part(&mut package, &main_part)?;
  let styles = DocxStyles::default();
  write_docx_package(
    ctx,
    cfg,
    path,
    &mut package,
    DocxPackageParts {
      main_part: &main_part,
      numbering_part: &numbering_part,
      styles: &styles,
      template: None,
    },
  )
}

fn write_docx_from_template(
  ctx: &RenderContext,
  cfg: &DocxConfig,
  template: &Path,
  path: &Path,
) -> Result<()> {
  let template_path = ctx.root.join(template);
  eprintln!("renderkit: using DOCX template {}", template_path.display());

  let mut package = WordprocessingDocument::create_from_template(&template_path)
    .with_context(|| format!("failed to read DOCX template {}", template_path.display()))?;
  let main_part = package.main_document_part()?;
  let numbering_part = ensure_numbering_part(&mut package, &main_part)?;
  let (styles, template_profile) = template_profile(ctx, &mut package, &main_part)?;

  write_docx_package(
    ctx,
    cfg,
    path,
    &mut package,
    DocxPackageParts {
      main_part: &main_part,
      numbering_part: &numbering_part,
      styles: &styles,
      template: Some(template_profile),
    },
  )
}

fn write_docx_package(
  ctx: &RenderContext,
  cfg: &DocxConfig,
  path: &Path,
  package: &mut WordprocessingDocument,
  parts: DocxPackageParts<'_>,
) -> Result<()> {
  let (document, ordered_lists) = render_document(
    ctx,
    cfg,
    package,
    parts.main_part,
    parts.styles,
    parts.template,
  )?;
  parts.main_part.set_root_element(package, document)?;
  parts
    .numbering_part
    .set_root_element(package, numbering_definitions(&ordered_lists))?;
  enable_update_fields_on_open(package, parts.main_part)?;

  let file = File::create(path).with_context(|| format!("failed to create {}", path.display()))?;
  package.save(file)?;
  Ok(())
}

fn render_document(
  ctx: &RenderContext,
  cfg: &DocxConfig,
  package: &mut WordprocessingDocument,
  main_part: &MainDocumentPart,
  styles: &DocxStyles,
  template: Option<TemplateProfile>,
) -> Result<(Document, Vec<OrderedListDefinition>)> {
  let mut docx = DocxRenderContext::new(ctx, package, main_part, cfg, styles.clone());
  let document = document(ctx, cfg, &mut docx, template)?;
  Ok((document, docx.ordered_lists))
}

fn ensure_numbering_part(
  package: &mut WordprocessingDocument,
  main_part: &MainDocumentPart,
) -> Result<NumberingDefinitionsPart> {
  if let Some(numbering_part) = main_part.numbering_definitions_part(package) {
    return Ok(numbering_part);
  }

  let numbering_part = package.add_new_part_auto_id::<NumberingDefinitionsPart>()?;
  Ok(main_part.add_part(package, numbering_part)?)
}

fn ensure_settings_part(
  package: &mut WordprocessingDocument,
  main_part: &MainDocumentPart,
) -> Result<DocumentSettingsPart> {
  if let Some(settings_part) = main_part.document_settings_part(package) {
    return Ok(settings_part);
  }

  let settings_part = package.add_new_part_auto_id::<DocumentSettingsPart>()?;
  Ok(main_part.add_part(package, settings_part)?)
}

fn enable_update_fields_on_open(
  package: &mut WordprocessingDocument,
  main_part: &MainDocumentPart,
) -> Result<()> {
  let settings_part = ensure_settings_part(package, main_part)?;
  let mut settings = settings_part
    .root_element(package)
    .cloned()
    .unwrap_or_else(|_| Settings::default());
  settings.update_fields_on_open = Some(UpdateFieldsOnOpen {
    val: Some(OnOffValue::True),
  });
  settings_part.set_root_element(package, settings)?;
  Ok(())
}

fn resolve_template_docx_styles(
  package: &mut WordprocessingDocument,
  main_part: &MainDocumentPart,
) -> DocxStyles {
  let mut styles = DocxStyles::default();
  let Some(style_part) = main_part.style_definitions_part(package) else {
    eprintln!(
      "renderkit: warning: DOCX template missing styles.xml; generated content will omit styles"
    );
    return styles;
  };
  let Ok(root) = style_part.root_element(package) else {
    eprintln!(
      "renderkit: warning: failed to read DOCX template styles.xml; generated content will omit styles"
    );
    return styles;
  };
  let style_by_id = root
    .style
    .iter()
    .filter_map(|style| {
      style
        .style_id
        .as_ref()
        .map(|style_id| (style_id.clone(), style))
    })
    .collect::<HashMap<_, _>>();

  styles.title = renderkit_style_id(&style_by_id, RENDERKIT_TITLE_STYLE);
  styles.body = renderkit_style_id(&style_by_id, RENDERKIT_BODY_STYLE);
  styles.code = renderkit_style_id(&style_by_id, RENDERKIT_CODE_STYLE);
  styles.inline_code = renderkit_style_id(&style_by_id, RENDERKIT_INLINE_CODE_STYLE);
  styles.image = renderkit_style_id(&style_by_id, RENDERKIT_IMAGE_STYLE);
  styles.list = renderkit_style_id(&style_by_id, RENDERKIT_LIST_STYLE);
  styles.quote = renderkit_style_id(&style_by_id, RENDERKIT_QUOTE_STYLE);
  styles.table = renderkit_style_id(&style_by_id, RENDERKIT_TABLE_STYLE);
  styles.hyperlink = renderkit_style_id(&style_by_id, RENDERKIT_HYPERLINK_STYLE);
  for level in 1..=9 {
    styles.headings[level - 1] =
      renderkit_style_id(&style_by_id, &format!("RenderkitHeading{level}"));
    styles.toc_entries[level - 1] =
      renderkit_style_id(&style_by_id, &format!("RenderkitToc{level}"));
  }
  warn_missing_renderkit_style(&style_by_id, RENDERKIT_TOC_HEADING_STYLE);
  styles
}

fn renderkit_style_id(style_by_id: &HashMap<String, &WordStyle>, style_id: &str) -> Option<String> {
  if style_by_id.contains_key(style_id) {
    Some(style_id.to_string())
  } else {
    warn_missing_renderkit_style(style_by_id, style_id);
    None
  }
}

fn warn_missing_renderkit_style(style_by_id: &HashMap<String, &WordStyle>, style_id: &str) {
  if !style_by_id.contains_key(style_id) {
    eprintln!(
      "renderkit: warning: DOCX template missing style {style_id}; generated content will omit it"
    );
  }
}

fn template_profile(
  ctx: &RenderContext,
  package: &mut WordprocessingDocument,
  main_part: &MainDocumentPart,
) -> Result<(DocxStyles, TemplateProfile)> {
  let mut styles = resolve_template_docx_styles(package, main_part);
  let document = main_part.root_element(package)?;
  let Some(body) = document.body.as_ref() else {
    bail!("DOCX template has no document body");
  };

  let body_section_properties = body.section_properties.clone();
  let paragraph_section_properties = body.body_choice.iter().find_map(|choice| {
    let BodyChoice::Paragraph(paragraph) = choice else {
      return None;
    };
    paragraph
      .paragraph_properties
      .as_ref()
      .and_then(|properties| properties.section_properties.as_ref())
      .filter(|properties| !properties.section_properties_choice.is_empty())
      .cloned()
  });

  let body_section_properties = paragraph_section_properties
    .clone()
    .or(body_section_properties);
  if let Some(content_width) = section_content_width_twips(body_section_properties.as_deref()) {
    styles.content_width_twips = content_width;
    styles.image_max_width_emu = twips_to_emu(content_width);
  }

  let mut body_choices = body.body_choice.clone();
  let toc_index = toc_field_index(&body_choices);
  let content_index = apply_template_markers(ctx, &mut body_choices)?;
  Ok((
    styles,
    TemplateProfile {
      body_choices,
      content_index,
      toc_index,
      body_section_properties,
    },
  ))
}

fn toc_field_index(body_choices: &[BodyChoice]) -> Option<usize> {
  body_choices
    .iter()
    .position(|choice| matches!(choice, BodyChoice::Paragraph(paragraph) if paragraph_has_toc_field(paragraph)))
}

fn paragraph_has_toc_field(paragraph: &Paragraph) -> bool {
  paragraph.paragraph_choice.iter().any(|choice| {
    let ParagraphChoice::WRun(run) = choice else {
      return false;
    };
    run.run_choice.iter().any(|choice| {
      matches!(
        choice,
        RunChoice::FieldCode(field_code)
          if field_code
            .xml_content
            .as_deref()
            .is_some_and(|content| content.contains("TOC"))
      )
    })
  })
}

fn wordprocessing_xmlns() -> Vec<XmlNamespaceDecl> {
  vec![known_xmlns(XmlKnownNamespace::W)]
}

fn section_content_width_twips(section_properties: Option<&SectionProperties>) -> Option<i64> {
  let section_properties = section_properties?;
  let page_width = section_properties
    .page_size
    .as_ref()?
    .width
    .map(TwipsMeasureValue::to_twips)?;
  let page_margin = section_properties.page_margin.as_ref()?;
  let left = page_margin.left.map(TwipsMeasureValue::to_twips)?;
  let right = page_margin.right.map(TwipsMeasureValue::to_twips)?;
  let gutter = page_margin
    .gutter
    .map(TwipsMeasureValue::to_twips)
    .unwrap_or(0);
  Some((page_width - left - right - gutter).max(1))
}

fn apply_template_markers(ctx: &RenderContext, body_choices: &mut [BodyChoice]) -> Result<usize> {
  let title = ctx.config.book.title.as_deref().unwrap_or("book");
  let author = ctx.config.book.authors.join(" ");
  let mut content_index = None;
  let mut title_count = 0usize;
  let mut author_count = 0usize;

  for (body_index, choice) in body_choices.iter_mut().enumerate() {
    let BodyChoice::Paragraph(paragraph) = choice else {
      continue;
    };
    for paragraph_choice in &mut paragraph.paragraph_choice {
      let ParagraphChoice::WRun(run) = paragraph_choice else {
        continue;
      };
      for run_choice in &mut run.run_choice {
        let RunChoice::Text(text) = run_choice else {
          continue;
        };
        let Some(content) = text.0.xml_content.as_deref() else {
          continue;
        };
        match content {
          TEMPLATE_CONTENT_MARKER if content_index.replace(body_index).is_some() => {
            bail!("DOCX template marker {TEMPLATE_CONTENT_MARKER} appears more than once");
          }
          TEMPLATE_CONTENT_MARKER => {}
          TEMPLATE_TITLE_MARKER => {
            title_count += 1;
            if title_count > 1 {
              bail!("DOCX template marker {TEMPLATE_TITLE_MARKER} appears more than once");
            }
            text.0.xml_content = Some(title.to_string());
            text.0.space =
              text_needs_preserve(title).then_some(SpaceProcessingModeValues::Preserve);
          }
          TEMPLATE_AUTHOR_MARKER => {
            author_count += 1;
            if author_count > 1 {
              bail!("DOCX template marker {TEMPLATE_AUTHOR_MARKER} appears more than once");
            }
            text.0.xml_content = Some(author.clone());
            text.0.space =
              text_needs_preserve(&author).then_some(SpaceProcessingModeValues::Preserve);
          }
          other if other.contains(TEMPLATE_MARKER_PREFIX) => {
            bail!("DOCX template marker must be a complete text run without extra text: {other}");
          }
          _ => {}
        }
      }
    }
  }

  let Some(content_index) = content_index else {
    bail!("DOCX template missing required marker {TEMPLATE_CONTENT_MARKER}");
  };
  if title_count == 0 {
    bail!("DOCX template missing required marker {TEMPLATE_TITLE_MARKER}");
  }
  if author_count == 0 {
    bail!("DOCX template missing required marker {TEMPLATE_AUTHOR_MARKER}");
  }

  Ok(content_index)
}

fn load_config(ctx: &RenderContext) -> Result<DocxConfig> {
  Ok(ctx.config.get("output.docx")?.unwrap_or_default())
}

fn document(
  ctx: &RenderContext,
  cfg: &DocxConfig,
  docx: &mut DocxRenderContext<'_>,
  template: Option<TemplateProfile>,
) -> Result<Document> {
  let body_section_properties = template
    .as_ref()
    .and_then(|template| template.body_section_properties.clone());
  if let Some(template) = template {
    let generated = generated_content_choices(ctx, cfg, docx)?;
    let toc = template
      .toc_index
      .map(|_| toc_entries(ctx, cfg, docx))
      .unwrap_or_default();
    let mut body = Body {
      section_properties: body_section_properties,
      ..Default::default()
    };
    let mut generated = Some(generated);
    let mut toc = Some(toc);
    for (index, choice) in template.body_choices.into_iter().enumerate() {
      if index == template.content_index {
        body
          .body_choice
          .extend(generated.take().expect("content marker is processed once"));
      } else if Some(index) == template.toc_index {
        body
          .body_choice
          .extend(toc.take().expect("TOC field is processed once"));
      } else {
        body.body_choice.push(choice);
      }
    }
    return Ok(Document {
      xmlns: document_xmlns(),
      body: Some(Box::new(body)),
      ..Default::default()
    });
  }

  let mut body = Body {
    section_properties: body_section_properties,
    ..Default::default()
  };

  if let Some(title) = &ctx.config.book.title {
    body
      .body_choice
      .push(BodyChoice::Paragraph(Box::new(cover_title_paragraph(
        title,
        &docx.styles,
      ))));
    body
      .body_choice
      .push(BodyChoice::Paragraph(Box::new(page_break_paragraph())));
  }

  body
    .body_choice
    .extend(generated_content_choices(ctx, cfg, docx)?);

  Ok(Document {
    xmlns: document_xmlns(),
    body: Some(Box::new(body)),
    ..Default::default()
  })
}

fn generated_content_choices(
  ctx: &RenderContext,
  cfg: &DocxConfig,
  docx: &mut DocxRenderContext<'_>,
) -> Result<Vec<BodyChoice>> {
  let mut out = Vec::new();

  let mut chapters = ctx.book.chapters().peekable();
  while let Some(chapter) = chapters.next() {
    let before = out.len();
    out.extend(chapter_body(cfg, docx, chapter)?);
    if out.len() == before {
      let bookmark = docx.chapter_bookmark(chapter);
      out.push(BodyChoice::Paragraph(Box::new(
        heading_paragraph_with_bookmark(
          chapter_level(chapter),
          &chapter.name,
          bookmark,
          &docx.styles,
        ),
      )));
    }
    if chapters.peek().is_some() {
      out.push(BodyChoice::Paragraph(Box::new(page_break_paragraph())));
    }
  }

  Ok(out)
}

fn toc_entries(
  ctx: &RenderContext,
  cfg: &DocxConfig,
  docx: &mut DocxRenderContext<'_>,
) -> Vec<BodyChoice> {
  ctx
    .book
    .chapters()
    .filter_map(|chapter| toc_entry_paragraph(cfg, docx, chapter))
    .map(|paragraph| BodyChoice::Paragraph(Box::new(paragraph)))
    .collect()
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
    paragraph_properties: Some(Box::new(toc_entry_properties(level, &docx.styles))),
    paragraph_choice: toc_entry_choices(&title, &bookmark),
    ..Default::default()
  })
}

fn toc_entry_properties(level: usize, styles: &DocxStyles) -> ParagraphProperties {
  let mut properties = styles
    .toc_entry(level)
    .map_or_else(ParagraphProperties::default, paragraph_properties);
  properties.tabs = Some(Tabs {
    tab_stop: vec![TabStop {
      val: TabStopValues::Right,
      leader: Some(TabStopLeaderCharValues::Dot),
      position: signed_twips(styles.content_width_twips),
    }],
  });
  properties
}

fn toc_entry_choices(title: &str, bookmark: &str) -> Vec<ParagraphChoice> {
  vec![
    ParagraphChoice::WRun(Box::new(toc_field_char_run(FieldCharValues::Begin, true))),
    ParagraphChoice::WRun(Box::new(toc_field_code_run(&format!(
      r#" HYPERLINK \l {bookmark} "#
    )))),
    ParagraphChoice::WRun(Box::new(toc_field_char_run(
      FieldCharValues::Separate,
      false,
    ))),
    ParagraphChoice::WRun(Box::new(Run {
      run_choice: vec![RunChoice::Text(Box::new(text_node(
        title,
        text_needs_preserve(title),
      )))],
      ..Default::default()
    })),
    ParagraphChoice::WRun(Box::new(Run {
      run_choice: vec![RunChoice::TabChar],
      ..Default::default()
    })),
    ParagraphChoice::WRun(Box::new(toc_field_char_run(FieldCharValues::Begin, true))),
    ParagraphChoice::WRun(Box::new(toc_field_code_run(&format!(
      r#" PAGEREF {bookmark} \h "#
    )))),
    ParagraphChoice::WRun(Box::new(toc_field_char_run(
      FieldCharValues::Separate,
      false,
    ))),
    ParagraphChoice::WRun(Box::new(toc_field_char_run(FieldCharValues::End, false))),
    ParagraphChoice::WRun(Box::new(toc_field_char_run(FieldCharValues::End, false))),
  ]
}

fn toc_field_char_run(field_char_type: FieldCharValues, dirty: bool) -> Run {
  Run {
    run_choice: vec![RunChoice::FieldChar(Box::new(FieldChar {
      field_char_type,
      dirty: dirty.then_some(OnOffValue::True),
      ..Default::default()
    }))],
    ..Default::default()
  }
}

fn toc_field_code_run(instruction: &str) -> Run {
  Run {
    run_choice: vec![RunChoice::FieldCode(Box::new(field_code_node(instruction)))],
    ..Default::default()
  }
}

fn document_xmlns() -> Vec<XmlNamespaceDecl> {
  vec![
    known_xmlns(XmlKnownNamespace::W),
    known_xmlns(XmlKnownNamespace::R),
    known_xmlns(XmlKnownNamespace::Wp),
    known_xmlns(XmlKnownNamespace::A),
    known_xmlns(XmlKnownNamespace::Pic),
    known_xmlns(XmlKnownNamespace::W14),
    known_xmlns(XmlKnownNamespace::W15),
    known_xmlns(XmlKnownNamespace::Wp14),
  ]
}

fn known_xmlns(namespace: XmlKnownNamespace) -> XmlNamespaceDecl {
  XmlNamespaceDecl {
    prefix: XmlPrefix::new(namespace.prefix_bytes()),
    uri: XmlNamespaceUri::Known(namespace),
  }
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
  let mut image: Option<(String, String)> = None;
  let mut lists: Vec<ListState> = Vec::new();
  let mut quote_depth = 0usize;
  let mut table: Option<TableBuilder> = None;
  let mut chapter_bookmark = docx.chapter_bookmark(chapter);

  for event in parser {
    if let Some((_, code)) = &mut code_block {
      match event {
        Event::End(TagEnd::CodeBlock) => {
          let (lang, code) = code_block.take().expect("checked above");
          push_code_block(&mut out, &mut table, docx, &lang, &code)?;
        }
        Event::Text(text) => code.push_str(&text),
        Event::SoftBreak | Event::HardBreak => code.push('\n'),
        _ => {}
      }
      continue;
    }

    if let Some((_dest_url, alt)) = &mut image {
      match event {
        Event::End(TagEnd::Image) => {
          let (dest_url, alt) = image.take().expect("checked above");
          if let Some(run) = docx.image_run(chapter, &dest_url, &alt)? {
            if paragraph.has_content() {
              paragraph.flush_to(&mut out, &mut table);
              paragraph = ParagraphBuilder::for_block(ParagraphKind::Normal, 0, None, &docx.styles);
            }
            paragraph.push_run_node(run);
          } else {
            paragraph.push_text("[image: ");
            paragraph.push_text(&dest_url);
            paragraph.push_text("]");
          }
        }
        Event::Text(text) | Event::Code(text) => alt.push_str(&text),
        Event::SoftBreak | Event::HardBreak => alt.push(' '),
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
        paragraph = ParagraphBuilder::for_block(kind, quote_depth, None, &docx.styles)
      }
      Event::End(TagEnd::Paragraph) => paragraph.flush_to(&mut out, &mut table),
      Event::Start(Tag::Heading { level, .. }) => {
        let markdown_level = heading_level(level);
        let level = if cfg.section_number {
          chapter_level(chapter) + markdown_level - 1
        } else {
          markdown_level
        };
        paragraph = ParagraphBuilder::with_heading(level, &docx.styles);
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
      Event::Start(Tag::List(start)) => {
        let level = lists.len();
        lists.push(ListState::new(start, level, docx));
      }
      Event::End(TagEnd::List(_)) => {
        lists.pop();
      }
      Event::Start(Tag::Item) => {
        let depth = lists.len().max(1);
        let marker = lists
          .last_mut()
          .map(|list| list.numbering_id())
          .unwrap_or(BULLET_NUM_ID);
        paragraph =
          ParagraphBuilder::for_block(ParagraphKind::List, quote_depth, Some(depth), &docx.styles);
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
      Event::Start(Tag::Table(alignments)) => {
        table = Some(TableBuilder::new(&docx.styles, alignments.to_vec()));
      }
      Event::End(TagEnd::Table) => {
        if let Some(table) = table.take() {
          out.push(BodyChoice::Table(Box::new(table.into_table())));
        }
      }
      Event::Start(Tag::TableHead) => {
        if let Some(table) = &mut table {
          table.in_head = true;
          table.start_row();
        }
      }
      Event::End(TagEnd::TableHead) => {
        if let Some(table) = &mut table {
          table.end_row();
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
        paragraph = ParagraphBuilder::for_block(ParagraphKind::Normal, 0, None, &docx.styles);
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
        image = Some((dest_url.to_string(), String::new()));
      }
      Event::Code(text) => {
        paragraph.push_inline_code(
          &text,
          RunStyle {
            inline_code: true,
            ..paragraph.current_run_style()
          },
        );
      }
      Event::Text(text) => paragraph.push_text(&text),
      Event::SoftBreak | Event::HardBreak => paragraph.push_text("\n"),
      Event::Rule => {
        paragraph.flush_to(&mut out, &mut table);
        out.push(BodyChoice::Paragraph(Box::new(paragraph_from_text(
          "----------------------------------------",
          RunStyle::default(),
          &docx.styles,
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
  source_dir: PathBuf,
  package: &'a mut WordprocessingDocument,
  main_part: &'a MainDocumentPart,
  styles: DocxStyles,
  highlighter: CodeHighlighter,
  ordered_lists: Vec<OrderedListDefinition>,
  hyperlinks: HashMap<String, String>,
  images: HashMap<PathBuf, ImageRef>,
  bookmarks: HashMap<PathBuf, String>,
  bookmark_ids: HashMap<String, String>,
  next_bookmark_id: usize,
  next_drawing_id: u32,
  next_numbering_id: i32,
}

impl<'a> DocxRenderContext<'a> {
  fn new(
    ctx: &RenderContext,
    package: &'a mut WordprocessingDocument,
    main_part: &'a MainDocumentPart,
    cfg: &DocxConfig,
    styles: DocxStyles,
  ) -> Self {
    Self {
      source_dir: ctx.source_dir(),
      package,
      main_part,
      styles,
      highlighter: CodeHighlighter::new(cfg),
      ordered_lists: Vec::new(),
      hyperlinks: HashMap::new(),
      images: HashMap::new(),
      bookmarks: HashMap::new(),
      bookmark_ids: HashMap::new(),
      next_bookmark_id: 1,
      next_drawing_id: 1,
      next_numbering_id: FIRST_ORDERED_NUM_ID,
    }
  }

  fn ordered_numbering_id(&mut self, level: usize, start: u64) -> i32 {
    let number_id = self.next_numbering_id;
    self.next_numbering_id += 1;
    self.ordered_lists.push(OrderedListDefinition {
      number_id,
      level,
      start,
    });
    number_id
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

  fn image_run(&mut self, chapter: &Chapter, url: &str, alt: &str) -> Result<Option<Run>> {
    if is_remote_url(url) {
      return Ok(None);
    }
    let Some(image_path) = local_asset_path(chapter, &self.source_dir, url) else {
      return Ok(None);
    };
    if !image_path.exists() {
      return Ok(None);
    }

    let image = if let Some(image) = self.images.get(&image_path) {
      image.clone()
    } else {
      let data = fs::read(&image_path)
        .with_context(|| format!("failed to read image {}", image_path.display()))?;
      let Some((width, height)) = image_dimensions(&data) else {
        return Ok(None);
      };
      let Some(content_type) = image_content_type(&image_path, &data) else {
        return Ok(None);
      };
      let part = self.main_part.add_image_part(self.package, content_type)?;
      part.set_data(self.package, data)?;
      let relationship_id = part
        .relationship_id()
        .context("image part missing relationship id")?
        .to_string();
      let image = ImageRef {
        relationship_id,
        width,
        height,
        max_width_emu: self.styles.image_max_width_emu,
        alt: alt.to_string(),
      };
      self.images.insert(image_path, image.clone());
      image
    };

    let drawing_id = self.next_drawing_id;
    self.next_drawing_id += 1;
    Ok(Some(Run {
      run_choice: vec![RunChoice::Drawing(Box::new(image_drawing(
        drawing_id, &image,
      )))],
      ..Default::default()
    }))
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
struct OrderedListDefinition {
  number_id: i32,
  level: usize,
  start: u64,
}

struct CodeHighlighter {
  enabled: bool,
  syntax_set: SyntaxSet,
  theme: Theme,
}

impl CodeHighlighter {
  fn new(cfg: &DocxConfig) -> Self {
    let syntax_set = SyntaxSet::load_defaults_newlines();
    let theme_set = syntect::highlighting::ThemeSet::load_defaults();
    let theme = theme_set
      .themes
      .get(&cfg.code_theme)
      .or_else(|| theme_set.themes.get("InspiredGitHub"))
      .or_else(|| theme_set.themes.values().next())
      .cloned()
      .unwrap_or_default();

    Self {
      enabled: cfg.code_highlight,
      syntax_set,
      theme,
    }
  }

  fn lines(&self, lang: &str, code: &str) -> Result<Vec<Vec<CodeRun>>> {
    let lang = lang.split([',', ' ']).next().unwrap_or_default().trim();
    let Some(syntax) = self.syntax_set.find_syntax_by_token(lang) else {
      return Ok(plain_code_lines(code));
    };
    if !self.enabled || lang.is_empty() {
      return Ok(plain_code_lines(code));
    }

    let mut highlighter = HighlightLines::new(syntax, &self.theme);
    code_lines(code)
      .into_iter()
      .map(|line| {
        let ranges = highlighter.highlight_line(line, &self.syntax_set)?;
        Ok(
          ranges
            .into_iter()
            .map(|(style, content)| CodeRun {
              text: content.to_string(),
              style: Some(style.into()),
            })
            .collect(),
        )
      })
      .collect()
  }
}

#[derive(Clone, Debug)]
struct CodeRun {
  text: String,
  style: Option<CodeRunStyle>,
}

#[derive(Clone, Debug, Default)]
struct CodeRunStyle {
  color: Option<String>,
  bold: bool,
  italic: bool,
  underline: bool,
}

impl From<Style> for CodeRunStyle {
  fn from(style: Style) -> Self {
    Self {
      color: Some(color_to_hex(style.foreground)),
      bold: style.font_style.contains(SyntectFontStyle::BOLD),
      italic: style.font_style.contains(SyntectFontStyle::ITALIC),
      underline: style.font_style.contains(SyntectFontStyle::UNDERLINE),
    }
  }
}

#[derive(Clone, Debug)]
struct BookmarkRef {
  id: String,
  name: String,
}

#[derive(Clone, Debug)]
struct ImageRef {
  relationship_id: String,
  width: u32,
  height: u32,
  max_width_emu: i64,
  alt: String,
}

#[derive(Clone, Debug)]
struct ListState {
  numbering_id: i32,
}

impl ListState {
  fn new(start: Option<u64>, level: usize, docx: &mut DocxRenderContext<'_>) -> Self {
    let numbering_id = start.map_or(BULLET_NUM_ID, |start| {
      docx.ordered_numbering_id(level, start)
    });
    Self { numbering_id }
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
  Heading,
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
  inline_code: bool,
  hyperlink: bool,
}

#[derive(Debug, Default)]
struct ParagraphBuilder {
  runs: Vec<ParagraphChoice>,
  style: StyleDepth,
  paragraph_style: Option<String>,
  styles: DocxStyles,
  kind: ParagraphKind,
  quote_depth: usize,
  list_depth: Option<usize>,
  list_numbering_id: Option<i32>,
  link_stack: Vec<LinkTarget>,
  bookmark: Option<BookmarkRef>,
  has_image: bool,
}

impl ParagraphBuilder {
  fn with_heading(level: usize, styles: &DocxStyles) -> Self {
    Self {
      paragraph_style: styles.heading(level).map(str::to_string),
      styles: styles.clone(),
      kind: ParagraphKind::Heading,
      ..Default::default()
    }
  }

  fn for_block(
    kind: ParagraphKind,
    quote_depth: usize,
    list_depth: Option<usize>,
    styles: &DocxStyles,
  ) -> Self {
    Self {
      paragraph_style: styles.block_style(kind).map(str::to_string),
      styles: styles.clone(),
      kind,
      quote_depth,
      list_depth,
      ..Default::default()
    }
  }

  fn push_text(&mut self, text: &str) {
    self.push_run(text, self.current_run_style());
  }

  fn push_inline_code(&mut self, text: &str, style: RunStyle) {
    self.push_run(text, style);
  }

  fn push_run_node(&mut self, run: Run) {
    if run
      .run_choice
      .iter()
      .any(|choice| matches!(choice, RunChoice::Drawing(_)))
    {
      self.has_image = true;
    }
    self.runs.push(ParagraphChoice::WRun(Box::new(run)));
  }

  fn has_content(&self) -> bool {
    !self.runs.is_empty()
  }

  fn current_run_style(&self) -> RunStyle {
    RunStyle {
      bold: self.style.bold > 0,
      italic: self.style.italic > 0,
      strike: self.style.strike > 0,
      inline_code: false,
      hyperlink: !self.link_stack.is_empty(),
    }
  }

  fn push_run(&mut self, text: &str, style: RunStyle) {
    if text.is_empty() {
      return;
    }

    for (index, line) in text.split('\n').enumerate() {
      if index > 0 {
        self.runs.push(ParagraphChoice::WRun(Box::new(Run {
          run_choice: vec![RunChoice::Break(Box::default())],
          ..Default::default()
        })));
      }
      if !line.is_empty() {
        self.push_text_run(line, style);
      }
    }
  }

  fn push_text_run(&mut self, text: &str, style: RunStyle) {
    let run = text_run(text, style, &self.styles);
    if let Some(link) = self.link_stack.last() {
      self
        .runs
        .push(ParagraphChoice::Hyperlink(Box::new(hyperlink(link, run))));
    } else {
      self.runs.push(ParagraphChoice::WRun(Box::new(run)));
    }
  }

  fn flush_to(&mut self, out: &mut Vec<BodyChoice>, table: &mut Option<TableBuilder>) {
    if self.runs.is_empty() {
      return;
    }

    let mut paragraph_choice = Vec::new();
    if let Some(bookmark) = self.bookmark.take() {
      paragraph_choice.push(ParagraphChoice::BookmarkStart(Box::new(BookmarkStart {
        name: bookmark.name,
        id: bookmark.id.clone(),
        ..Default::default()
      })));
      paragraph_choice.append(&mut self.runs);
      paragraph_choice.push(ParagraphChoice::BookmarkEnd(Box::new(BookmarkEnd {
        id: bookmark.id,
        ..Default::default()
      })));
    } else {
      paragraph_choice.append(&mut self.runs);
    }

    let paragraph_style = if self.has_image {
      self.styles.image.as_deref()
    } else {
      self.paragraph_style.as_deref()
    };
    let paragraph = Paragraph {
      paragraph_properties: Some(Box::new(block_properties(
        paragraph_style,
        self.kind,
        self.quote_depth,
        self.list_depth,
        self.list_numbering_id,
        self.has_image,
      ))),
      paragraph_choice,
      ..Default::default()
    };

    push_paragraph(out, table, paragraph);
  }
}

#[derive(Debug, Default)]
struct TableBuilder {
  table_style: Option<String>,
  table_width: i64,
  alignments: Vec<Alignment>,
  rows: Vec<TableRowData>,
  current_row: Vec<TableCellData>,
  current_cell: Vec<Paragraph>,
  in_head: bool,
  current_head: bool,
  current_cell_head: bool,
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
  fn new(styles: &DocxStyles, alignments: Vec<Alignment>) -> Self {
    Self {
      table_style: styles.table.clone(),
      table_width: styles.content_width_twips,
      alignments,
      ..Default::default()
    }
  }

  fn start_row(&mut self) {
    if !self.current_row.is_empty() || !self.current_cell.is_empty() {
      self.end_row();
    }
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
    self.current_cell_head = self.in_head;
    self.current_head |= self.current_cell_head;
  }

  fn end_cell(&mut self) {
    if self.current_cell.is_empty() {
      self.current_cell.push(Paragraph::default());
    }
    self.current_row.push(TableCellData {
      paragraphs: std::mem::take(&mut self.current_cell),
      is_head: self.current_cell_head,
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
    let table_width = self.table_width.max(1);
    let cell_width = table_width / column_count as i64;
    let alignments = self.alignments;

    Table {
      table_properties: Box::new(TableProperties {
        table_style: self.table_style.map(|val| TableStyle { val }),
        table_width: Some(TableWidth {
          width: Some(pct(table_width)),
          r#type: Some(TableWidthUnitValues::Dxa),
        }),
        table_layout: Some(TableLayout {
          r#type: Some(TableLayoutValues::Fixed),
        }),
        table_look: Some(TableLook {
          first_row: Some(OnOffValue::True),
          no_horizontal_band: Some(OnOffValue::True),
          no_vertical_band: Some(OnOffValue::True),
          ..Default::default()
        }),
        ..Default::default()
      }),
      table_grid: Some(Box::new(TableGrid {
        grid_column: (0..column_count)
          .map(|_| GridColumn {
            width: Some(dxa(cell_width)),
          })
          .collect(),
        ..Default::default()
      })),
      table_choice2: self
        .rows
        .into_iter()
        .map(|row| {
          TableChoice2::TableRow(Box::new(TableRow {
            table_row_properties: Some(Box::new(table_row_properties(row.is_head))),
            table_row_choice: row
              .cells
              .into_iter()
              .enumerate()
              .map(|(index, cell)| {
                let is_head = cell.is_head || row.is_head;
                TableRowChoice::TableCell(Box::new(TableCell {
                  table_cell_properties: Some(Box::new(TableCellProperties {
                    table_cell_width: Some(TableCellWidth {
                      width: Some(pct(cell_width)),
                      r#type: Some(TableWidthUnitValues::Dxa),
                    }),
                    ..Default::default()
                  })),
                  table_cell_choice: table_cell_paragraphs(
                    cell.paragraphs,
                    is_head,
                    alignments.get(index).copied().unwrap_or(Alignment::None),
                  )
                  .into_iter()
                  .map(|paragraph| TableCellChoice::Paragraph(Box::new(paragraph)))
                  .collect(),
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

fn table_cell_paragraphs(
  paragraphs: Vec<Paragraph>,
  _is_head: bool,
  alignment: Alignment,
) -> Vec<Paragraph> {
  paragraphs
    .into_iter()
    .map(|mut paragraph| {
      if let Some(justification) = paragraph_alignment(alignment) {
        let properties = paragraph
          .paragraph_properties
          .get_or_insert_with(|| Box::new(ParagraphProperties::default()));
        properties.justification = Some(Justification { val: justification });
      }
      paragraph
    })
    .collect()
}

fn paragraph_alignment(alignment: Alignment) -> Option<JustificationValues> {
  match alignment {
    Alignment::Center => Some(JustificationValues::Center),
    Alignment::Right => Some(JustificationValues::Right),
    Alignment::Left => Some(JustificationValues::Left),
    Alignment::None => None,
  }
}

fn table_row_properties(is_head: bool) -> TableRowProperties {
  let mut choices = Vec::new();
  if is_head {
    choices.push(TableRowPropertiesChoice::TableHeader(Box::default()));
  }

  TableRowProperties {
    table_row_properties_choice1: choices,
    ..Default::default()
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
    out.push(BodyChoice::Paragraph(Box::new(paragraph)));
  }
}

fn push_code_block(
  out: &mut Vec<BodyChoice>,
  table: &mut Option<TableBuilder>,
  docx: &DocxRenderContext<'_>,
  lang: &str,
  code: &str,
) -> Result<()> {
  let lines = docx.highlighter.lines(lang, code)?;
  let last = lines.len().saturating_sub(1);

  if let Some(table) = table {
    for (index, line) in lines.into_iter().enumerate() {
      table.push_paragraph(code_paragraph(line, index == last, &docx.styles));
    }
  } else {
    out.extend(lines.into_iter().enumerate().map(|(index, line)| {
      BodyChoice::Paragraph(Box::new(code_paragraph(line, index == last, &docx.styles)))
    }));
  }
  Ok(())
}

fn code_paragraph(line: Vec<CodeRun>, _last: bool, styles: &DocxStyles) -> Paragraph {
  let runs = if line.is_empty() {
    vec![ParagraphChoice::WRun(Box::new(code_text_run("", None)))]
  } else {
    line
      .into_iter()
      .map(|run| ParagraphChoice::WRun(Box::new(code_text_run(&run.text, run.style))))
      .collect()
  };

  let properties = block_properties(
    styles.code.as_deref(),
    ParagraphKind::Code,
    0,
    None,
    None,
    false,
  );

  Paragraph {
    paragraph_properties: Some(Box::new(properties)),
    paragraph_choice: runs,
    ..Default::default()
  }
}

fn code_text_run(text: &str, style: Option<CodeRunStyle>) -> Run {
  Run {
    run_properties: style.map(code_run_properties).map(Box::new),
    run_choice: vec![RunChoice::Text(Box::new(text_node(text, true)))],
    ..Default::default()
  }
}

fn code_run_properties(style: CodeRunStyle) -> RunProperties {
  let mut choices = Vec::new();
  if style.bold {
    choices.push(RunPropertiesChoice::Bold(Box::default()));
  }
  if style.italic {
    choices.push(RunPropertiesChoice::Italic(Box::default()));
  }
  if style.underline {
    choices.push(RunPropertiesChoice::Underline(Box::new(Underline {
      val: Some(UnderlineValues::Single),
      ..Default::default()
    })));
  }
  if let Some(color) = style.color {
    choices.push(RunPropertiesChoice::Color(Box::new(Color {
      val: color,
      ..Default::default()
    })));
  }

  RunProperties {
    run_properties_choice: choices,
    ..Default::default()
  }
}

fn heading_paragraph_with_bookmark(
  level: usize,
  text: &str,
  bookmark: Option<BookmarkRef>,
  styles: &DocxStyles,
) -> Paragraph {
  let mut paragraph_choice = Vec::new();
  if let Some(bookmark) = bookmark {
    paragraph_choice.push(ParagraphChoice::BookmarkStart(Box::new(BookmarkStart {
      name: bookmark.name,
      id: bookmark.id.clone(),
      ..Default::default()
    })));
    paragraph_choice.push(ParagraphChoice::WRun(Box::new(text_run(
      text,
      RunStyle {
        bold: true,
        ..Default::default()
      },
      styles,
    ))));
    paragraph_choice.push(ParagraphChoice::BookmarkEnd(Box::new(BookmarkEnd {
      id: bookmark.id,
      ..Default::default()
    })));
  } else {
    paragraph_choice.push(ParagraphChoice::WRun(Box::new(text_run(
      text,
      RunStyle {
        bold: true,
        ..Default::default()
      },
      styles,
    ))));
  }

  Paragraph {
    paragraph_properties: styles
      .heading(level)
      .map(paragraph_properties)
      .map(Box::new),
    paragraph_choice,
    ..Default::default()
  }
}

fn cover_title_paragraph(text: &str, styles: &DocxStyles) -> Paragraph {
  Paragraph {
    paragraph_properties: styles
      .title
      .as_deref()
      .map(paragraph_properties)
      .map(Box::new),
    paragraph_choice: vec![ParagraphChoice::WRun(Box::new(Run {
      run_choice: vec![RunChoice::Text(Box::new(text_node(text, false)))],
      ..Default::default()
    }))],
    ..Default::default()
  }
}

fn paragraph_from_text(text: &str, style: RunStyle, styles: &DocxStyles) -> Paragraph {
  Paragraph {
    paragraph_choice: vec![ParagraphChoice::WRun(Box::new(text_run(
      text, style, styles,
    )))],
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

fn image_drawing(id: u32, image: &ImageRef) -> Drawing {
  let (cx, cy) = image_extent(image.width, image.height, image.max_width_emu);
  Drawing {
    drawing_choice: Some(DrawingChoice::Inline(Box::new(wp::Inline {
      distance_from_top: Some(0),
      distance_from_bottom: Some(0),
      distance_from_left: Some(0),
      distance_from_right: Some(0),
      extent: Box::new(wp::Extent { cx, cy }),
      effect_extent: Some(wp::EffectExtent {
        left_edge: CoordinateValue::Emu(0),
        top_edge: CoordinateValue::Emu(0),
        right_edge: CoordinateValue::Emu(0),
        bottom_edge: CoordinateValue::Emu(0),
      }),
      doc_properties: Box::new(wp::DocProperties {
        id,
        name: format!("Picture {id}"),
        description: (!image.alt.is_empty()).then(|| image.alt.clone()),
        ..Default::default()
      }),
      non_visual_graphic_frame_drawing_properties: Some(Box::new(
        wp::NonVisualGraphicFrameDrawingProperties {
          graphic_frame_locks: Some(Box::new(a::GraphicFrameLocks {
            no_change_aspect: Some(BooleanValue::One),
            ..Default::default()
          })),
          ..Default::default()
        },
      )),
      graphic: Box::new(a::Graphic {
        graphic_data: Box::new(a::GraphicData {
          uri: "http://schemas.openxmlformats.org/drawingml/2006/picture".to_string(),
          graphic_data_choice: vec![a::GraphicDataChoice::Picture(Box::new(pic::Picture {
            non_visual_picture_properties: Some(Box::new(pic::NonVisualPictureProperties {
              non_visual_drawing_properties: Box::new(pic::NonVisualDrawingProperties {
                id,
                name: format!("Picture {id}"),
                description: (!image.alt.is_empty()).then(|| image.alt.clone()),
                ..Default::default()
              }),
              non_visual_picture_drawing_properties: Box::new(
                pic::NonVisualPictureDrawingProperties {
                  picture_locks: Some(Box::new(a::PictureLocks {
                    no_change_aspect: Some(BooleanValue::One),
                    ..Default::default()
                  })),
                  ..Default::default()
                },
              ),
            })),
            blip_fill: Box::new(pic::BlipFill {
              blip: Some(Box::new(a::Blip {
                embed: Some(image.relationship_id.clone()),
                ..Default::default()
              })),
              blip_fill_choice: Some(pic::BlipFillChoice::Stretch(Box::new(a::Stretch {
                fill_rectangle: Some(a::FillRectangle::default()),
              }))),
              ..Default::default()
            }),
            shape_properties: Some(Box::new(pic::ShapeProperties {
              transform2_d: Some(Box::new(a::Transform2D {
                offset: Some(a::Offset {
                  x: CoordinateValue::Emu(0),
                  y: CoordinateValue::Emu(0),
                }),
                extents: Some(a::Extents {
                  cx: CoordinateValue::Emu(cx),
                  cy: CoordinateValue::Emu(cy),
                }),
                ..Default::default()
              })),
              shape_properties_choice1: Some(pic::ShapePropertiesChoice::PresetGeometry(Box::new(
                a::PresetGeometry {
                  preset: a::ShapeTypeValues::Rectangle,
                  adjust_value_list: Some(a::AdjustValueList::default()),
                  ..Default::default()
                },
              ))),
              ..Default::default()
            })),
            ..Default::default()
          }))],
        }),
        ..Default::default()
      }),
      ..Default::default()
    }))),
    ..Default::default()
  }
}

fn image_extent(width: u32, height: u32, max_width_emu: i64) -> (i64, i64) {
  const EMU_PER_PIXEL: f64 = 9525.0;
  let max_width_emu = max_width_emu.max(1) as f64;
  let width_emu = width as f64 * EMU_PER_PIXEL;
  let height_emu = height as f64 * EMU_PER_PIXEL;
  let scale = if width_emu > max_width_emu {
    max_width_emu / width_emu
  } else {
    1.0
  };
  ((width_emu * scale) as i64, (height_emu * scale) as i64)
}

fn page_break_paragraph() -> Paragraph {
  Paragraph {
    paragraph_choice: vec![ParagraphChoice::WRun(Box::new(Run {
      run_choice: vec![RunChoice::Break(Box::new(Break {
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
      history: Some(OnOffValue::True),
      hyperlink_choice: vec![HyperlinkChoice::WRun(Box::new(run))],
      ..Default::default()
    },
    LinkTarget::Anchor(anchor) => Hyperlink {
      anchor: Some(anchor.clone()),
      history: Some(OnOffValue::True),
      hyperlink_choice: vec![HyperlinkChoice::WRun(Box::new(run))],
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

fn local_asset_path(chapter: &Chapter, source_dir: &Path, url: &str) -> Option<PathBuf> {
  if is_remote_url(url) {
    return None;
  }
  let path = url.split(['#', '?']).next().unwrap_or(url);
  if path.is_empty() {
    return None;
  }

  let mut resolved = chapter
    .source_path
    .as_ref()
    .and_then(|path| path.parent())
    .map_or_else(PathBuf::new, Path::to_path_buf);
  resolved.push(path);
  Some(source_dir.join(normalize_path(&resolved)))
}

fn image_content_type(path: &Path, data: &[u8]) -> Option<&'static str> {
  if data.starts_with(b"\x89PNG\r\n\x1a\n") {
    return Some("image/png");
  }
  if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
    return Some("image/jpeg");
  }
  match path.extension().and_then(|extension| extension.to_str()) {
    Some(extension) if extension.eq_ignore_ascii_case("png") => Some("image/png"),
    Some(extension) if extension.eq_ignore_ascii_case("jpg") => Some("image/jpeg"),
    Some(extension) if extension.eq_ignore_ascii_case("jpeg") => Some("image/jpeg"),
    _ => None,
  }
}

fn image_dimensions(data: &[u8]) -> Option<(u32, u32)> {
  png_dimensions(data).or_else(|| jpeg_dimensions(data))
}

fn png_dimensions(data: &[u8]) -> Option<(u32, u32)> {
  if data.len() < 24 || !data.starts_with(b"\x89PNG\r\n\x1a\n") {
    return None;
  }
  let width = u32::from_be_bytes(data[16..20].try_into().ok()?);
  let height = u32::from_be_bytes(data[20..24].try_into().ok()?);
  Some((width, height))
}

fn jpeg_dimensions(data: &[u8]) -> Option<(u32, u32)> {
  if data.len() < 4 || !data.starts_with(&[0xFF, 0xD8]) {
    return None;
  }
  let mut index = 2usize;
  while index + 9 < data.len() {
    while index < data.len() && data[index] == 0xFF {
      index += 1;
    }
    if index >= data.len() {
      return None;
    }
    let marker = data[index];
    index += 1;
    if marker == 0xD9 || marker == 0xDA {
      return None;
    }
    if index + 2 > data.len() {
      return None;
    }
    let length = u16::from_be_bytes(data[index..index + 2].try_into().ok()?) as usize;
    if length < 2 || index + length > data.len() {
      return None;
    }
    if matches!(
      marker,
      0xC0 | 0xC1 | 0xC2 | 0xC3 | 0xC5 | 0xC6 | 0xC7 | 0xC9 | 0xCA | 0xCB | 0xCD | 0xCE | 0xCF
    ) {
      let height = u16::from_be_bytes(data[index + 3..index + 5].try_into().ok()?) as u32;
      let width = u16::from_be_bytes(data[index + 5..index + 7].try_into().ok()?) as u32;
      return Some((width, height));
    }
    index += length;
  }
  None
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

fn numbering_definitions(ordered_lists: &[OrderedListDefinition]) -> Numbering {
  Numbering {
    xmlns: wordprocessing_xmlns(),
    abstract_num: vec![
      abstract_numbering(1, NumberFormatValues::Bullet, "-"),
      abstract_numbering(2, NumberFormatValues::Decimal, "%1."),
      abstract_numbering(3, NumberFormatValues::Bullet, "[x]"),
      abstract_numbering(4, NumberFormatValues::Bullet, "[ ]"),
    ],
    numbering_instance: [
      numbering_instance(BULLET_NUM_ID, 1),
      numbering_instance(TASK_DONE_NUM_ID, 3),
      numbering_instance(TASK_TODO_NUM_ID, 4),
    ]
    .into_iter()
    .chain(ordered_lists.iter().map(ordered_numbering_instance))
    .collect(),
    ..Default::default()
  }
}

fn abstract_numbering(id: i32, format: NumberFormatValues, text: &str) -> AbstractNum {
  AbstractNum {
    abstract_number_id: id,
    multi_level_type: Some(MultiLevelType {
      val: MultiLevelValues::Multilevel,
    }),
    level: (0..9)
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
                    left: Some(signed_twips(((level as usize + 1) * 420) as i64)),
                    hanging: Some(twips(240)),
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

fn ordered_numbering_instance(definition: &OrderedListDefinition) -> NumberingInstance {
  NumberingInstance {
    number_id: definition.number_id,
    abstract_num_id: Box::new(AbstractNumId { val: 2 }),
    level_override: vec![LevelOverride {
      level_index: definition.level.min(8) as i32,
      start_override_numbering_value: Some(StartOverrideNumberingValue {
        val: definition.start.min(i32::MAX as u64) as i32,
      }),
      ..Default::default()
    }],
    ..Default::default()
  }
}

fn block_properties(
  style: Option<&str>,
  _kind: ParagraphKind,
  _quote_depth: usize,
  list_depth: Option<usize>,
  numbering_id: Option<i32>,
  _has_image: bool,
) -> ParagraphProperties {
  let mut properties = style.map_or_else(ParagraphProperties::default, paragraph_properties);

  if let (Some(depth), Some(numbering_id)) = (list_depth, numbering_id) {
    properties.numbering_properties = Some(Box::new(NumberingProperties {
      numbering_level_reference: Some(NumberingLevelReference {
        val: depth.saturating_sub(1).min(8) as i32,
      }),
      numbering_id: Some(NumberingId { val: numbering_id }),
      ..Default::default()
    }));
  }

  properties
}

fn text_run(text: &str, style: RunStyle, styles: &DocxStyles) -> Run {
  Run {
    run_properties: run_properties(style, styles).map(Box::new),
    run_choice: vec![RunChoice::Text(Box::new(text_node(
      text,
      text_needs_preserve(text),
    )))],
    ..Default::default()
  }
}

fn run_properties(style: RunStyle, styles: &DocxStyles) -> Option<RunProperties> {
  if !(style.bold || style.italic || style.strike || style.inline_code || style.hyperlink) {
    return None;
  }

  let mut choices = Vec::new();
  if style.bold {
    choices.push(RunPropertiesChoice::Bold(Box::default()));
  }
  if style.italic {
    choices.push(RunPropertiesChoice::Italic(Box::default()));
  }
  if style.strike {
    choices.push(RunPropertiesChoice::Strike(Box::default()));
  }
  let run_style = if style.hyperlink {
    styles.hyperlink.as_ref()
  } else if style.inline_code {
    styles.inline_code.as_ref()
  } else {
    None
  };
  if let Some(run_style) = run_style {
    choices.push(RunPropertiesChoice::RunStyle(Box::new(WordRunStyle {
      val: run_style.clone(),
    })));
  }
  Some(RunProperties {
    run_properties_choice: choices,
    ..Default::default()
  })
}

fn plain_code_lines(code: &str) -> Vec<Vec<CodeRun>> {
  code_lines(code)
    .into_iter()
    .map(|line| {
      vec![CodeRun {
        text: line.to_string(),
        style: None,
      }]
    })
    .collect()
}

fn code_lines(code: &str) -> Vec<&str> {
  let mut lines = code.split('\n').collect::<Vec<_>>();
  if lines.last().is_some_and(|line| line.is_empty()) {
    lines.pop();
  }
  if lines.is_empty() {
    lines.push("");
  }
  lines
}

fn color_to_hex(color: SyntectColor) -> String {
  format!("{:02X}{:02X}{:02X}", color.r, color.g, color.b)
}

fn text_needs_preserve(text: &str) -> bool {
  text.starts_with(char::is_whitespace)
    || text.ends_with(char::is_whitespace)
    || text.contains("  ")
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
