mod docx;
mod pdf;

use std::io;

use anyhow::{Context, Result};
use clap::{Arg, Command};
use mdbook_renderer::book::Book;
use mdbook_renderer::{RenderContext, Renderer};

fn main() -> Result<()> {
    let matches = cli().get_matches();

    match matches.subcommand() {
        Some(("preprocess", matches)) => match matches.subcommand() {
            Some(("supports", matches)) => {
                let renderer = matches
                    .get_one::<String>("renderer")
                    .expect("required by clap");
                run_preprocessor_supports(renderer)
            }
            _ => run_preprocessor(),
        },
        Some(("render-pdf", _)) => run_backend(PdfBackend),
        Some(("render-docx", _)) => run_backend(DocxBackend),
        _ => unreachable!("subcommand is required by clap"),
    }
}

fn cli() -> Command {
    Command::new("mdbook-renderkit")
        .about("mdBook preprocessor and PDF/DOCX backends")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(
            Command::new("preprocess")
                .about("Run the mdBook preprocessor")
                .subcommand(
                    Command::new("supports")
                        .about("Report whether the preprocessor supports a renderer")
                        .arg(Arg::new("renderer").required(true)),
                ),
        )
        .subcommand(Command::new("render-pdf").about("Run the mdBook PDF backend"))
        .subcommand(Command::new("render-docx").about("Run the mdBook DOCX backend"))
}

fn run_preprocessor_supports(renderer: &str) -> Result<()> {
    match renderer {
        "pdf" | "docx" | "render-pdf" | "render-docx" => Ok(()),
        _ => std::process::exit(1),
    }
}

fn run_preprocessor() -> Result<()> {
    let stdin = io::stdin();
    let (_ctx, book): (serde_json::Value, Book) = serde_json::from_reader(stdin.lock())
        .context("failed to read mdBook preprocessor input")?;

    let stdout = io::stdout();
    serde_json::to_writer(stdout.lock(), &book).context("failed to write preprocessed book")?;

    Ok(())
}

fn run_backend(renderer: impl Renderer) -> Result<()> {
    let stdin = io::stdin();
    let ctx = RenderContext::from_json(stdin.lock())?;
    renderer.render(&ctx)
}

struct PdfBackend;

impl Renderer for PdfBackend {
    fn name(&self) -> &str {
        "pdf"
    }

    fn render(&self, ctx: &RenderContext) -> mdbook_renderer::errors::Result<()> {
        pdf::render(ctx).map_err(Into::into)
    }
}

struct DocxBackend;

impl Renderer for DocxBackend {
    fn name(&self) -> &str {
        "docx"
    }

    fn render(&self, ctx: &RenderContext) -> mdbook_renderer::errors::Result<()> {
        docx::render(ctx).map_err(Into::into)
    }
}
