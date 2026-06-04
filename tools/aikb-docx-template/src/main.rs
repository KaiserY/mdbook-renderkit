use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;

use ooxmlsdk::parts::wordprocessing_document::WordprocessingDocument;
use ooxmlsdk::schemas::schemas_openxmlformats_org_wordprocessingml_2006_main::{
  BasedOn, BodyChoice, Bold, BoldComplexScript, BookmarkEnd, BookmarkStart, BorderValues,
  BottomBorder, BottomMargin, Break, BreakValues, Color, Document, FieldChar, FieldCharValues,
  FieldCode, FontSize, FontSizeComplexScript, Indentation, InsideHorizontalBorder,
  InsideVerticalBorder, Justification, JustificationValues, LeftBorder, NextParagraphStyle,
  Paragraph, ParagraphChoice, ParagraphProperties, ParagraphStyleId, RightBorder, Run, RunChoice,
  RunFonts, RunPropertiesBaseStyle, Shading, ShadingPatternValues, SpacingBetweenLines,
  Style as WordStyle, StyleName, StyleParagraphProperties, StyleRunProperties,
  StyleTableProperties, StyleValues, Styles, TableBorders, TableCellBorders, TableCellLeftMargin,
  TableCellMarginDefault, TableCellRightMargin, TableCellVerticalAlignment, TableJustification,
  TableRowAlignmentValues, TableStyleConditionalFormattingTableCellProperties,
  TableStyleConditionalFormattingTableProperties, TableStyleOverrideValues, TableStyleProperties,
  TableVerticalAlignmentValues, TableWidthUnitValues, Text, TextType, TopBorder, TopMargin,
  Underline, UnderlineValues,
};
use ooxmlsdk::schemas::www_w3_org_xml_1998_namespace::SpaceProcessingModeValues;
use ooxmlsdk::simple_type::OnOffValue;
use ooxmlsdk::simple_type::{
  MeasurementOrPercentValue, SignedTwipsMeasureValue, TwipsMeasureValue,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const TITLE_MARKER: &str = "mdbook_renderkit_title";
const AUTHOR_MARKER: &str = "mdbook_renderkit_author";
const CONTENT_BOOKMARK_MARKER: &str = "mdbook_renderkit_content";
const MARKERS: [&str; 3] = [CONTENT_BOOKMARK_MARKER, TITLE_MARKER, AUTHOR_MARKER];
const DEFAULT_INPUT: &str = "../aikb-books/single-node-install/template_old.docx";
const DEFAULT_OUTPUT: &str = "../aikb-books/single-node-install/template.docx";

fn main() -> Result<()> {
  let args = Args::parse()?;
  let mut package = WordprocessingDocument::create_from_template(&args.input)
    .map_err(|error| format!("failed to read {}: {error}", args.input.display()))?;
  let main_part = package.main_document_part()?;
  let styles_part = main_part
    .style_definitions_part(&package)
    .ok_or("template has no styles.xml")?;
  let mut styles = styles_part.root_element(&mut package)?.clone();
  add_renderkit_styles(&mut styles)?;
  styles_part.set_root_element(&mut package, styles)?;

  let mut document = main_part.root_element(&mut package)?.clone();
  rewrite_document(&mut document)?;
  main_part.set_root_element(&mut package, document)?;

  let file = File::create(&args.output)
    .map_err(|error| format!("failed to create {}: {error}", args.output.display()))?;
  package.save(file)?;
  eprintln!("wrote {}", args.output.display());
  Ok(())
}

struct Args {
  input: PathBuf,
  output: PathBuf,
}

impl Args {
  fn parse() -> Result<Self> {
    let mut input = PathBuf::from(DEFAULT_INPUT);
    let mut output = PathBuf::from(DEFAULT_OUTPUT);
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
      match arg.as_str() {
        "--input" => {
          input = PathBuf::from(args.next().ok_or("--input requires a path")?);
        }
        "--output" => {
          output = PathBuf::from(args.next().ok_or("--output requires a path")?);
        }
        "-h" | "--help" => {
          print_help();
          std::process::exit(0);
        }
        _ => return Err(format!("unknown argument {arg}").into()),
      }
    }
    Ok(Self { input, output })
  }
}

fn print_help() {
  println!(
    "Usage: cargo run --manifest-path tools/aikb-docx-template/Cargo.toml -- [--input PATH] [--output PATH]"
  );
}

struct StyleSpec {
  id: String,
  r#type: StyleValues,
  source_ids: Vec<String>,
  source_names: Vec<String>,
  next_style: Option<String>,
  based_on: Option<String>,
}

fn add_renderkit_styles(styles: &mut Styles) -> Result<()> {
  let lookup = StyleLookup::new(&styles.style);
  let specs = renderkit_style_specs();
  let mut additions = Vec::new();
  for spec in &specs {
    let source = lookup
      .find(&spec.source_ids, &spec.source_names, spec.r#type)
      .or_else(|| lookup.find(&["Normal"], &["Normal"], spec.r#type))
      .ok_or_else(|| format!("cannot create style {}: no source style", spec.id))?;
    additions.push(renderkit_style_from_source(source, spec));
  }

  styles.style.retain(|style| {
    let Some(style_id) = style.style_id.as_deref() else {
      return true;
    };
    !specs.iter().any(|spec| spec.id == style_id)
  });
  for style in &mut additions {
    apply_renderkit_style_formatting(style);
  }
  styles.style.extend(additions);
  Ok(())
}

fn renderkit_style_specs() -> Vec<StyleSpec> {
  let mut specs = vec![
    StyleSpec {
      id: "RenderkitTitle".to_string(),
      r#type: StyleValues::Paragraph,
      source_ids: strings(&["20", "Title"]),
      source_names: strings(&["Title"]),
      next_style: Some("RenderkitBody".to_string()),
      based_on: None,
    },
    StyleSpec {
      id: "RenderkitBody".to_string(),
      r#type: StyleValues::Paragraph,
      source_ids: strings(&["59", "BodyText", "Normal"]),
      source_names: strings(&["Body Text", "Normal"]),
      next_style: Some("RenderkitBody".to_string()),
      based_on: None,
    },
    StyleSpec {
      id: "RenderkitTocHeading".to_string(),
      r#type: StyleValues::Paragraph,
      source_ids: strings(&["52", "TOCHeading"]),
      source_names: strings(&["TOC Heading"]),
      next_style: Some("RenderkitBody".to_string()),
      based_on: None,
    },
    StyleSpec {
      id: "RenderkitCode".to_string(),
      r#type: StyleValues::Paragraph,
      source_ids: strings(&["61", "NoSpacing"]),
      source_names: strings(&["无间隔1", "无间隔", "No Spacing"]),
      next_style: Some("RenderkitBody".to_string()),
      based_on: None,
    },
    StyleSpec {
      id: "RenderkitInlineCode".to_string(),
      r#type: StyleValues::Character,
      source_ids: strings(&["24", "Hyperlink"]),
      source_names: strings(&["Hyperlink"]),
      next_style: None,
      based_on: None,
    },
    StyleSpec {
      id: "RenderkitImage".to_string(),
      r#type: StyleValues::Paragraph,
      source_ids: strings(&["59", "BodyText", "Normal"]),
      source_names: strings(&["Body Text", "Normal"]),
      next_style: Some("RenderkitBody".to_string()),
      based_on: None,
    },
    StyleSpec {
      id: "RenderkitList".to_string(),
      r#type: StyleValues::Paragraph,
      source_ids: strings(&["55", "ListParagraph"]),
      source_names: strings(&["List Paragraph"]),
      next_style: Some("RenderkitList".to_string()),
      based_on: Some("RenderkitBody".to_string()),
    },
    StyleSpec {
      id: "RenderkitQuote".to_string(),
      r#type: StyleValues::Paragraph,
      source_ids: strings(&["Quote", "59", "BodyText"]),
      source_names: strings(&["Quote", "Body Text"]),
      next_style: Some("RenderkitBody".to_string()),
      based_on: Some("RenderkitBody".to_string()),
    },
    StyleSpec {
      id: "RenderkitTable".to_string(),
      r#type: StyleValues::Table,
      source_ids: strings(&["22", "TableGrid", "21", "34"]),
      source_names: strings(&["Table Grid", "Normal Table", "BlackHeader"]),
      next_style: None,
      based_on: None,
    },
    StyleSpec {
      id: "RenderkitHyperlink".to_string(),
      r#type: StyleValues::Character,
      source_ids: strings(&["24", "Hyperlink"]),
      source_names: strings(&["Hyperlink"]),
      next_style: None,
      based_on: None,
    },
  ];

  for level in 1..=9 {
    specs.push(StyleSpec {
      id: format!("RenderkitHeading{level}"),
      r#type: StyleValues::Paragraph,
      source_ids: vec![(level + 1).to_string(), format!("Heading{level}")],
      source_names: vec![format!("heading {level}")],
      next_style: Some("RenderkitBody".to_string()),
      based_on: None,
    });
    specs.push(StyleSpec {
      id: format!("RenderkitToc{level}"),
      r#type: StyleValues::Paragraph,
      source_ids: vec![toc_source_id(level).to_string(), format!("TOC{level}")],
      source_names: vec![format!("toc {}", level.min(3))],
      next_style: Some("RenderkitBody".to_string()),
      based_on: None,
    });
  }
  specs
}

fn strings(values: &[&str]) -> Vec<String> {
  values.iter().map(|value| (*value).to_string()).collect()
}

fn toc_source_id(level: usize) -> &'static str {
  match level {
    1 => "16",
    2 => "18",
    _ => "13",
  }
}

fn renderkit_style_from_source(source: &WordStyle, spec: &StyleSpec) -> WordStyle {
  let mut style = source.clone();
  style.style_id = Some(spec.id.clone());
  style.r#type = Some(spec.r#type);
  style.default = None;
  style.custom_style = Some(OnOffValue::True);
  style.style_name = Some(StyleName {
    val: spec.id.clone(),
  });
  style.aliases = None;
  style.linked_style = None;
  style.next_paragraph_style = spec.next_style.as_ref().map(|style_id| NextParagraphStyle {
    val: style_id.clone(),
  });
  style.based_on = spec.based_on.as_ref().map(|style_id| BasedOn {
    val: style_id.clone(),
  });
  style
}

fn apply_renderkit_style_formatting(style: &mut WordStyle) {
  let Some(style_id) = style.style_id.as_deref() else {
    return;
  };
  match style_id {
    "RenderkitBody" => {
      style.style_paragraph_properties = Some(Box::new(body_paragraph_style(Some(420))));
      style.style_run_properties = Some(Box::new(run_style(body_fonts(), "21", false)));
    }
    "RenderkitList" => {
      style.style_paragraph_properties = Some(Box::new(body_paragraph_style(None)));
      style.style_run_properties = Some(Box::new(run_style(body_fonts(), "21", false)));
    }
    "RenderkitQuote" => {
      let mut paragraph = body_paragraph_style(None);
      paragraph.indentation = Some(Indentation {
        left: Some(signed_twips(420)),
        ..Default::default()
      });
      style.style_paragraph_properties = Some(Box::new(paragraph));
      style.style_run_properties = Some(Box::new(run_style(body_fonts(), "21", false)));
    }
    "RenderkitCode" => {
      style.style_paragraph_properties = Some(Box::new(StyleParagraphProperties {
        shading: Some(fill_shading("F6F8FA")),
        spacing_between_lines: Some(line_spacing_exact(0, 0, 324)),
        justification: Some(Justification {
          val: JustificationValues::Both,
        }),
        ..Default::default()
      }));
      style.style_run_properties = Some(Box::new(run_style(code_fonts(), "21", false)));
    }
    "RenderkitInlineCode" => {
      style.style_run_properties = Some(Box::new(StyleRunProperties {
        shading: Some(fill_shading("F6F8FA")),
        ..run_style(code_fonts(), "21", false)
      }));
    }
    "RenderkitImage" => {
      style.style_paragraph_properties = Some(Box::new(StyleParagraphProperties {
        justification: Some(Justification {
          val: JustificationValues::Center,
        }),
        indentation: Some(Indentation {
          left: Some(signed_twips(0)),
          right: Some(signed_twips(0)),
          first_line: Some(twips(0)),
          ..Default::default()
        }),
        spacing_between_lines: Some(line_spacing(0, 0)),
        ..Default::default()
      }));
      style.style_run_properties = Some(Box::new(run_style(body_fonts(), "21", false)));
    }
    "RenderkitTocHeading" => {
      style.style_paragraph_properties = Some(Box::new(StyleParagraphProperties {
        justification: Some(Justification {
          val: JustificationValues::Center,
        }),
        spacing_between_lines: Some(line_spacing(0, 0)),
        ..Default::default()
      }));
      style.style_run_properties = Some(Box::new(run_style(body_fonts(), "48", true)));
    }
    "RenderkitTable" => {
      style.style_table_properties = Some(Box::new(StyleTableProperties {
        table_justification: Some(TableJustification {
          val: TableRowAlignmentValues::Center,
        }),
        table_borders: Some(Box::new(table_borders())),
        table_cell_margin_default: Some(Box::new(table_cell_margin_default())),
        ..Default::default()
      }));
      style.table_style_properties = vec![TableStyleProperties {
        r#type: TableStyleOverrideValues::FirstRow,
        style_paragraph_properties: Some(Box::new(StyleParagraphProperties {
          justification: Some(Justification {
            val: JustificationValues::Center,
          }),
          spacing_between_lines: Some(line_spacing(0, 0)),
          ..Default::default()
        })),
        run_properties_base_style: Some(Box::new(run_properties_base_style(
          body_fonts(),
          "21",
          true,
        ))),
        table_style_conditional_formatting_table_properties: Some(Box::new(
          TableStyleConditionalFormattingTableProperties {
            table_justification: Some(TableJustification {
              val: TableRowAlignmentValues::Center,
            }),
            table_borders: Some(Box::new(table_borders())),
            shading: Some(fill_shading("CCCCCC")),
            ..Default::default()
          },
        )),
        table_style_conditional_formatting_table_cell_properties: Some(Box::new(
          TableStyleConditionalFormattingTableCellProperties {
            table_cell_borders: Some(Box::new(table_cell_borders())),
            shading: Some(fill_shading("CCCCCC")),
            table_cell_vertical_alignment: Some(TableCellVerticalAlignment {
              val: TableVerticalAlignmentValues::Center,
            }),
            ..Default::default()
          },
        )),
        ..Default::default()
      }];
    }
    "RenderkitHyperlink" => {
      style.style_run_properties = Some(Box::new(StyleRunProperties {
        color: Some(Color {
          val: "0563C1".to_string(),
          ..Default::default()
        }),
        underline: Some(Underline {
          val: Some(UnderlineValues::Single),
          ..Default::default()
        }),
        ..run_style(body_fonts(), "21", false)
      }));
    }
    id if id.starts_with("RenderkitHeading") => {
      let level = id
        .trim_start_matches("RenderkitHeading")
        .parse::<usize>()
        .unwrap_or(3);
      let mut paragraph = style
        .style_paragraph_properties
        .as_deref()
        .cloned()
        .unwrap_or_default();
      paragraph.numbering_properties = None;
      paragraph.justification = Some(Justification {
        val: JustificationValues::Both,
      });
      paragraph.spacing_between_lines = Some(line_spacing(0, 0));
      style.style_paragraph_properties = Some(Box::new(paragraph));
      style.style_run_properties = Some(Box::new(run_style(
        body_fonts(),
        heading_font_size(level),
        true,
      )));
    }
    id if id.starts_with("RenderkitToc") => {
      style.style_run_properties = Some(Box::new(run_style(body_fonts(), "21", false)));
    }
    _ => {}
  }
}

fn body_paragraph_style(first_line: Option<i64>) -> StyleParagraphProperties {
  StyleParagraphProperties {
    spacing_between_lines: Some(line_spacing(0, 0)),
    indentation: first_line.map(|first_line| Indentation {
      first_line: Some(twips(first_line as u64)),
      ..Default::default()
    }),
    justification: Some(Justification {
      val: JustificationValues::Both,
    }),
    ..Default::default()
  }
}

fn heading_font_size(level: usize) -> &'static str {
  match level {
    1 => "28",
    2 => "24",
    _ => "21",
  }
}

fn body_fonts() -> RunFonts {
  RunFonts {
    ascii: Some("Arial".to_string()),
    high_ansi: Some("Arial".to_string()),
    east_asia: Some("微软雅黑".to_string()),
    complex_script: Some("微软雅黑".to_string()),
    ..Default::default()
  }
}

fn code_fonts() -> RunFonts {
  RunFonts {
    ascii: Some("Consolas".to_string()),
    high_ansi: Some("Consolas".to_string()),
    east_asia: Some("Consolas".to_string()),
    complex_script: Some("Consolas".to_string()),
    ..Default::default()
  }
}

fn run_style(fonts: RunFonts, size: &str, bold: bool) -> StyleRunProperties {
  StyleRunProperties {
    run_fonts: Some(fonts),
    bold: bold.then(Bold::default),
    bold_complex_script: bold.then(BoldComplexScript::default),
    font_size: Some(FontSize {
      val: size.to_string(),
    }),
    font_size_complex_script: Some(FontSizeComplexScript {
      val: size.to_string(),
    }),
    ..Default::default()
  }
}

fn run_properties_base_style(fonts: RunFonts, size: &str, bold: bool) -> RunPropertiesBaseStyle {
  RunPropertiesBaseStyle {
    run_fonts: Some(fonts),
    bold: bold.then(Bold::default),
    bold_complex_script: bold.then(BoldComplexScript::default),
    font_size: Some(FontSize {
      val: size.to_string(),
    }),
    font_size_complex_script: Some(FontSizeComplexScript {
      val: size.to_string(),
    }),
    ..Default::default()
  }
}

fn line_spacing(before: u64, after: u64) -> SpacingBetweenLines {
  line_spacing_exact(before, after, 240)
}

fn line_spacing_exact(before: u64, after: u64, line: i64) -> SpacingBetweenLines {
  SpacingBetweenLines {
    before: Some(twips(before)),
    after: Some(twips(after)),
    line: Some(signed_twips(line)),
    line_rule: Some(
      ooxmlsdk::schemas::schemas_openxmlformats_org_wordprocessingml_2006_main::LineSpacingRuleValues::Auto,
    ),
    ..Default::default()
  }
}

fn table_cell_margin_default() -> TableCellMarginDefault {
  TableCellMarginDefault {
    top_margin: Some(TopMargin {
      width: Some(pct(0)),
      r#type: Some(TableWidthUnitValues::Dxa),
    }),
    table_cell_left_margin: Some(TableCellLeftMargin {
      width: Some(pct(108)),
      r#type: Some(TableWidthUnitValues::Dxa),
    }),
    bottom_margin: Some(BottomMargin {
      width: Some(pct(0)),
      r#type: Some(TableWidthUnitValues::Dxa),
    }),
    table_cell_right_margin: Some(TableCellRightMargin {
      width: Some(pct(108)),
      r#type: Some(TableWidthUnitValues::Dxa),
    }),
    ..Default::default()
  }
}

fn table_borders() -> TableBorders {
  TableBorders {
    top_border: Some(table_top_border()),
    left_border: Some(table_left_border()),
    bottom_border: Some(table_bottom_border()),
    right_border: Some(table_right_border()),
    inside_horizontal_border: Some(InsideHorizontalBorder {
      val: BorderValues::Single,
      color: Some("auto".to_string()),
      size: Some(4),
      space: Some(0),
      ..Default::default()
    }),
    inside_vertical_border: Some(InsideVerticalBorder {
      val: BorderValues::Single,
      color: Some("auto".to_string()),
      size: Some(4),
      space: Some(0),
      ..Default::default()
    }),
    ..Default::default()
  }
}

fn table_cell_borders() -> TableCellBorders {
  TableCellBorders {
    top_border: Some(table_top_border()),
    left_border: Some(table_left_border()),
    bottom_border: Some(table_bottom_border()),
    right_border: Some(table_right_border()),
    inside_horizontal_border: Some(InsideHorizontalBorder {
      val: BorderValues::Single,
      color: Some("auto".to_string()),
      size: Some(4),
      space: Some(0),
      ..Default::default()
    }),
    inside_vertical_border: Some(InsideVerticalBorder {
      val: BorderValues::Single,
      color: Some("auto".to_string()),
      size: Some(4),
      space: Some(0),
      ..Default::default()
    }),
    ..Default::default()
  }
}

fn table_top_border() -> TopBorder {
  TopBorder {
    val: BorderValues::Single,
    color: Some("auto".to_string()),
    size: Some(4),
    space: Some(0),
    ..Default::default()
  }
}

fn table_left_border() -> LeftBorder {
  LeftBorder {
    val: BorderValues::Single,
    color: Some("auto".to_string()),
    size: Some(4),
    space: Some(0),
    ..Default::default()
  }
}

fn table_bottom_border() -> BottomBorder {
  BottomBorder {
    val: BorderValues::Single,
    color: Some("auto".to_string()),
    size: Some(4),
    space: Some(0),
    ..Default::default()
  }
}

fn table_right_border() -> RightBorder {
  RightBorder {
    val: BorderValues::Single,
    color: Some("auto".to_string()),
    size: Some(4),
    space: Some(0),
    ..Default::default()
  }
}

fn fill_shading(fill: &str) -> Shading {
  Shading {
    val: ShadingPatternValues::Clear,
    color: Some("auto".to_string()),
    fill: Some(fill.to_string()),
    ..Default::default()
  }
}

fn twips(value: u64) -> TwipsMeasureValue {
  TwipsMeasureValue::Twips(value)
}

fn signed_twips(value: i64) -> SignedTwipsMeasureValue {
  SignedTwipsMeasureValue::Twips(value)
}

fn pct(value: i64) -> MeasurementOrPercentValue {
  MeasurementOrPercentValue::from_bytes(value.to_string().as_bytes())
    .expect("static measurement value is valid")
}

struct StyleLookup {
  by_id: HashMap<String, WordStyle>,
  by_name: HashMap<String, WordStyle>,
}

impl StyleLookup {
  fn new(styles: &[WordStyle]) -> Self {
    let mut by_id = HashMap::new();
    let mut by_name = HashMap::new();
    for style in styles {
      if let Some(style_id) = style.style_id.as_ref() {
        by_id.insert(style_id.clone(), style.clone());
      }
      if let Some(name) = style.style_name.as_ref() {
        by_name.insert(normalize_style_name(&name.val), style.clone());
      }
    }
    Self { by_id, by_name }
  }

  fn find<S: AsRef<str>>(&self, ids: &[S], names: &[S], r#type: StyleValues) -> Option<&WordStyle> {
    ids
      .iter()
      .filter_map(|id| self.by_id.get(id.as_ref()))
      .chain(
        names
          .iter()
          .filter_map(|name| self.by_name.get(&normalize_style_name(name.as_ref()))),
      )
      .find(|style| style.r#type == Some(r#type))
  }
}

fn normalize_style_name(name: &str) -> String {
  name
    .split_whitespace()
    .collect::<Vec<_>>()
    .join(" ")
    .to_ascii_lowercase()
}

fn rewrite_document(document: &mut Document) -> Result<()> {
  let body = document
    .body
    .as_mut()
    .ok_or("template has no document body")?;
  let choices = std::mem::take(&mut body.body_choice);
  let toc_start = first_toc_block_index(&choices)?;
  let section_break = first_section_break_after(&choices, toc_start)?;
  let back_cover = last_drawing_paragraph(&choices)?;

  let mut new_choices = choices[..toc_start].to_vec();
  apply_cover_markers(&mut new_choices)?;
  new_choices.push(choices[section_break].clone());
  new_choices.extend(toc_block());
  new_choices.push(BodyChoice::Paragraph(Box::new(bookmark_marker_paragraph(
    CONTENT_BOOKMARK_MARKER,
    "900003",
  ))));
  new_choices.push(BodyChoice::Paragraph(Box::new(page_break_paragraph())));
  new_choices.push(BodyChoice::Paragraph(Box::new(back_cover)));
  body.body_choice = new_choices;

  validate_markers(document)?;
  Ok(())
}

fn first_toc_block_index(choices: &[BodyChoice]) -> Result<usize> {
  choices
    .iter()
    .position(|choice| matches!(choice, BodyChoice::SdtBlock(_)))
    .ok_or_else(|| "legacy template has no TOC block".into())
}

fn first_section_break_after(choices: &[BodyChoice], start: usize) -> Result<usize> {
  choices
    .iter()
    .enumerate()
    .skip(start + 1)
    .position(|choice| {
      let (_, choice) = choice;
      matches!(
        choice,
        BodyChoice::Paragraph(paragraph)
          if paragraph
            .paragraph_properties
            .as_ref()
            .and_then(|properties| properties.section_properties.as_ref())
            .is_some()
      )
    })
    .map(|offset| start + 1 + offset)
    .ok_or_else(|| "legacy template has no paragraph section break".into())
}

fn last_drawing_paragraph(choices: &[BodyChoice]) -> Result<Paragraph> {
  choices
    .iter()
    .rev()
    .find_map(|choice| {
      let BodyChoice::Paragraph(paragraph) = choice else {
        return None;
      };
      paragraph_has_drawing(paragraph).then(|| (**paragraph).clone())
    })
    .ok_or_else(|| "legacy template has no drawing paragraph for the back cover".into())
}

fn apply_cover_markers(choices: &mut Vec<BodyChoice>) -> Result<()> {
  for choice in choices.iter_mut() {
    let BodyChoice::Paragraph(paragraph) = choice else {
      continue;
    };
    clear_paragraph_keep_constraints(paragraph);
  }

  let text_indices = choices
    .iter()
    .enumerate()
    .filter_map(|(index, choice)| {
      let BodyChoice::Paragraph(paragraph) = choice else {
        return None;
      };
      (!paragraph_text(paragraph).trim().is_empty()).then_some(index)
    })
    .collect::<Vec<_>>();

  if text_indices.len() < 3 {
    return Err("cover has fewer than three text paragraphs".into());
  }

  let title_index = text_indices[0];
  let subtitle_index = text_indices[1];
  let author_text_index = text_indices.len().saturating_sub(2);
  let author_index = text_indices[author_text_index];

  add_bookmark_marker(paragraph_mut_at(choices, title_index)?, TITLE_MARKER, "900001");
  add_bookmark_marker(
    paragraph_mut_at(choices, author_index)?,
    AUTHOR_MARKER,
    "900002",
  );

  for (text_index, body_index) in text_indices.iter().copied().enumerate() {
    if text_index != 0 && text_index != author_text_index {
      replace_paragraph_text(paragraph_mut_at(choices, body_index)?, "")?;
    }
  }
  choices.remove(subtitle_index);
  Ok(())
}

fn paragraph_mut_at(choices: &mut [BodyChoice], index: usize) -> Result<&mut Paragraph> {
  match choices.get_mut(index) {
    Some(BodyChoice::Paragraph(paragraph)) => Ok(paragraph.as_mut()),
    _ => Err("cover text index no longer points to a paragraph".into()),
  }
}

fn clear_paragraph_keep_constraints(paragraph: &mut Paragraph) {
  if let Some(properties) = paragraph.paragraph_properties.as_mut() {
    properties.keep_next = None;
    properties.keep_lines = None;
  }
}

fn bookmark_marker_paragraph(name: &str, id: &str) -> Paragraph {
  Paragraph {
    paragraph_choice: vec![
      ParagraphChoice::BookmarkStart(Box::new(BookmarkStart {
        name: name.to_string(),
        id: id.to_string(),
        ..Default::default()
      })),
      ParagraphChoice::BookmarkEnd(Box::new(BookmarkEnd {
        id: id.to_string(),
        ..Default::default()
      })),
    ],
    ..Default::default()
  }
}

fn add_bookmark_marker(paragraph: &mut Paragraph, name: &str, id: &str) {
  paragraph
    .paragraph_choice
    .insert(0, ParagraphChoice::BookmarkStart(Box::new(BookmarkStart {
      name: name.to_string(),
      id: id.to_string(),
      ..Default::default()
    })));
  paragraph
    .paragraph_choice
    .push(ParagraphChoice::BookmarkEnd(Box::new(BookmarkEnd {
      id: id.to_string(),
      ..Default::default()
    })));
}

fn toc_block() -> Vec<BodyChoice> {
  vec![
    BodyChoice::Paragraph(Box::new(toc_heading_paragraph())),
    BodyChoice::Paragraph(Box::new(toc_field_paragraph())),
    BodyChoice::Paragraph(Box::new(page_break_paragraph())),
  ]
}

fn toc_heading_paragraph() -> Paragraph {
  let mut properties = paragraph_properties("RenderkitTocHeading");
  properties.justification = Some(Justification {
    val: JustificationValues::Center,
  });

  Paragraph {
    paragraph_properties: Some(Box::new(properties)),
    paragraph_choice: vec![ParagraphChoice::WRun(Box::new(Run {
      run_choice: vec![RunChoice::Text(Box::new(Text(TextType {
        xml_content: Some("目录".to_string()),
        ..Default::default()
      })))],
      ..Default::default()
    }))],
    ..Default::default()
  }
}

fn toc_field_paragraph() -> Paragraph {
  Paragraph {
    paragraph_properties: Some(Box::new(paragraph_properties("RenderkitToc1"))),
    paragraph_choice: vec![
      field_char_run(FieldCharValues::Begin, true),
      field_code_run(&toc_instruction()),
      field_char_run(FieldCharValues::Separate, false),
      field_char_run(FieldCharValues::End, false),
    ]
    .into_iter()
    .map(|run| ParagraphChoice::WRun(Box::new(run)))
    .collect(),
    ..Default::default()
  }
}

fn toc_instruction() -> String {
  let style_map = (1..=9)
    .map(|level| format!("RenderkitHeading{level},{level}"))
    .collect::<Vec<_>>()
    .join(",");
  format!(r#" TOC \t "{style_map}" \h \z \u "#)
}

fn paragraph_properties(style_id: &str) -> ParagraphProperties {
  ParagraphProperties {
    paragraph_style_id: Some(ParagraphStyleId {
      val: style_id.to_string(),
    }),
    ..Default::default()
  }
}

fn field_char_run(field_char_type: FieldCharValues, dirty: bool) -> Run {
  Run {
    run_choice: vec![RunChoice::FieldChar(Box::new(FieldChar {
      field_char_type,
      dirty: dirty.then_some(OnOffValue::True),
      ..Default::default()
    }))],
    ..Default::default()
  }
}

fn field_code_run(instruction: &str) -> Run {
  Run {
    run_choice: vec![RunChoice::FieldCode(Box::new(FieldCode {
      space: Some(SpaceProcessingModeValues::Preserve),
      xml_content: Some(instruction.to_string()),
    }))],
    ..Default::default()
  }
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

fn replace_paragraph_text(paragraph: &mut Paragraph, replacement: &str) -> Result<()> {
  let mut replaced = false;
  for choice in &mut paragraph.paragraph_choice {
    let ParagraphChoice::WRun(run) = choice else {
      continue;
    };
    for run_choice in &mut run.run_choice {
      let RunChoice::Text(text) = run_choice else {
        continue;
      };
      if replaced {
        text.0.xml_content = None;
      } else {
        text.0.xml_content = Some(replacement.to_string());
        text.0.space =
          text_needs_preserve(replacement).then_some(SpaceProcessingModeValues::Preserve);
        replaced = true;
      }
    }
  }
  if !replaced {
    return Err("cannot replace text in paragraph without w:t nodes".into());
  }
  Ok(())
}

fn validate_markers(document: &Document) -> Result<()> {
  let mut counts = [0usize; MARKERS.len()];
  for name in document_bookmark_names(document) {
    if let Some(index) = MARKERS.iter().position(|marker| marker == &name) {
      counts[index] += 1;
    }
  }
  for text in document_text_nodes(document) {
    let Some(content) = text.0.xml_content.as_deref() else {
      continue;
    };
    if content.contains("MDBOOK_RENDERKIT_") {
      return Err(format!("marker is not an exact text run: {content}").into());
    }
  }
  for (index, marker) in MARKERS.iter().enumerate() {
    match counts[index] {
      0 => return Err(format!("missing marker {marker}").into()),
      1 => {}
      _ => return Err(format!("repeated marker {marker}").into()),
    }
  }
  Ok(())
}

fn document_bookmark_names(document: &Document) -> Vec<&str> {
  let mut out = Vec::new();
  if let Some(body) = document.body.as_ref() {
    for choice in &body.body_choice {
      let BodyChoice::Paragraph(paragraph) = choice else {
        continue;
      };
      for paragraph_choice in &paragraph.paragraph_choice {
        if let ParagraphChoice::BookmarkStart(bookmark) = paragraph_choice {
          out.push(bookmark.name.as_str());
        }
      }
    }
  }
  out
}

fn document_text_nodes(document: &Document) -> Vec<&Text> {
  let mut out = Vec::new();
  if let Some(body) = document.body.as_ref() {
    for choice in &body.body_choice {
      if let BodyChoice::Paragraph(paragraph) = choice {
        paragraph_text_nodes(paragraph, &mut out);
      }
    }
  }
  out
}

fn paragraph_text(paragraph: &Paragraph) -> String {
  paragraph
    .paragraph_choice
    .iter()
    .filter_map(|choice| {
      let ParagraphChoice::WRun(run) = choice else {
        return None;
      };
      Some(run)
    })
    .flat_map(|run| &run.run_choice)
    .filter_map(|choice| {
      let RunChoice::Text(text) = choice else {
        return None;
      };
      text.0.xml_content.as_deref()
    })
    .collect()
}

fn paragraph_text_nodes<'a>(paragraph: &'a Paragraph, out: &mut Vec<&'a Text>) {
  for choice in &paragraph.paragraph_choice {
    let ParagraphChoice::WRun(run) = choice else {
      continue;
    };
    for run_choice in &run.run_choice {
      if let RunChoice::Text(text) = run_choice {
        out.push(text);
      }
    }
  }
}

fn paragraph_has_drawing(paragraph: &Paragraph) -> bool {
  paragraph.paragraph_choice.iter().any(|choice| {
    matches!(
      choice,
      ParagraphChoice::WRun(run)
        if run
          .run_choice
          .iter()
          .any(|run_choice| matches!(run_choice, RunChoice::Drawing(_)))
    )
  })
}

fn text_needs_preserve(text: &str) -> bool {
  text.starts_with(char::is_whitespace) || text.ends_with(char::is_whitespace)
}
