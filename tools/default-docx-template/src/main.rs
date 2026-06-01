use std::env;
use std::fs::File;
use std::path::PathBuf;

use ooxmlsdk::common::{XmlNamespace as XmlNamespaceDecl, XmlNamespaceUri, XmlPrefix};
use ooxmlsdk::namespaces::XmlKnownNamespace;
use ooxmlsdk::parts::style_definitions_part::StyleDefinitionsPart;
use ooxmlsdk::parts::wordprocessing_document::WordprocessingDocument;
use ooxmlsdk::schemas::schemas_openxmlformats_org_wordprocessingml_2006_main::{
  BasedOn, Body, BodyChoice, Bold, BoldComplexScript, BorderValues, BottomBorder, BottomMargin,
  Break, BreakValues, Color, Document, FieldChar, FieldCharValues, FieldCode, FontSize,
  FontSizeComplexScript, Indentation, InsideHorizontalBorder, InsideVerticalBorder, Justification,
  JustificationValues, KeepLines, KeepNext, LatentStyles, LeftBorder, NextParagraphStyle,
  OutlineLevel, PageMargin, PageSize, Paragraph, ParagraphChoice, ParagraphProperties,
  ParagraphStyleId, RightBorder, Run, RunChoice, RunFonts, RunPropertiesBaseStyle,
  SectionProperties, Shading, ShadingPatternValues, SpacingBetweenLines, Style as WordStyle,
  StyleName, StyleParagraphProperties, StyleRunProperties, StyleTableProperties, StyleValues,
  Styles, TabStop, TabStopLeaderCharValues, TabStopValues, TableBorders, TableCellBorders,
  TableCellLeftMargin, TableCellMarginDefault, TableCellRightMargin, TableCellVerticalAlignment,
  TableJustification, TableRowAlignmentValues, TableStyleOverrideValues, TableStyleProperties,
  TableVerticalAlignmentValues, TableWidthUnitValues, Tabs, Text, TextType, TopBorder, TopMargin,
  Underline, UnderlineValues,
};
use ooxmlsdk::schemas::www_w3_org_xml_1998_namespace::SpaceProcessingModeValues;
use ooxmlsdk::sdk::WordprocessingDocumentType;
use ooxmlsdk::simple_type::{
  MeasurementOrPercentValue, OnOffValue, SignedTwipsMeasureValue, TwipsMeasureValue,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const TITLE_MARKER: &str = "MDBOOK_RENDERKIT_TITLE";
const AUTHOR_MARKER: &str = "MDBOOK_RENDERKIT_AUTHOR";
const CONTENT_MARKER: &str = "MDBOOK_RENDERKIT_CONTENT";
const DEFAULT_OUTPUT: &str = "src/assets/template.docx";
const PAGE_WIDTH_TWIPS: u64 = 11906;
const PAGE_HEIGHT_TWIPS: u64 = 16838;
const PAGE_MARGIN_TWIPS: u64 = 1361;
const CONTENT_WIDTH_TWIPS: i64 = 9184;

fn main() -> Result<()> {
  let output = parse_output()?;
  let mut package = WordprocessingDocument::create(WordprocessingDocumentType::Document);
  let main_part = package.add_main_document_part()?;
  let style_part = package.add_new_part_auto_id::<StyleDefinitionsPart>()?;
  let style_part = main_part.add_part(&mut package, style_part)?;

  main_part.set_root_element(&mut package, document())?;
  style_part.set_root_element(&mut package, styles())?;

  let file = File::create(&output)?;
  package.save(file)?;
  eprintln!("wrote {}", output.display());
  Ok(())
}

fn parse_output() -> Result<PathBuf> {
  let mut args = env::args().skip(1);
  let mut output = PathBuf::from(DEFAULT_OUTPUT);
  while let Some(arg) = args.next() {
    match arg.as_str() {
      "--output" => output = args.next().ok_or("--output requires a path")?.into(),
      "-h" | "--help" => {
        eprintln!(
          "Usage: cargo run --manifest-path tools/default-docx-template/Cargo.toml -- [--output PATH]"
        );
        std::process::exit(0);
      }
      _ => return Err(format!("unknown argument {arg}").into()),
    }
  }
  Ok(output)
}

fn document() -> Document {
  Document {
    xmlns: vec![known_xmlns(XmlKnownNamespace::W)],
    body: Some(Box::new(Body {
      body_choice: vec![
        BodyChoice::Paragraph(Box::new(marker_paragraph(TITLE_MARKER, "RenderkitTitle"))),
        BodyChoice::Paragraph(Box::new(marker_paragraph(AUTHOR_MARKER, "RenderkitAuthor"))),
        BodyChoice::Paragraph(Box::new(page_break_paragraph())),
        BodyChoice::Paragraph(Box::new(text_paragraph("Contents", "RenderkitTocHeading"))),
        BodyChoice::Paragraph(Box::new(toc_field_paragraph())),
        BodyChoice::Paragraph(Box::new(page_break_paragraph())),
        BodyChoice::Paragraph(Box::new(marker_paragraph(CONTENT_MARKER, "RenderkitBody"))),
      ],
      section_properties: Some(Box::new(section_properties())),
    })),
    ..Default::default()
  }
}

fn styles() -> Styles {
  let mut styles = Styles {
    xmlns: vec![known_xmlns(XmlKnownNamespace::W)],
    latent_styles: Some(LatentStyles {
      default_locked_state: Some(OnOffValue::False),
      default_ui_priority: Some(99),
      default_semi_hidden: Some(OnOffValue::True),
      default_unhide_when_used: Some(OnOffValue::True),
      default_primary_style: Some(OnOffValue::False),
      count: Some(260),
      ..Default::default()
    }),
    ..Default::default()
  };

  styles.style.push(paragraph_style(ParagraphStyle {
    id: "RenderkitTitle",
    name: "RenderkitTitle",
    based_on: None,
    next: Some("RenderkitAuthor"),
    size: "36",
    bold: true,
    justification: Some(JustificationValues::Center),
    spacing_before: Some(0),
    spacing_after: Some(240),
    line: None,
    first_line: None,
    left: None,
    outline_level: None,
    keep_next: false,
    shading: None,
  }));
  styles.style.push(paragraph_style(ParagraphStyle {
    id: "RenderkitAuthor",
    name: "RenderkitAuthor",
    based_on: Some("RenderkitBody"),
    next: Some("RenderkitBody"),
    size: "22",
    bold: false,
    justification: Some(JustificationValues::Center),
    spacing_before: Some(0),
    spacing_after: Some(0),
    line: Some(264),
    first_line: Some(0),
    left: None,
    outline_level: None,
    keep_next: false,
    shading: None,
  }));
  styles.style.push(paragraph_style(ParagraphStyle {
    id: "RenderkitBody",
    name: "RenderkitBody",
    based_on: None,
    next: Some("RenderkitBody"),
    size: "22",
    bold: false,
    justification: None,
    spacing_before: Some(0),
    spacing_after: Some(120),
    line: Some(264),
    first_line: None,
    left: None,
    outline_level: None,
    keep_next: false,
    shading: None,
  }));
  styles.style.push(paragraph_style(ParagraphStyle {
    id: "RenderkitTocHeading",
    name: "RenderkitTocHeading",
    based_on: Some("RenderkitBody"),
    next: Some("RenderkitBody"),
    size: "28",
    bold: true,
    justification: Some(JustificationValues::Center),
    spacing_before: Some(0),
    spacing_after: Some(240),
    line: None,
    first_line: None,
    left: None,
    outline_level: None,
    keep_next: false,
    shading: None,
  }));
  styles.style.push(paragraph_style(ParagraphStyle {
    id: "RenderkitCode",
    name: "RenderkitCode",
    based_on: Some("RenderkitBody"),
    next: Some("RenderkitBody"),
    size: "20",
    bold: false,
    justification: None,
    spacing_before: Some(80),
    spacing_after: Some(80),
    line: Some(240),
    first_line: Some(0),
    left: Some(0),
    outline_level: None,
    keep_next: false,
    shading: Some("F5F5F5"),
  }));
  styles.style.push(character_style(CharacterStyle {
    id: "RenderkitInlineCode",
    name: "RenderkitInlineCode",
    fonts: code_fonts(),
    size: "20",
    bold: false,
    shading: Some("F5F5F5"),
    color: None,
    underline: false,
  }));
  styles.style.push(paragraph_style(ParagraphStyle {
    id: "RenderkitImage",
    name: "RenderkitImage",
    based_on: Some("RenderkitBody"),
    next: Some("RenderkitBody"),
    size: "22",
    bold: false,
    justification: Some(JustificationValues::Center),
    spacing_before: Some(80),
    spacing_after: Some(120),
    line: None,
    first_line: Some(0),
    left: Some(0),
    outline_level: None,
    keep_next: false,
    shading: None,
  }));
  styles.style.push(paragraph_style(ParagraphStyle {
    id: "RenderkitList",
    name: "RenderkitList",
    based_on: Some("RenderkitBody"),
    next: Some("RenderkitList"),
    size: "22",
    bold: false,
    justification: None,
    spacing_before: Some(0),
    spacing_after: Some(80),
    line: Some(264),
    first_line: Some(0),
    left: None,
    outline_level: None,
    keep_next: false,
    shading: None,
  }));
  styles.style.push(paragraph_style(ParagraphStyle {
    id: "RenderkitQuote",
    name: "RenderkitQuote",
    based_on: Some("RenderkitBody"),
    next: Some("RenderkitBody"),
    size: "22",
    bold: false,
    justification: None,
    spacing_before: Some(0),
    spacing_after: Some(120),
    line: Some(264),
    first_line: Some(0),
    left: Some(360),
    outline_level: None,
    keep_next: false,
    shading: None,
  }));
  styles.style.push(table_style());
  styles.style.push(character_style(CharacterStyle {
    id: "RenderkitHyperlink",
    name: "RenderkitHyperlink",
    fonts: body_fonts(),
    size: "22",
    bold: false,
    shading: None,
    color: Some("0563C1"),
    underline: true,
  }));

  for level in 1..=9 {
    styles.style.push(heading_style(level));
    styles.style.push(toc_style(level));
  }

  styles
}

struct ParagraphStyle<'a> {
  id: &'a str,
  name: &'a str,
  based_on: Option<&'a str>,
  next: Option<&'a str>,
  size: &'a str,
  bold: bool,
  justification: Option<JustificationValues>,
  spacing_before: Option<i64>,
  spacing_after: Option<i64>,
  line: Option<i64>,
  first_line: Option<i64>,
  left: Option<i64>,
  outline_level: Option<i32>,
  keep_next: bool,
  shading: Option<&'a str>,
}

fn paragraph_style(spec: ParagraphStyle<'_>) -> WordStyle {
  WordStyle {
    r#type: Some(StyleValues::Paragraph),
    style_id: Some(spec.id.to_string()),
    custom_style: Some(OnOffValue::True),
    style_name: Some(StyleName {
      val: spec.name.to_string(),
    }),
    based_on: spec.based_on.map(|val| BasedOn {
      val: val.to_string(),
    }),
    next_paragraph_style: spec.next.map(|val| NextParagraphStyle {
      val: val.to_string(),
    }),
    style_paragraph_properties: Some(Box::new(StyleParagraphProperties {
      spacing_between_lines: Some(SpacingBetweenLines {
        before: spec.spacing_before.map(twips_measure),
        after: spec.spacing_after.map(twips_measure),
        line: spec.line.map(signed_twips),
        ..Default::default()
      }),
      indentation: (spec.first_line.is_some() || spec.left.is_some()).then(|| Indentation {
        first_line: spec.first_line.map(twips_measure),
        left: spec.left.map(signed_twips),
        ..Default::default()
      }),
      justification: spec.justification.map(|val| Justification { val }),
      outline_level: spec.outline_level.map(|val| OutlineLevel { val }),
      keep_next: spec.keep_next.then_some(KeepNext::default()),
      keep_lines: spec.keep_next.then_some(KeepLines::default()),
      shading: spec.shading.map(fill_shading),
      ..Default::default()
    })),
    style_run_properties: Some(Box::new(style_run_properties(
      body_fonts(),
      spec.size,
      spec.bold,
      None,
      None,
      false,
    ))),
    ..Default::default()
  }
}

fn heading_style(level: usize) -> WordStyle {
  let size = match level {
    1 => "32",
    2 => "28",
    3 => "24",
    _ => "22",
  };
  paragraph_style(ParagraphStyle {
    id: &format!("RenderkitHeading{level}"),
    name: &format!("RenderkitHeading{level}"),
    based_on: Some("RenderkitBody"),
    next: Some("RenderkitBody"),
    size,
    bold: true,
    justification: None,
    spacing_before: Some(if level == 1 { 360 } else { 240 }),
    spacing_after: Some(120),
    line: None,
    first_line: Some(0),
    left: None,
    outline_level: Some((level - 1) as i32),
    keep_next: true,
    shading: None,
  })
}

fn toc_style(level: usize) -> WordStyle {
  paragraph_style(ParagraphStyle {
    id: &format!("RenderkitToc{level}"),
    name: &format!("RenderkitToc{level}"),
    based_on: Some("RenderkitBody"),
    next: Some("RenderkitBody"),
    size: "22",
    bold: false,
    justification: None,
    spacing_before: Some(0),
    spacing_after: Some(80),
    line: None,
    first_line: Some(0),
    left: Some(((level.saturating_sub(1)).min(2) as i64) * 240),
    outline_level: None,
    keep_next: false,
    shading: None,
  })
}

struct CharacterStyle<'a> {
  id: &'a str,
  name: &'a str,
  fonts: RunFonts,
  size: &'a str,
  bold: bool,
  shading: Option<&'a str>,
  color: Option<&'a str>,
  underline: bool,
}

fn character_style(spec: CharacterStyle<'_>) -> WordStyle {
  WordStyle {
    r#type: Some(StyleValues::Character),
    style_id: Some(spec.id.to_string()),
    custom_style: Some(OnOffValue::True),
    style_name: Some(StyleName {
      val: spec.name.to_string(),
    }),
    style_run_properties: Some(Box::new(style_run_properties(
      spec.fonts,
      spec.size,
      spec.bold,
      spec.shading,
      spec.color,
      spec.underline,
    ))),
    ..Default::default()
  }
}

fn table_style() -> WordStyle {
  WordStyle {
    r#type: Some(StyleValues::Table),
    style_id: Some("RenderkitTable".to_string()),
    custom_style: Some(OnOffValue::True),
    style_name: Some(StyleName {
      val: "RenderkitTable".to_string(),
    }),
    style_table_properties: Some(Box::new(StyleTableProperties {
      table_justification: Some(TableJustification {
        val: TableRowAlignmentValues::Center,
      }),
      table_borders: Some(Box::new(table_borders())),
      table_cell_margin_default: Some(Box::new(TableCellMarginDefault {
        top_margin: Some(TopMargin {
          width: Some(pct(80)),
          r#type: Some(TableWidthUnitValues::Dxa),
        }),
        table_cell_left_margin: Some(TableCellLeftMargin {
          width: Some(pct(120)),
          r#type: Some(TableWidthUnitValues::Dxa),
        }),
        bottom_margin: Some(BottomMargin {
          width: Some(pct(80)),
          r#type: Some(TableWidthUnitValues::Dxa),
        }),
        table_cell_right_margin: Some(TableCellRightMargin {
          width: Some(pct(120)),
          r#type: Some(TableWidthUnitValues::Dxa),
        }),
        ..Default::default()
      })),
      ..Default::default()
    })),
    table_style_properties: vec![TableStyleProperties {
      r#type: TableStyleOverrideValues::FirstRow,
      run_properties_base_style: Some(Box::new(run_base_properties(body_fonts(), "22", true))),
      table_style_conditional_formatting_table_cell_properties: Some(Box::new(
        ooxmlsdk::schemas::schemas_openxmlformats_org_wordprocessingml_2006_main::TableStyleConditionalFormattingTableCellProperties {
          table_cell_borders: Some(Box::new(table_cell_borders())),
          shading: Some(fill_shading("F5F5F5")),
          table_cell_vertical_alignment: Some(TableCellVerticalAlignment {
            val: TableVerticalAlignmentValues::Center,
          }),
          ..Default::default()
        },
      )),
      ..Default::default()
    }],
    ..Default::default()
  }
}

fn style_run_properties(
  fonts: RunFonts,
  size: &str,
  bold: bool,
  shading: Option<&str>,
  color: Option<&str>,
  underline: bool,
) -> StyleRunProperties {
  let mut properties = StyleRunProperties {
    run_fonts: Some(fonts),
    font_size: Some(FontSize {
      val: size.to_string(),
    }),
    font_size_complex_script: Some(FontSizeComplexScript {
      val: size.to_string(),
    }),
    shading: shading.map(fill_shading),
    color: color.map(|val| Color {
      val: val.to_string(),
      ..Default::default()
    }),
    underline: underline.then_some(Underline {
      val: Some(UnderlineValues::Single),
      ..Default::default()
    }),
    ..Default::default()
  };
  if bold {
    properties.bold = Some(Bold::default());
    properties.bold_complex_script = Some(BoldComplexScript::default());
  }
  properties
}

fn run_base_properties(fonts: RunFonts, size: &str, bold: bool) -> RunPropertiesBaseStyle {
  RunPropertiesBaseStyle {
    run_fonts: Some(fonts),
    font_size: Some(FontSize {
      val: size.to_string(),
    }),
    font_size_complex_script: Some(FontSizeComplexScript {
      val: size.to_string(),
    }),
    bold: bold.then_some(Bold::default()),
    bold_complex_script: bold.then_some(BoldComplexScript::default()),
    ..Default::default()
  }
}

fn marker_paragraph(marker: &str, style: &str) -> Paragraph {
  Paragraph {
    paragraph_properties: Some(Box::new(paragraph_properties(style))),
    paragraph_choice: vec![ParagraphChoice::WRun(Box::new(Run {
      run_choice: vec![RunChoice::Text(Box::new(Text(TextType {
        xml_content: Some(marker.to_string()),
        ..Default::default()
      })))],
      ..Default::default()
    }))],
    ..Default::default()
  }
}

fn text_paragraph(text: &str, style: &str) -> Paragraph {
  Paragraph {
    paragraph_properties: Some(Box::new(paragraph_properties(style))),
    paragraph_choice: vec![ParagraphChoice::WRun(Box::new(Run {
      run_choice: vec![RunChoice::Text(Box::new(Text(TextType {
        xml_content: Some(text.to_string()),
        ..Default::default()
      })))],
      ..Default::default()
    }))],
    ..Default::default()
  }
}

fn toc_field_paragraph() -> Paragraph {
  Paragraph {
    paragraph_properties: Some(Box::new(ParagraphProperties {
      paragraph_style_id: Some(ParagraphStyleId {
        val: "RenderkitToc1".to_string(),
      }),
      tabs: Some(Tabs {
        tab_stop: vec![TabStop {
          val: TabStopValues::Right,
          leader: Some(TabStopLeaderCharValues::Dot),
          position: SignedTwipsMeasureValue::Twips(CONTENT_WIDTH_TWIPS),
        }],
      }),
      ..Default::default()
    })),
    paragraph_choice: vec![
      ParagraphChoice::WRun(Box::new(Run {
        run_choice: vec![RunChoice::FieldChar(Box::new(FieldChar {
          field_char_type: FieldCharValues::Begin,
          ..Default::default()
        }))],
        ..Default::default()
      })),
      ParagraphChoice::WRun(Box::new(Run {
        run_choice: vec![RunChoice::FieldCode(Box::new(FieldCode {
          space: Some(SpaceProcessingModeValues::Preserve),
          xml_content: Some(r#" TOC \o "1-3" \h \z \u "#.to_string()),
        }))],
        ..Default::default()
      })),
      ParagraphChoice::WRun(Box::new(Run {
        run_choice: vec![RunChoice::FieldChar(Box::new(FieldChar {
          field_char_type: FieldCharValues::Separate,
          ..Default::default()
        }))],
        ..Default::default()
      })),
      ParagraphChoice::WRun(Box::new(Run {
        run_choice: vec![RunChoice::Text(Box::new(Text(TextType {
          xml_content: Some("Right-click to update field.".to_string()),
          ..Default::default()
        })))],
        ..Default::default()
      })),
      ParagraphChoice::WRun(Box::new(Run {
        run_choice: vec![RunChoice::FieldChar(Box::new(FieldChar {
          field_char_type: FieldCharValues::End,
          ..Default::default()
        }))],
        ..Default::default()
      })),
    ],
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

fn paragraph_properties(style: &str) -> ParagraphProperties {
  ParagraphProperties {
    paragraph_style_id: Some(ParagraphStyleId {
      val: style.to_string(),
    }),
    ..Default::default()
  }
}

fn section_properties() -> SectionProperties {
  SectionProperties {
    page_size: Some(PageSize {
      width: Some(TwipsMeasureValue::Twips(PAGE_WIDTH_TWIPS)),
      height: Some(TwipsMeasureValue::Twips(PAGE_HEIGHT_TWIPS)),
      ..Default::default()
    }),
    page_margin: Some(PageMargin {
      top: Some(SignedTwipsMeasureValue::Twips(PAGE_MARGIN_TWIPS as i64)),
      right: Some(TwipsMeasureValue::Twips(PAGE_MARGIN_TWIPS)),
      bottom: Some(SignedTwipsMeasureValue::Twips(PAGE_MARGIN_TWIPS as i64)),
      left: Some(TwipsMeasureValue::Twips(PAGE_MARGIN_TWIPS)),
      header: Some(TwipsMeasureValue::Twips(720)),
      footer: Some(TwipsMeasureValue::Twips(720)),
      gutter: Some(TwipsMeasureValue::Twips(0)),
    }),
    ..Default::default()
  }
}

fn body_fonts() -> RunFonts {
  RunFonts {
    ascii: Some("Arial".to_string()),
    high_ansi: Some("Arial".to_string()),
    east_asia: Some("Microsoft YaHei".to_string()),
    complex_script: Some("Arial".to_string()),
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

fn fill_shading(fill: &str) -> Shading {
  Shading {
    val: ShadingPatternValues::Clear,
    color: Some("auto".to_string()),
    fill: Some(fill.to_string()),
    ..Default::default()
  }
}

fn table_borders() -> TableBorders {
  TableBorders {
    top_border: Some(TopBorder {
      val: BorderValues::Single,
      color: Some("DDDDDD".to_string()),
      size: Some(4),
      space: Some(0),
      ..Default::default()
    }),
    left_border: Some(LeftBorder {
      val: BorderValues::Single,
      color: Some("DDDDDD".to_string()),
      size: Some(4),
      space: Some(0),
      ..Default::default()
    }),
    bottom_border: Some(BottomBorder {
      val: BorderValues::Single,
      color: Some("DDDDDD".to_string()),
      size: Some(4),
      space: Some(0),
      ..Default::default()
    }),
    right_border: Some(RightBorder {
      val: BorderValues::Single,
      color: Some("DDDDDD".to_string()),
      size: Some(4),
      space: Some(0),
      ..Default::default()
    }),
    inside_horizontal_border: Some(InsideHorizontalBorder {
      val: BorderValues::Single,
      color: Some("DDDDDD".to_string()),
      size: Some(4),
      space: Some(0),
      ..Default::default()
    }),
    inside_vertical_border: Some(InsideVerticalBorder {
      val: BorderValues::Single,
      color: Some("DDDDDD".to_string()),
      size: Some(4),
      space: Some(0),
      ..Default::default()
    }),
    ..Default::default()
  }
}

fn table_cell_borders() -> TableCellBorders {
  TableCellBorders {
    top_border: Some(TopBorder {
      val: BorderValues::Single,
      color: Some("DDDDDD".to_string()),
      size: Some(4),
      space: Some(0),
      ..Default::default()
    }),
    left_border: Some(LeftBorder {
      val: BorderValues::Single,
      color: Some("DDDDDD".to_string()),
      size: Some(4),
      space: Some(0),
      ..Default::default()
    }),
    bottom_border: Some(BottomBorder {
      val: BorderValues::Single,
      color: Some("DDDDDD".to_string()),
      size: Some(4),
      space: Some(0),
      ..Default::default()
    }),
    right_border: Some(RightBorder {
      val: BorderValues::Single,
      color: Some("DDDDDD".to_string()),
      size: Some(4),
      space: Some(0),
      ..Default::default()
    }),
    inside_horizontal_border: Some(InsideHorizontalBorder {
      val: BorderValues::Single,
      color: Some("DDDDDD".to_string()),
      size: Some(4),
      space: Some(0),
      ..Default::default()
    }),
    inside_vertical_border: Some(InsideVerticalBorder {
      val: BorderValues::Single,
      color: Some("DDDDDD".to_string()),
      size: Some(4),
      space: Some(0),
      ..Default::default()
    }),
    ..Default::default()
  }
}

fn pct(value: i64) -> MeasurementOrPercentValue {
  MeasurementOrPercentValue::from_bytes(value.to_string().as_bytes()).expect("valid dxa value")
}

fn signed_twips(value: i64) -> SignedTwipsMeasureValue {
  SignedTwipsMeasureValue::Twips(value)
}

fn twips_measure(value: i64) -> TwipsMeasureValue {
  TwipsMeasureValue::Twips(value.max(0) as u64)
}

fn known_xmlns(namespace: XmlKnownNamespace) -> XmlNamespaceDecl {
  XmlNamespaceDecl {
    prefix: XmlPrefix::new(namespace.prefix_bytes()),
    uri: XmlNamespaceUri::Known(namespace),
  }
}
