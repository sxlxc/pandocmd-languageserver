use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::LazyLock;

use ignore::WalkBuilder;
use pandocmd_syntax::{ParsedDocument, TextRange};
use regex::Regex;

static HEADING_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(#{1,6})[ \t]+(.+?)[ \t#]*$").unwrap());
static FOOTNOTE_DEF_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[ \t]{0,3}\[\^([^\]\n]+)\]:").unwrap());
static REF_DEF_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^[ \t]{0,3}\[([^\]\n]+)\]:[ \t]*(\S+)(?:[ \t]+(.+))?$"#).unwrap()
});
static FULL_REF_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([^\]\n]+)\]\[([^\]\n]+)\]").unwrap());
static COLLAPSED_REF_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([^\]\n]+)\]\[\]").unwrap());
static FOOTNOTE_REF_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\^([^\]\n]+)\]").unwrap());
static HEADING_LINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\]\(#([A-Za-z0-9_.:\-]+)\)").unwrap());
static CITATION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(^|[\s;\[\(])(-?@)([A-Za-z0-9_:.#$%&+\-?<>~/]+)").unwrap());
static BIB_KEY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)@\w+\s*\{\s*([^,\s]+)").unwrap());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Information,
    Hint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub range: TextRange,
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    pub level: u8,
    pub title: String,
    pub anchor: String,
    pub range: TextRange,
    pub selection_range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceDefinition {
    pub label: String,
    pub normalized_label: String,
    pub target: String,
    pub range: TextRange,
    pub label_range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FootnoteDefinition {
    pub label: String,
    pub normalized_label: String,
    pub range: TextRange,
    pub label_range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceLink {
    pub label: String,
    pub normalized_label: String,
    pub range: TextRange,
    pub label_range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FootnoteReference {
    pub label: String,
    pub normalized_label: String,
    pub range: TextRange,
    pub label_range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadingLink {
    pub anchor: String,
    pub range: TextRange,
    pub anchor_range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Citation {
    pub key: String,
    pub range: TextRange,
    pub key_range: TextRange,
}

#[derive(Debug, Clone, Default)]
pub struct WorkspaceIndex {
    citation_keys: HashSet<String>,
}

impl WorkspaceIndex {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn from_root(root: &Path) -> Self {
        let mut index = Self::default();
        let walker = WalkBuilder::new(root)
            .standard_filters(true)
            .max_filesize(Some(2_000_000))
            .build();

        for entry in walker.flatten() {
            let path = entry.path();
            if !path.is_file() || !is_bibliography_file(path) {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(path) {
                index.add_bibliography_text(&text);
            }
        }

        index
    }

    pub fn add_bibliography_text(&mut self, text: &str) {
        for capture in BIB_KEY_RE.captures_iter(text) {
            if let Some(key) = capture.get(1) {
                self.citation_keys.insert(key.as_str().to_string());
            }
        }
    }

    pub fn has_citation_keys(&self) -> bool {
        !self.citation_keys.is_empty()
    }

    pub fn contains_citation_key(&self, key: &str) -> bool {
        self.citation_keys.contains(key)
    }

    pub fn citation_keys(&self) -> impl Iterator<Item = &str> {
        self.citation_keys.iter().map(String::as_str)
    }
}

#[derive(Debug, Clone, Default)]
pub struct DocumentAnalysis {
    pub headings: Vec<Heading>,
    pub reference_definitions: Vec<ReferenceDefinition>,
    pub footnote_definitions: Vec<FootnoteDefinition>,
    pub reference_links: Vec<ReferenceLink>,
    pub footnote_references: Vec<FootnoteReference>,
    pub heading_links: Vec<HeadingLink>,
    pub citations: Vec<Citation>,
    pub diagnostics: Vec<Diagnostic>,
}

impl DocumentAnalysis {
    pub fn analyze(document: &ParsedDocument, workspace: &WorkspaceIndex) -> Self {
        let mut analysis = scan_document(document.text());
        analysis.add_diagnostics(document, workspace);
        analysis
    }

    pub fn heading_by_anchor(&self, anchor: &str) -> Option<&Heading> {
        self.headings
            .iter()
            .find(|heading| heading.anchor == anchor)
    }

    pub fn reference_definition(&self, label: &str) -> Option<&ReferenceDefinition> {
        let label = normalize_label(label);
        self.reference_definitions
            .iter()
            .find(|definition| definition.normalized_label == label)
    }

    pub fn footnote_definition(&self, label: &str) -> Option<&FootnoteDefinition> {
        let label = normalize_label(label);
        self.footnote_definitions
            .iter()
            .find(|definition| definition.normalized_label == label)
    }

    pub fn symbol_at(&self, offset: usize) -> Option<SymbolAtOffset<'_>> {
        for heading in &self.headings {
            if heading.selection_range.contains(offset) {
                return Some(SymbolAtOffset::Heading(heading));
            }
        }
        for definition in &self.reference_definitions {
            if definition.label_range.contains(offset) {
                return Some(SymbolAtOffset::ReferenceDefinition(definition));
            }
        }
        for definition in &self.footnote_definitions {
            if definition.label_range.contains(offset) {
                return Some(SymbolAtOffset::FootnoteDefinition(definition));
            }
        }
        for link in &self.reference_links {
            if link.label_range.contains(offset) {
                return Some(SymbolAtOffset::ReferenceLink(link));
            }
        }
        for reference in &self.footnote_references {
            if reference.label_range.contains(offset) {
                return Some(SymbolAtOffset::FootnoteReference(reference));
            }
        }
        for link in &self.heading_links {
            if link.anchor_range.contains(offset) {
                return Some(SymbolAtOffset::HeadingLink(link));
            }
        }
        for citation in &self.citations {
            if citation.key_range.contains(offset) {
                return Some(SymbolAtOffset::Citation(citation));
            }
        }
        None
    }

    pub fn reference_ranges_for_label(&self, label: &str) -> Vec<TextRange> {
        let label = normalize_label(label);
        self.reference_definitions
            .iter()
            .filter(|definition| definition.normalized_label == label)
            .map(|definition| definition.label_range)
            .chain(
                self.reference_links
                    .iter()
                    .filter(|link| link.normalized_label == label)
                    .map(|link| link.label_range),
            )
            .collect()
    }

    pub fn footnote_ranges_for_label(&self, label: &str) -> Vec<TextRange> {
        let label = normalize_label(label);
        self.footnote_definitions
            .iter()
            .filter(|definition| definition.normalized_label == label)
            .map(|definition| definition.label_range)
            .chain(
                self.footnote_references
                    .iter()
                    .filter(|reference| reference.normalized_label == label)
                    .map(|reference| reference.label_range),
            )
            .collect()
    }

    pub fn heading_link_ranges_for_anchor(&self, anchor: &str) -> Vec<TextRange> {
        self.headings
            .iter()
            .filter(|heading| heading.anchor == anchor)
            .map(|heading| heading.selection_range)
            .chain(
                self.heading_links
                    .iter()
                    .filter(|link| link.anchor == anchor)
                    .map(|link| link.anchor_range),
            )
            .collect()
    }

    fn add_diagnostics(&mut self, document: &ParsedDocument, workspace: &WorkspaceIndex) {
        for syntax in document.syntax_diagnostics() {
            self.diagnostics.push(Diagnostic {
                range: syntax.range,
                severity: Severity::Error,
                code: "syntax",
                message: syntax.message,
            });
        }

        push_duplicate_diagnostics(
            &mut self.diagnostics,
            self.reference_definitions.iter().map(|definition| {
                (
                    definition.normalized_label.as_str(),
                    definition.label_range,
                    "duplicate-reference",
                    "duplicate reference definition",
                )
            }),
        );

        push_duplicate_diagnostics(
            &mut self.diagnostics,
            self.footnote_definitions.iter().map(|definition| {
                (
                    definition.normalized_label.as_str(),
                    definition.label_range,
                    "duplicate-footnote",
                    "duplicate footnote definition",
                )
            }),
        );

        push_duplicate_diagnostics(
            &mut self.diagnostics,
            self.headings.iter().map(|heading| {
                (
                    heading.anchor.as_str(),
                    heading.selection_range,
                    "duplicate-heading",
                    "duplicate generated heading anchor",
                )
            }),
        );

        let references = self
            .reference_definitions
            .iter()
            .map(|definition| definition.normalized_label.as_str())
            .collect::<HashSet<_>>();
        for link in &self.reference_links {
            if !references.contains(link.normalized_label.as_str()) {
                self.diagnostics.push(Diagnostic {
                    range: link.label_range,
                    severity: Severity::Warning,
                    code: "unresolved-reference",
                    message: format!("unresolved reference label `{}`", link.label),
                });
            }
        }

        let footnotes = self
            .footnote_definitions
            .iter()
            .map(|definition| definition.normalized_label.as_str())
            .collect::<HashSet<_>>();
        for reference in &self.footnote_references {
            if !footnotes.contains(reference.normalized_label.as_str()) {
                self.diagnostics.push(Diagnostic {
                    range: reference.label_range,
                    severity: Severity::Warning,
                    code: "unresolved-footnote",
                    message: format!("unresolved footnote `{}`", reference.label),
                });
            }
        }

        let anchors = self
            .headings
            .iter()
            .map(|heading| heading.anchor.as_str())
            .collect::<HashSet<_>>();
        for link in &self.heading_links {
            if !anchors.contains(link.anchor.as_str()) {
                self.diagnostics.push(Diagnostic {
                    range: link.anchor_range,
                    severity: Severity::Warning,
                    code: "unresolved-heading",
                    message: format!("unresolved heading anchor `#{}`", link.anchor),
                });
            }
        }

        if workspace.has_citation_keys() {
            for citation in &self.citations {
                if !workspace.contains_citation_key(&citation.key) {
                    self.diagnostics.push(Diagnostic {
                        range: citation.key_range,
                        severity: Severity::Warning,
                        code: "unresolved-citation",
                        message: format!("unresolved citation `@{}`", citation.key),
                    });
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum SymbolAtOffset<'a> {
    Heading(&'a Heading),
    ReferenceDefinition(&'a ReferenceDefinition),
    FootnoteDefinition(&'a FootnoteDefinition),
    ReferenceLink(&'a ReferenceLink),
    FootnoteReference(&'a FootnoteReference),
    HeadingLink(&'a HeadingLink),
    Citation(&'a Citation),
}

pub fn slugify_heading(title: &str) -> String {
    let mut slug = String::new();
    let mut previous_was_separator = false;

    for ch in strip_inline_markup(title).chars() {
        if ch.is_alphanumeric() || ch == '_' || ch == '-' || ch == '.' {
            for lower in ch.to_lowercase() {
                slug.push(lower);
            }
            previous_was_separator = false;
        } else if ch.is_whitespace() || ch == '/' {
            if !slug.is_empty() && !previous_was_separator {
                slug.push('-');
                previous_was_separator = true;
            }
        }
    }

    while slug.ends_with('-') {
        slug.pop();
    }

    if slug.is_empty() {
        "section".to_string()
    } else {
        slug
    }
}

pub fn normalize_label(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn scan_document(text: &str) -> DocumentAnalysis {
    let mut analysis = DocumentAnalysis::default();
    let mut byte_offset = 0;
    let mut anchor_counts = HashMap::<String, usize>::new();

    for line in text.split_inclusive('\n') {
        let line_without_newline = line.trim_end_matches(['\r', '\n']);
        scan_block_line(
            line_without_newline,
            byte_offset,
            &mut analysis,
            &mut anchor_counts,
        );
        scan_inline_line(line_without_newline, byte_offset, &mut analysis);
        byte_offset += line.len();
    }

    if text.is_empty() {
        return analysis;
    }
    if !text.ends_with('\n') && byte_offset < text.len() {
        let line = &text[byte_offset..];
        scan_block_line(line, byte_offset, &mut analysis, &mut anchor_counts);
        scan_inline_line(line, byte_offset, &mut analysis);
    }

    analysis
}

fn scan_block_line(
    line: &str,
    byte_offset: usize,
    analysis: &mut DocumentAnalysis,
    anchor_counts: &mut HashMap<String, usize>,
) {
    if let Some(captures) = HEADING_RE.captures(line) {
        let marker = captures.get(1).unwrap();
        let title_match = captures.get(2).unwrap();
        let title = title_match.as_str().trim().trim_end_matches('#').trim();
        let base_anchor = slugify_heading(title);
        let count = anchor_counts.entry(base_anchor.clone()).or_insert(0);
        let anchor = if *count == 0 {
            base_anchor
        } else {
            format!("{base_anchor}-{}", *count)
        };
        *count += 1;

        let selection_start = byte_offset + title_match.start();
        let selection_end = selection_start + title.len();
        analysis.headings.push(Heading {
            level: marker.as_str().len() as u8,
            title: title.to_string(),
            anchor,
            range: TextRange::new(byte_offset, byte_offset + line.len()),
            selection_range: TextRange::new(selection_start, selection_end),
        });
        return;
    }

    if let Some(captures) = FOOTNOTE_DEF_RE.captures(line) {
        let label = captures.get(1).unwrap();
        analysis.footnote_definitions.push(FootnoteDefinition {
            label: label.as_str().to_string(),
            normalized_label: normalize_label(label.as_str()),
            range: TextRange::new(byte_offset, byte_offset + line.len()),
            label_range: TextRange::new(byte_offset + label.start(), byte_offset + label.end()),
        });
        return;
    }

    if let Some(captures) = REF_DEF_RE.captures(line) {
        let label = captures.get(1).unwrap();
        let target = captures.get(2).unwrap();
        analysis.reference_definitions.push(ReferenceDefinition {
            label: label.as_str().to_string(),
            normalized_label: normalize_label(label.as_str()),
            target: target.as_str().to_string(),
            range: TextRange::new(byte_offset, byte_offset + line.len()),
            label_range: TextRange::new(byte_offset + label.start(), byte_offset + label.end()),
        });
    }
}

fn scan_inline_line(line: &str, byte_offset: usize, analysis: &mut DocumentAnalysis) {
    let is_footnote_definition = FOOTNOTE_DEF_RE.is_match(line);
    let is_reference_definition = REF_DEF_RE.is_match(line);

    if !is_reference_definition {
        for captures in FULL_REF_RE.captures_iter(line) {
            let whole = captures.get(0).unwrap();
            let label = captures.get(2).unwrap();
            if label.as_str().starts_with('^') {
                continue;
            }
            analysis.reference_links.push(ReferenceLink {
                label: label.as_str().to_string(),
                normalized_label: normalize_label(label.as_str()),
                range: TextRange::new(byte_offset + whole.start(), byte_offset + whole.end()),
                label_range: TextRange::new(byte_offset + label.start(), byte_offset + label.end()),
            });
        }

        for captures in COLLAPSED_REF_RE.captures_iter(line) {
            let whole = captures.get(0).unwrap();
            let label = captures.get(1).unwrap();
            analysis.reference_links.push(ReferenceLink {
                label: label.as_str().to_string(),
                normalized_label: normalize_label(label.as_str()),
                range: TextRange::new(byte_offset + whole.start(), byte_offset + whole.end()),
                label_range: TextRange::new(byte_offset + label.start(), byte_offset + label.end()),
            });
        }
    }

    if !is_footnote_definition {
        for captures in FOOTNOTE_REF_RE.captures_iter(line) {
            let whole = captures.get(0).unwrap();
            let label = captures.get(1).unwrap();
            analysis.footnote_references.push(FootnoteReference {
                label: label.as_str().to_string(),
                normalized_label: normalize_label(label.as_str()),
                range: TextRange::new(byte_offset + whole.start(), byte_offset + whole.end()),
                label_range: TextRange::new(byte_offset + label.start(), byte_offset + label.end()),
            });
        }
    }

    for captures in HEADING_LINK_RE.captures_iter(line) {
        let whole = captures.get(0).unwrap();
        let anchor = captures.get(1).unwrap();
        analysis.heading_links.push(HeadingLink {
            anchor: anchor.as_str().to_string(),
            range: TextRange::new(byte_offset + whole.start(), byte_offset + whole.end()),
            anchor_range: TextRange::new(byte_offset + anchor.start(), byte_offset + anchor.end()),
        });
    }

    for captures in CITATION_RE.captures_iter(line) {
        let sigil = captures.get(2).unwrap();
        let key = captures.get(3).unwrap();
        analysis.citations.push(Citation {
            key: key.as_str().to_string(),
            range: TextRange::new(byte_offset + sigil.start(), byte_offset + key.end()),
            key_range: TextRange::new(byte_offset + key.start(), byte_offset + key.end()),
        });
    }
}

fn push_duplicate_diagnostics<'a>(
    diagnostics: &mut Vec<Diagnostic>,
    items: impl Iterator<Item = (&'a str, TextRange, &'static str, &'static str)>,
) {
    let mut seen = HashSet::new();
    for (key, range, code, message) in items {
        if !seen.insert(key.to_string()) {
            diagnostics.push(Diagnostic {
                range,
                severity: Severity::Warning,
                code,
                message: message.to_string(),
            });
        }
    }
}

fn strip_inline_markup(title: &str) -> String {
    title
        .replace(['`', '*', '_'], "")
        .replace('[', "")
        .replace(']', "")
        .replace('(', "")
        .replace(')', "")
}

fn is_bibliography_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("bib") | Some("bibtex")
    )
}

#[cfg(test)]
mod tests {
    use pandocmd_syntax::PandocMarkdownParser;

    use super::*;

    #[test]
    fn extracts_writing_symbols_and_diagnostics() {
        let text = "# Intro\n\nSee [the docs][docs] and [missing][nope].\n\n[^a]\n\n[docs]: https://example.com\n[^a]: Footnote\n";
        let mut parser = PandocMarkdownParser::new().unwrap();
        let document = parser.parse(text).unwrap();
        let analysis = DocumentAnalysis::analyze(&document, &WorkspaceIndex::empty());

        assert_eq!(analysis.headings[0].anchor, "intro");
        assert_eq!(analysis.reference_definitions[0].label, "docs");
        assert_eq!(analysis.footnote_definitions[0].label, "a");
        assert!(analysis
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "unresolved-reference"));
    }

    #[test]
    fn reads_bibliography_keys() {
        let mut workspace = WorkspaceIndex::empty();
        workspace.add_bibliography_text("@article{doe2024,\n title = {T}\n}");
        assert!(workspace.contains_citation_key("doe2024"));
    }

    #[test]
    fn slugifies_headings() {
        assert_eq!(
            slugify_heading("Hello, Pandoc Markdown!"),
            "hello-pandoc-markdown"
        );
        assert_eq!(slugify_heading("`Code` & Math"), "code-math");
    }
}
