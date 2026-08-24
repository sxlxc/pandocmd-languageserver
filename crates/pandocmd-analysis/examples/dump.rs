//! Debug helper: dump DocumentAnalysis facts with byte offsets and line
//! numbers. Usage:
//! `cargo run -p pandocmd-analysis --example dump -- file.md [markdown]`

use pandocmd_analysis::{AnalyzeOptions, DocumentAnalysis, WorkspaceIndex};
use pandocmd_extensions::{ExtensionSet, Flavor};
use pandocmd_syntax::PandocMarkdownParser;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = &args[1];
    let flavor = args
        .get(2)
        .and_then(|name| Flavor::from_name(name))
        .unwrap_or(Flavor::Markdown);
    let text = std::fs::read_to_string(path).unwrap();
    let mut line_starts = vec![0usize];
    for (idx, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            line_starts.push(idx + 1);
        }
    }
    let line_of = |offset: usize| -> usize {
        line_starts
            .iter()
            .rposition(|start| *start <= offset)
            .map(|position| position + 1)
            .unwrap_or(0)
    };

    let mut parser = PandocMarkdownParser::new().unwrap();
    let document = parser.parse(text.clone()).unwrap();
    let options = AnalyzeOptions::with_extensions(ExtensionSet::flavor_defaults(flavor));
    let analysis = DocumentAnalysis::analyze(&document, &WorkspaceIndex::empty(), &options);

    let show = std::env::var("DUMP").unwrap_or_default();
    if show.is_empty() || show.contains("heading") {
        for heading in &analysis.headings {
            println!(
                "H{} {:?} #{} @L{}",
                heading.level,
                heading.title,
                heading.anchor.as_deref().unwrap_or(""),
                line_of(heading.selection_range.start)
            );
        }
    }
    if show.is_empty() || show.contains("citation") {
        for citation in &analysis.citations {
            println!(
                "CITE @{:?} @L{}",
                citation.key,
                line_of(citation.range.start)
            );
        }
    }
    if show.is_empty() || show.contains("link") {
        for definition in &analysis.reference_definitions {
            println!(
                "DEF {:?} -> {:?} @L{}",
                definition.label,
                definition.target,
                line_of(definition.label_range.start)
            );
        }
        for link in &analysis.links {
            println!(
                "LINK {:?} {:?} @L{}",
                format!("{:?}", link.kind),
                link.target,
                line_of(link.range.start)
            );
        }
        for reference in &analysis.reference_links {
            println!(
                "REFLINK {:?} @L{}",
                reference.label,
                line_of(reference.range.start)
            );
        }
    }
    if show.is_empty() || show.contains("div") {
        for div in &analysis.fenced_divs {
            println!(
                "DIV {:?} @L{}",
                div.id.clone().unwrap_or_default(),
                line_of(div.opening_range.start)
            );
        }
    }
    if show.is_empty() || show.contains("note") {
        println!(
            "NOTES refs={} defs={} inline={}",
            analysis.footnote_references.len(),
            analysis.footnote_definitions.len(),
            analysis.inline_notes.len()
        );
        for reference in &analysis.footnote_references {
            println!(
                "NOTEREF {:?} @L{}",
                reference.label,
                line_of(reference.range.start)
            );
        }
    }
}
