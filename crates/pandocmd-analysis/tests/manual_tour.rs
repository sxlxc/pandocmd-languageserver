//! Fixture-driven tests against the Pandoc User's Guide feature tour.
//!
//! The fixture `tests/fixtures/manual-tour.md` exercises as many Pandoc
//! Markdown constructs as possible. These tests verify that:
//!
//! 1. the language server analyzes the document with zero diagnostics,
//! 2. key constructs (headings, divs, citations, footnotes, links, tables)
//!    are extracted correctly,
//! 3. every generated heading identifier matches an installed pandoc's
//!    output for the same document (ground truth; skipped without pandoc).

use std::path::Path;
use std::process::{Command, Stdio};

use pandocmd_analysis::{AnalyzeOptions, DocumentAnalysis, WorkspaceIndex};
use pandocmd_extensions::Flavor;
use pandocmd_syntax::PandocMarkdownParser;

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/manual-tour.md"
);

fn analyze_fixture() -> DocumentAnalysis {
    let text = std::fs::read_to_string(FIXTURE).unwrap();
    let mut parser = PandocMarkdownParser::new().unwrap();
    let document = parser.parse(text).unwrap();
    let fixture_path = Path::new(FIXTURE);
    let workspace = WorkspaceIndex::from_root(fixture_path.parent().unwrap())
        .for_document_with_extensions(
            Some(fixture_path),
            document.text(),
            pandocmd_extensions::ExtensionSet::flavor_defaults(Flavor::Markdown),
        );
    DocumentAnalysis::analyze(&document, &workspace, &AnalyzeOptions::default())
}

#[test]
fn tour_fixture_has_no_diagnostics() {
    let analysis = analyze_fixture();
    assert!(
        analysis.diagnostics.is_empty(),
        "unexpected diagnostics: {:#?}",
        analysis.diagnostics
    );
}

#[test]
fn tour_fixture_extracts_every_construct_family() {
    let analysis = analyze_fixture();

    // Headings with explicit and automatic identifiers.
    assert_eq!(analysis.headings.len(), 14);
    let anchors: Vec<&str> = analysis
        .headings
        .iter()
        .filter_map(|heading| heading.anchor.as_deref())
        .collect();
    for expected in [
        "sec:introduction",
        "sec:inline",
        "sec:links",
        "sec:math",
        "sec:citations",
        "sec:code",
        "sec:divs",
        "sec:tables",
        "sec:lists",
        "sec:quotes",
        "sec:raw",
        "footnotes",
        "definitions",
        "sec:conclusion",
    ] {
        assert!(anchors.contains(&expected), "missing anchor {expected}");
    }
    assert!(
        anchors.contains(&"inline-formatting") || anchors.contains(&"sec:inline"),
        "automatic or explicit id must exist for the inline section"
    );

    // Fenced divs, including nesting and an unbraced class.
    assert!(analysis.fenced_divs.len() >= 3, "fenced divs");
    assert!(analysis
        .fenced_divs
        .iter()
        .any(|div| div.id.as_deref() == Some("panel")));
    assert!(analysis
        .fenced_divs
        .iter()
        .any(|div| div.classes.contains(&"nested".to_string())));
    assert!(analysis
        .fenced_divs
        .iter()
        .any(|div| div.classes.contains(&"lemma".to_string())));

    // Citations including prefix/suffix forms.
    let keys: Vec<&str> = analysis.citations.iter().map(|c| c.key.as_str()).collect();
    assert!(keys.contains(&"doe2004"));
    assert!(keys.contains(&"smith2020"));
    assert!(keys.contains(&"eq:gauss"));
    assert!(keys.iter().filter(|key| **key == "doe2004").count() >= 2);

    // Footnotes: one long definition, one reference, one inline note.
    assert!(analysis
        .footnote_definitions
        .iter()
        .any(|definition| definition.label == "longnote"));
    assert!(analysis
        .footnote_references
        .iter()
        .any(|reference| reference.label == "longnote"));
    assert!(analysis
        .inline_notes
        .iter()
        .any(|note| note.content.contains("shorter")));

    // Reference links of all three kinds.
    for label in ["ref-full", "ref-collapsed", "ref-shortcut"] {
        assert!(
            analysis
                .reference_definitions
                .iter()
                .any(|d| d.label == label),
            "missing reference definition {label}"
        );
    }

    // Local cross-reference targets for spans, listings, equations, tables.
    for id in [
        "span-example",
        "code-example",
        "lst:demo",
        "eq:gauss",
        "tbl:pipe",
        "tbl:multiline",
    ] {
        assert!(
            analysis.local_reference(id).is_some(),
            "missing local reference {id}"
        );
    }

    // Links: inline, autolink, image, definitions.
    assert!(analysis
        .links
        .iter()
        .any(|link| link.kind == pandocmd_analysis::LinkKind::Autolink));
    assert!(analysis
        .links
        .iter()
        .any(|link| link.target == "figure.png"));
    assert!(analysis
        .links
        .iter()
        .any(|link| link.target.starts_with("https://example.com/full")));

    // Code fences are opaque: no diagnostics and no bogus citations from
    // the tilde block that mentions them literally.
    assert!(analysis
        .citations
        .iter()
        .all(|citation| citation.key != "not-a-citation"));
}

/// Every heading identifier we generate must byte-match what pandoc itself
/// generates for the same document. Skipped when pandoc is not installed.
#[test]
fn tour_heading_identifiers_match_installed_pandoc() {
    if !pandoc_available() {
        eprintln!("skipping: pandoc not installed");
        return;
    }

    let text = std::fs::read_to_string(FIXTURE).unwrap();
    let mut parser = PandocMarkdownParser::new().unwrap();
    let document = parser.parse(text.clone()).unwrap();
    let analysis = DocumentAnalysis::analyze(
        &document,
        &WorkspaceIndex::empty(),
        &AnalyzeOptions::with_extensions(pandocmd_extensions::ExtensionSet::flavor_defaults(
            Flavor::Markdown,
        )),
    );

    let output = Command::new("pandoc")
        .args(["--from", "markdown", "--to", "json"])
        .arg(FIXTURE)
        .stderr(Stdio::null())
        .output()
        .expect("pandoc run");
    assert!(output.status.success(), "pandoc failed on fixture");
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();

    let pandoc_headers: Vec<String> = json["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|block| block["t"] == "Header")
        .map(|block| block["c"][1][0].as_str().unwrap().to_string())
        .collect();

    // Explicit ids and automatic slugs must line up one-to-one. Explicit
    // ids pass through verbatim; automatic ones get -1 suffixes on repeats.
    let ours: Vec<String> = analysis
        .headings
        .iter()
        .filter_map(|heading| heading.anchor.clone())
        .collect();

    assert_eq!(
        ours, pandoc_headers,
        "heading identifiers diverge from pandoc"
    );
}

/// The full extension model default set must accept the fixture the same
/// way pandoc does: `pandoc -f markdown` must exit cleanly.
#[test]
fn tour_fixture_is_valid_pandoc_markdown() {
    if !pandoc_available() {
        eprintln!("skipping: pandoc not installed");
        return;
    }
    let output = Command::new("pandoc")
        .args(["--from", "markdown", "--to", "native"])
        .arg(FIXTURE)
        .stderr(Stdio::null())
        .output()
        .expect("pandoc run");
    assert!(
        output.status.success(),
        "pandoc rejected the fixture: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(Path::new(FIXTURE).exists());
}

fn pandoc_available() -> bool {
    Command::new("pandoc")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}
