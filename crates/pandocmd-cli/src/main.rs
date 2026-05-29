use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use pandocmd_analysis::{DocumentAnalysis, WorkspaceIndex};
use pandocmd_pandoc::PandocValidator;
use pandocmd_syntax::PandocMarkdownParser;

#[derive(Debug, Parser)]
#[command(name = "pandocmd")]
#[command(about = "Debug tools for the Pandoc Markdown language server")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Parse { file: PathBuf },
    Symbols { file: PathBuf },
    Diagnose { file: PathBuf },
    Pandoc { file: PathBuf },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Parse { file } => {
            let document = parse_file(&file)?;
            println!("{}", document.tree().root_node().to_sexp());
        }
        Command::Symbols { file } => {
            let document = parse_file(&file)?;
            let analysis = DocumentAnalysis::analyze(&document, &WorkspaceIndex::empty());
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
                            heading.anchor
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
            let base_workspace = file
                .parent()
                .map(WorkspaceIndex::from_root)
                .unwrap_or_else(WorkspaceIndex::empty);
            let workspace = base_workspace.for_document(Some(&file), document.text());
            let analysis = DocumentAnalysis::analyze(&document, &workspace);
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

fn parse_file(file: &PathBuf) -> Result<pandocmd_syntax::ParsedDocument> {
    let text = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let mut parser = PandocMarkdownParser::new()?;
    parser.parse(text).map_err(Into::into)
}
