use std::fs::{self, File};
use std::path::Path;

use anyhow::{Context, Result};
use mdbook_renderer::RenderContext;
use mdbook_renderer::book::Chapter;
use ooxmlsdk::common::XmlNamespaceDecl;
use ooxmlsdk::parts::wordprocessing_document::WordprocessingDocument;
use ooxmlsdk::schemas::schemas_openxmlformats_org_wordprocessingml_2006_main::{
    Body, BodyChoice, Bold, Document, Italic, Paragraph, ParagraphChoice, ParagraphProperties,
    ParagraphStyleId, Run, RunChoice, RunFonts, RunProperties, Strike, Text,
};
use ooxmlsdk::sdk::WordprocessingDocumentType;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

pub fn render(ctx: &RenderContext) -> Result<()> {
    fs::create_dir_all(&ctx.destination)
        .with_context(|| format!("failed to create {}", ctx.destination.display()))?;

    let output = ctx.destination.join("book.docx");
    eprintln!(
        "renderkit: rendering {} chapters to {}",
        ctx.book.chapters().count(),
        output.display()
    );

    write_docx(ctx, &output)?;
    eprintln!("renderkit: wrote {}", output.display());

    Ok(())
}

fn write_docx(ctx: &RenderContext, path: &Path) -> Result<()> {
    let mut package = WordprocessingDocument::create(WordprocessingDocumentType::Document);
    let main_part = package.add_main_document_part()?;
    main_part.set_root_element(&mut package, document(ctx)?)?;

    let file =
        File::create(path).with_context(|| format!("failed to create {}", path.display()))?;
    package.save(file)?;

    Ok(())
}

fn document(ctx: &RenderContext) -> Result<Document> {
    let mut body = Body::default();

    if let Some(title) = &ctx.config.book.title {
        body.body_choice
            .push(BodyChoice::WP(Box::new(heading_paragraph(1, title))));
    }

    for chapter in ctx.book.chapters() {
        body.body_choice
            .push(BodyChoice::WP(Box::new(heading_paragraph(
                chapter_level(chapter),
                &chapter.name,
            ))));
        body.body_choice.extend(chapter_body(chapter)?);
    }

    Ok(Document {
        xmlns: vec![XmlNamespaceDecl::new(
            "w",
            "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
        )],
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

fn chapter_body(chapter: &Chapter) -> Result<Vec<BodyChoice>> {
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
    let mut list_depth = 0usize;

    for event in parser {
        if let Some((_, code)) = &mut code_block {
            match event {
                Event::End(TagEnd::CodeBlock) => {
                    let (lang, code) = code_block.take().expect("checked above");
                    let label = if lang.is_empty() {
                        "code".to_string()
                    } else {
                        format!("code: {lang}")
                    };
                    out.push(BodyChoice::WP(Box::new(paragraph_from_text(
                        &label,
                        RunStyle {
                            bold: true,
                            ..Default::default()
                        },
                    ))));
                    for line in code.lines() {
                        out.push(BodyChoice::WP(Box::new(paragraph_from_text(
                            line,
                            RunStyle {
                                code: true,
                                ..Default::default()
                            },
                        ))));
                    }
                }
                Event::Text(text) => code.push_str(&text),
                Event::SoftBreak | Event::HardBreak => code.push('\n'),
                _ => {}
            }
            continue;
        }

        match event {
            Event::Start(Tag::Paragraph) => paragraph = ParagraphBuilder::default(),
            Event::End(TagEnd::Paragraph) => paragraph.flush_to(&mut out),
            Event::Start(Tag::Heading { level, .. }) => {
                paragraph = ParagraphBuilder::with_heading(heading_level(level));
            }
            Event::End(TagEnd::Heading(_)) => paragraph.flush_to(&mut out),
            Event::Start(Tag::Emphasis) => paragraph.style.italic += 1,
            Event::End(TagEnd::Emphasis) => paragraph.style.italic -= 1,
            Event::Start(Tag::Strong) => paragraph.style.bold += 1,
            Event::End(TagEnd::Strong) => paragraph.style.bold -= 1,
            Event::Start(Tag::Strikethrough) => paragraph.style.strike += 1,
            Event::End(TagEnd::Strikethrough) => paragraph.style.strike -= 1,
            Event::Start(Tag::BlockQuote(_)) => {
                paragraph.push_text("> ");
            }
            Event::Start(Tag::List(_)) => list_depth += 1,
            Event::End(TagEnd::List(_)) => list_depth = list_depth.saturating_sub(1),
            Event::Start(Tag::Item) => {
                paragraph = ParagraphBuilder::default();
                paragraph.push_text(&"  ".repeat(list_depth.saturating_sub(1)));
                paragraph.push_text("- ");
            }
            Event::End(TagEnd::Item) => paragraph.flush_to(&mut out),
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
                paragraph = ParagraphBuilder::default();
                paragraph.push_text("Table:");
                paragraph.flush_to(&mut out);
            }
            Event::Start(Tag::TableRow) => paragraph = ParagraphBuilder::default(),
            Event::End(TagEnd::TableRow) => paragraph.flush_to(&mut out),
            Event::End(TagEnd::TableCell) => paragraph.push_text("    "),
            Event::Start(Tag::Link { dest_url, .. }) => {
                paragraph.push_text("[");
                paragraph.link_stack.push(dest_url.to_string());
            }
            Event::End(TagEnd::Link) => {
                if let Some(url) = paragraph.link_stack.pop() {
                    paragraph.push_text("](");
                    paragraph.push_text(&url);
                    paragraph.push_text(")");
                }
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
                paragraph.flush_to(&mut out);
                out.push(BodyChoice::WP(Box::new(paragraph_from_text(
                    "----------------------------------------",
                    RunStyle::default(),
                ))));
            }
            Event::TaskListMarker(checked) => {
                paragraph.push_text(if checked { "[x] " } else { "[ ] " });
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

    paragraph.flush_to(&mut out);
    Ok(out)
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
}

#[derive(Debug, Default)]
struct ParagraphBuilder {
    runs: Vec<Run>,
    style: StyleDepth,
    paragraph_style: Option<String>,
    link_stack: Vec<String>,
}

impl ParagraphBuilder {
    fn with_heading(level: usize) -> Self {
        Self {
            paragraph_style: Some(format!("Heading{}", level.clamp(1, 9))),
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
            code: false,
        }
    }

    fn push_run(&mut self, text: &str, style: RunStyle) {
        if text.is_empty() {
            return;
        }

        for (index, line) in text.split('\n').enumerate() {
            if index > 0 {
                self.runs.push(Run {
                    run_choice: vec![RunChoice::WBr(Box::new(Default::default()))],
                    ..Default::default()
                });
            }
            if !line.is_empty() {
                self.runs.push(text_run(line, style));
            }
        }
    }

    fn flush_to(&mut self, out: &mut Vec<BodyChoice>) {
        if self.runs.is_empty() {
            return;
        }

        out.push(BodyChoice::WP(Box::new(Paragraph {
            paragraph_properties: self
                .paragraph_style
                .take()
                .map(|style| Box::new(paragraph_properties(&style))),
            paragraph_choice: self
                .runs
                .drain(..)
                .map(|run| ParagraphChoice::WR(Box::new(run)))
                .collect(),
            ..Default::default()
        })));
    }
}

fn heading_paragraph(level: usize, text: &str) -> Paragraph {
    Paragraph {
        paragraph_properties: Some(Box::new(paragraph_properties(&format!(
            "Heading{}",
            level.clamp(1, 9)
        )))),
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

fn run_properties(style: RunStyle) -> Option<RunProperties> {
    if !(style.bold || style.italic || style.strike || style.code) {
        return None;
    }

    Some(RunProperties {
        bold: style.bold.then(Bold::default),
        italic: style.italic.then(Italic::default),
        strike: style.strike.then(Strike::default),
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
