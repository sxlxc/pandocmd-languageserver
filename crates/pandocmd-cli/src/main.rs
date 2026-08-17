use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use pandocmd_analysis::{AnalyzeOptions, DocumentAnalysis, WorkspaceIndex};
use pandocmd_extensions::{ExtensionSet, Flavor};
use pandocmd_pandoc::PandocValidator;
use pandocmd_syntax::PandocMarkdownParser;

#[derive(Debug, Parser)]
#[command(name = "pandocmd")]
#[command(about = "Debug tools for the Pandoc Markdown language server")]
struct Cli {
    /// Pandoc flavor preset (markdown, gfm, commonmark, ...).
    #[arg(long, default_value = "markdown")]
    flavor: String,

    /// Extension diff applied to the flavor, e.g. "+citations-smart".
    #[arg(long, default_value = "")]
    extensions: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Parse { file: PathBuf },
    Symbols { file: PathBuf },
    Diagnose { file: PathBuf },
    Extensions,
    Pandoc { file: PathBuf },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let analyze_options = analyze_options(&cli.flavor, &cli.extensions)?;

    match cli.command {
        Command::Parse { file } => {
            let document = parse_file(&file)?;
            println!("{}", document.tree().root_node().to_sexp());
        }
        Command::Symbols { file } => {
            let document = parse_file(&file)?;
            let analysis = analyze(&file, &document, &analyze_options);
            let mut symbols = analysis
                .headings
                .iter()
                .map(|heading| {
                    (
                        heading.range.start,
                        format!(
                            "{} {} #{}",
                            "#".repeat(heading.level as usize),
                            heading.title,
                            heading.anchor.as_deref().unwrap_or("")
                        ),
                    )
                })
                .chain(
                    analysis
                        .fenced_divs
                        .iter()
                        .map(|div| (div.range.start, format!("::: {}", div.detail()))),
                )
                .collect::<Vec<_>>();
            symbols.sort_by_key(|(start, _)| *start);
            for (_, symbol) in symbols {
                println!("{symbol}");
            }
        }
        Command::Diagnose { file } => {
            let document = parse_file(&file)?;
            let analysis = analyze(&file, &document, &analyze_options);
            for diagnostic in analysis.diagnostics {
                let (start, _) = document
                    .line_index()
                    .range_to_positions(document.text(), diagnostic.range);
                println!(
                    "{}:{}: {} [{}]",
                    start.line + 1,
                    start.character + 1,
                    diagnostic.message,
                    diagnostic.code
                );
            }
        }
        Command::Extensions => {
            println!("{:<32} {:<8} DESCRIPTION", "EXTENSION", "DEFAULT");
            for flavor in [
                Flavor::Markdown,
                Flavor::Gfm,
                Flavor::CommonMark,
                Flavor::CommonMarkX,
                Flavor::MarkdownStrict,
                Flavor::MarkdownMmd,
                Flavor::MarkdownPhpextra,
            ] {
                let defaults = ExtensionSet::flavor_defaults(flavor);
                println!("\n[{}]", flavor.name());
                for extension in pandocmd_extensions::Extension::ALL {
                    println!(
                        "{:<32} {:<8} {}",
                        extension.name(),
                        if defaults.contains(*extension) {
                            "on"
                        } else {
                            "off"
                        },
                        extension.description()
                    );
                }
            }
        }
        Command::Pandoc { file } => {
            let text = std::fs::read_to_string(&file)
                .with_context(|| format!("failed to read {}", file.display()))?;
            let Some(validator) = PandocValidator::detect() else {
                anyhow::bail!("pandoc executable was not found on PATH");
            };
            for diagnostic in validator.validate_markdown(&text)? {
                println!("{}", diagnostic.message);
            }
        }
    }

    Ok(())
}

fn analyze_options(flavor: &str, diff: &str) -> Result<AnalyzeOptions> {
    let spec = format!("{flavor}{diff}");
    let parsed = pandocmd_extensions::parse_format_spec(&spec)?;
    Ok(AnalyzeOptions::with_extensions(parsed.extensions))
}

fn analyze(
    file: &std::path::Path,
    document: &pandocmd_syntax::ParsedDocument,
    options: &AnalyzeOptions,
) -> DocumentAnalysis {
    let base_workspace = file
        .parent()
        .map(WorkspaceIndex::from_root)
        .unwrap_or_else(WorkspaceIndex::empty);
    let workspace = base_workspace.for_document_with_extensions(
        Some(file),
        document.text(),
        options.extensions,
    );
    DocumentAnalysis::analyze(document, &workspace, options)
}

fn parse_file(file: &std::path::Path) -> Result<pandocmd_syntax::ParsedDocument> {
    let text = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let mut parser = PandocMarkdownParser::new()?;
    parser.parse(text).map_err(Into::into)
}
