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
static FENCED_DIV_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[ \t]{0,3}(:{3,})(.*)$").unwrap());
static BRACED_ATTR_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{([^}\n]*)\}").unwrap());

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DivAttribute {
    pub key: String,
    pub value: Option<String>,
    pub range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FencedDiv {
    pub id: Option<String>,
    pub classes: Vec<String>,
    pub attributes: Vec<DivAttribute>,
    pub fence_len: usize,
    pub range: TextRange,
    pub opening_range: TextRange,
    pub closing_range: Option<TextRange>,
    pub selection_range: TextRange,
    pub id_range: Option<TextRange>,
}

impl FencedDiv {
    pub fn label(&self) -> String {
        if let Some(id) = &self.id {
            format!("#{id}")
        } else if self.classes.is_empty() {
            "div".to_string()
        } else {
            format!(".{}", self.classes.join("."))
        }
    }

    pub fn detail(&self) -> String {
        let mut parts = Vec::new();
        if let Some(id) = &self.id {
            parts.push(format!("#{id}"));
        }
        parts.extend(self.classes.iter().map(|class| format!(".{class}")));
        parts.extend(self.attributes.iter().map(|attribute| {
            if let Some(value) = &attribute.value {
                format!("{}={value}", attribute.key)
            } else {
                attribute.key.clone()
            }
        }));

        if parts.is_empty() {
            "fenced div".to_string()
        } else {
            parts.join(" ")
        }
    }
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
    pub fenced_divs: Vec<FencedDiv>,
    pub diagnostics: Vec<Diagnostic>,
}

impl DocumentAnalysis {
    pub fn analyze(document: &ParsedDocument, workspace: &WorkspaceIndex) -> Self {
        let mut analysis = scan_document(document.text());
        analysis.add_diagnostics(document, workspace);
        analysis
    }

    pub fn local_reference_ids(&self, text: &str) -> HashSet<String> {
        local_reference_ids(self, text)
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

    pub fn div_by_id(&self, id: &str) -> Option<&FencedDiv> {
        self.fenced_divs
            .iter()
            .find(|div| div.id.as_deref() == Some(id))
    }

    pub fn anchor_target_range(&self, anchor: &str) -> Option<TextRange> {
        self.heading_by_anchor(anchor)
            .map(|heading| heading.selection_range)
            .or_else(|| {
                self.div_by_id(anchor)
                    .map(|div| div.id_range.unwrap_or(div.selection_range))
            })
    }

    pub fn symbol_at(&self, offset: usize) -> Option<SymbolAtOffset<'_>> {
        for heading in &self.headings {
            if heading.selection_range.contains(offset) {
                return Some(SymbolAtOffset::Heading(heading));
            }
        }
        for div in &self.fenced_divs {
            if div.selection_range.contains(offset) || div.opening_range.contains(offset) {
                return Some(SymbolAtOffset::FencedDiv(div));
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
                self.fenced_divs
                    .iter()
                    .filter(|div| div.id.as_deref() == Some(anchor))
                    .map(|div| div.id_range.unwrap_or(div.selection_range)),
            )
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

        let duplicate_anchor_diagnostics = duplicate_anchor_diagnostics(self);
        self.diagnostics.extend(duplicate_anchor_diagnostics);

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
            .chain(self.fenced_divs.iter().filter_map(|div| div.id.as_deref()))
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

        let local_refs = self.local_reference_ids(document.text());
        if workspace.has_citation_keys() {
            for citation in &self.citations {
                if !workspace.contains_citation_key(&citation.key)
                    && !local_refs.contains(&citation.key)
                {
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
    FencedDiv(&'a FencedDiv),
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
    let mut div_stack = Vec::<OpenDiv>::new();

    for line in text.split_inclusive('\n') {
        let line_without_newline = line.trim_end_matches(['\r', '\n']);
        scan_fenced_div_line(
            line_without_newline,
            byte_offset,
            &mut analysis,
            &mut div_stack,
        );
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
        scan_fenced_div_line(line, byte_offset, &mut analysis, &mut div_stack);
        scan_block_line(line, byte_offset, &mut analysis, &mut anchor_counts);
        scan_inline_line(line, byte_offset, &mut analysis);
    }

    finish_unclosed_fenced_divs(&mut analysis, div_stack, text.len());

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

#[derive(Debug)]
struct OpenDiv {
    index: usize,
    fence_len: usize,
    opening_range: TextRange,
}

#[derive(Debug)]
struct ParsedDivAttributes {
    id: Option<String>,
    id_range: Option<TextRange>,
    classes: Vec<String>,
    class_ranges: Vec<TextRange>,
    attributes: Vec<DivAttribute>,
    selection_range: TextRange,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Debug)]
struct AttrToken<'a> {
    text: &'a str,
    range: TextRange,
}

fn scan_fenced_div_line(
    line: &str,
    byte_offset: usize,
    analysis: &mut DocumentAnalysis,
    div_stack: &mut Vec<OpenDiv>,
) {
    let Some(captures) = FENCED_DIV_RE.captures(line) else {
        return;
    };

    let fence = captures.get(1).unwrap();
    let rest = captures.get(2).unwrap();
    let fence_len = fence.as_str().len();
    let line_range = TextRange::new(byte_offset, byte_offset + line.len());

    if rest.as_str().trim().is_empty() {
        close_fenced_div(fence_len, line_range, analysis, div_stack);
        return;
    }

    let parsed = parse_div_attributes(rest.as_str(), byte_offset + rest.start(), line_range);
    analysis.diagnostics.extend(parsed.diagnostics);
    let selection_range = parsed
        .id_range
        .or_else(|| parsed.class_ranges.first().copied())
        .unwrap_or(parsed.selection_range);

    let index = analysis.fenced_divs.len();
    analysis.fenced_divs.push(FencedDiv {
        id: parsed.id,
        classes: parsed.classes,
        attributes: parsed.attributes,
        fence_len,
        range: line_range,
        opening_range: line_range,
        closing_range: None,
        selection_range,
        id_range: parsed.id_range,
    });
    div_stack.push(OpenDiv {
        index,
        fence_len,
        opening_range: line_range,
    });
}

fn close_fenced_div(
    fence_len: usize,
    closing_range: TextRange,
    analysis: &mut DocumentAnalysis,
    div_stack: &mut Vec<OpenDiv>,
) {
    let Some(open) = div_stack.last() else {
        analysis.diagnostics.push(Diagnostic {
            range: closing_range,
            severity: Severity::Warning,
            code: "unmatched-fenced-div-close",
            message: "fenced div closing fence has no matching opening fence".to_string(),
        });
        return;
    };

    if fence_len < open.fence_len {
        analysis.diagnostics.push(Diagnostic {
            range: closing_range,
            severity: Severity::Warning,
            code: "short-fenced-div-close",
            message: format!(
                "fenced div closing fence needs at least {} colons",
                open.fence_len
            ),
        });
        return;
    }

    let open = div_stack.pop().unwrap();
    if let Some(div) = analysis.fenced_divs.get_mut(open.index) {
        div.range = TextRange::new(open.opening_range.start, closing_range.end);
        div.closing_range = Some(closing_range);
    }
}

fn finish_unclosed_fenced_divs(
    analysis: &mut DocumentAnalysis,
    div_stack: Vec<OpenDiv>,
    document_len: usize,
) {
    for open in div_stack.into_iter().rev() {
        if let Some(div) = analysis.fenced_divs.get_mut(open.index) {
            div.range = TextRange::new(open.opening_range.start, document_len);
        }
        analysis.diagnostics.push(Diagnostic {
            range: open.opening_range,
            severity: Severity::Warning,
            code: "unclosed-fenced-div",
            message: "fenced div has no closing fence".to_string(),
        });
    }
}

fn parse_div_attributes(
    rest: &str,
    rest_offset: usize,
    fallback_range: TextRange,
) -> ParsedDivAttributes {
    let (attr_text, attr_offset, attr_range) = trim_div_attribute_text(rest, rest_offset);
    let mut parsed = ParsedDivAttributes {
        id: None,
        id_range: None,
        classes: Vec::new(),
        class_ranges: Vec::new(),
        attributes: Vec::new(),
        selection_range: attr_range,
        diagnostics: Vec::new(),
    };

    if attr_text.is_empty() {
        parsed.selection_range = fallback_range;
        parsed.diagnostics.push(Diagnostic {
            range: fallback_range,
            severity: Severity::Warning,
            code: "missing-fenced-div-attributes",
            message: "fenced div opening fence should include attributes or a class name"
                .to_string(),
        });
        return parsed;
    }

    if attr_text.starts_with('{') {
        if !attr_text.ends_with('}') {
            parsed.diagnostics.push(Diagnostic {
                range: attr_range,
                severity: Severity::Warning,
                code: "malformed-fenced-div-attributes",
                message: "fenced div attributes should be enclosed with `{` and `}`".to_string(),
            });
        }

        let inner_start = usize::from(attr_text.starts_with('{'));
        let inner_end = if attr_text.ends_with('}') {
            attr_text.len().saturating_sub(1)
        } else {
            attr_text.len()
        };
        let inner = &attr_text[inner_start..inner_end];
        parse_braced_div_attributes(inner, attr_offset + inner_start, &mut parsed);
    } else {
        parse_unbraced_div_attributes(attr_text, attr_range, &mut parsed);
    }

    parsed
}

fn trim_div_attribute_text(rest: &str, rest_offset: usize) -> (&str, usize, TextRange) {
    let leading = rest.len() - rest.trim_start().len();
    let mut end = rest.trim_end().len();
    let mut attr_text = &rest[leading..end];

    if let Some(trailing_start) = trailing_colon_fence_start(attr_text) {
        end = leading + attr_text[..trailing_start].trim_end().len();
        attr_text = &rest[leading..end];
    }

    let attr_offset = rest_offset + leading;
    (
        attr_text,
        attr_offset,
        TextRange::new(attr_offset, attr_offset + attr_text.len()),
    )
}

fn trailing_colon_fence_start(text: &str) -> Option<usize> {
    let trimmed = text.trim_end();
    let token_start = trimmed.rfind(char::is_whitespace).map(|index| index + 1)?;
    let token = &trimmed[token_start..];
    (token.len() >= 3 && token.chars().all(|ch| ch == ':')).then_some(token_start)
}

fn parse_braced_div_attributes(inner: &str, inner_offset: usize, parsed: &mut ParsedDivAttributes) {
    for token in tokenize_attributes(inner, inner_offset) {
        if let Some(id) = token.text.strip_prefix('#') {
            let id_range = TextRange::new(token.range.start + 1, token.range.end);
            if id.is_empty() {
                parsed.diagnostics.push(Diagnostic {
                    range: token.range,
                    severity: Severity::Warning,
                    code: "empty-fenced-div-id",
                    message: "fenced div id cannot be empty".to_string(),
                });
            } else {
                parsed.id = Some(id.to_string());
                parsed.id_range = Some(id_range);
            }
        } else if let Some(class) = token.text.strip_prefix('.') {
            let class_range = TextRange::new(token.range.start + 1, token.range.end);
            if class.is_empty() {
                parsed.diagnostics.push(Diagnostic {
                    range: token.range,
                    severity: Severity::Warning,
                    code: "empty-fenced-div-class",
                    message: "fenced div class cannot be empty".to_string(),
                });
            } else {
                parsed.classes.push(class.to_string());
                parsed.class_ranges.push(class_range);
            }
        } else if let Some((key, value)) = token.text.split_once('=') {
            let value = value.trim_matches(['"', '\'']);
            if key.is_empty() {
                parsed.diagnostics.push(Diagnostic {
                    range: token.range,
                    severity: Severity::Warning,
                    code: "empty-fenced-div-attribute",
                    message: "fenced div attribute key cannot be empty".to_string(),
                });
            } else {
                parsed.attributes.push(DivAttribute {
                    key: key.to_string(),
                    value: Some(value.to_string()),
                    range: token.range,
                });
            }
        } else {
            parsed.classes.push(token.text.to_string());
            parsed.class_ranges.push(token.range);
        }
    }
}

fn parse_unbraced_div_attributes(
    attr_text: &str,
    attr_range: TextRange,
    parsed: &mut ParsedDivAttributes,
) {
    let mut parts = attr_text.split_whitespace();
    let Some(class) = parts.next() else {
        return;
    };

    parsed.classes.push(class.to_string());
    parsed.class_ranges.push(TextRange::new(
        attr_range.start,
        attr_range.start + class.len(),
    ));

    if parts.next().is_some() {
        parsed.diagnostics.push(Diagnostic {
            range: attr_range,
            severity: Severity::Warning,
            code: "malformed-fenced-div-attributes",
            message: "unbraced fenced div attributes should be a single class name".to_string(),
        });
    }
}

fn tokenize_attributes(input: &str, offset: usize) -> Vec<AttrToken<'_>> {
    let mut tokens = Vec::new();
    let mut token_start = None;
    let mut quote = None;

    for (index, ch) in input.char_indices() {
        if token_start.is_none() {
            if ch.is_whitespace() {
                continue;
            }
            token_start = Some(index);
        }

        if let Some(active_quote) = quote {
            if ch == active_quote {
                quote = None;
            }
            continue;
        }

        if ch == '"' || ch == '\'' {
            quote = Some(ch);
        } else if ch.is_whitespace() {
            let start = token_start.take().unwrap();
            if start < index {
                tokens.push(AttrToken {
                    text: &input[start..index],
                    range: TextRange::new(offset + start, offset + index),
                });
            }
        }
    }

    if let Some(start) = token_start {
        tokens.push(AttrToken {
            text: &input[start..],
            range: TextRange::new(offset + start, offset + input.len()),
        });
    }

    tokens
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

fn duplicate_anchor_diagnostics(analysis: &DocumentAnalysis) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut seen = HashSet::<String>::new();
    for (anchor, range) in analysis
        .headings
        .iter()
        .map(|heading| (heading.anchor.as_str(), heading.selection_range))
        .chain(analysis.fenced_divs.iter().filter_map(|div| {
            div.id
                .as_deref()
                .map(|id| (id, div.id_range.unwrap_or(div.selection_range)))
        }))
    {
        if !seen.insert(anchor.to_string()) {
            diagnostics.push(Diagnostic {
                range,
                severity: Severity::Warning,
                code: "duplicate-anchor",
                message: format!("duplicate document anchor `#{anchor}`"),
            });
        }
    }
    diagnostics
}

fn local_reference_ids(analysis: &DocumentAnalysis, text: &str) -> HashSet<String> {
    let mut ids = analysis
        .headings
        .iter()
        .map(|heading| heading.anchor.as_str())
        .chain(
            analysis
                .fenced_divs
                .iter()
                .filter_map(|div| div.id.as_deref()),
        )
        .map(str::to_string)
        .collect::<HashSet<_>>();

    for id in braced_attribute_ids(text) {
        ids.insert(id);
    }

    ids
}

fn braced_attribute_ids(text: &str) -> HashSet<String> {
    let mut ids = HashSet::new();
    for captures in BRACED_ATTR_RE.captures_iter(text) {
        let inner = captures.get(1).unwrap();
        for token in tokenize_attributes(inner.as_str(), inner.start()) {
            if let Some(id) = token.text.strip_prefix('#') {
                if !id.is_empty() {
                    ids.insert(id.to_string());
                }
            }
        }
    }
    ids
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
    fn extracts_fenced_divs_and_resolves_div_anchors() {
        let text = "# Intro\n\n::: {#panel .note key=\"two words\"}\nContent.\n:::\n\n::: warning\nBody.\n:::\n\nSee [panel](#panel) and [intro](#intro).\n";
        let mut parser = PandocMarkdownParser::new().unwrap();
        let document = parser.parse(text).unwrap();
        let analysis = DocumentAnalysis::analyze(&document, &WorkspaceIndex::empty());

        assert_eq!(analysis.fenced_divs.len(), 2);
        assert_eq!(analysis.fenced_divs[0].id.as_deref(), Some("panel"));
        assert_eq!(analysis.fenced_divs[0].classes, vec!["note"]);
        assert_eq!(analysis.fenced_divs[0].attributes[0].key, "key");
        assert_eq!(
            analysis.fenced_divs[0].attributes[0].value.as_deref(),
            Some("two words")
        );
        assert_eq!(analysis.fenced_divs[1].classes, vec!["warning"]);
        assert!(analysis
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "unresolved-heading"));
    }

    #[test]
    fn diagnoses_fenced_div_structure() {
        let text = "# Panel\n\n::: {#panel}\ncontent\n\n:::\n";
        let mut parser = PandocMarkdownParser::new().unwrap();
        let document = parser.parse(text).unwrap();
        let analysis = DocumentAnalysis::analyze(&document, &WorkspaceIndex::empty());

        assert!(analysis
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "duplicate-anchor"));

        let document = parser.parse(":::\n").unwrap();
        let analysis = DocumentAnalysis::analyze(&document, &WorkspaceIndex::empty());
        assert!(analysis
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "unmatched-fenced-div-close"));

        let document = parser.parse(":::: {.note}\n:::\n").unwrap();
        let analysis = DocumentAnalysis::analyze(&document, &WorkspaceIndex::empty());
        assert!(analysis
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "short-fenced-div-close"));
        assert!(analysis
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "unclosed-fenced-div"));
    }

    #[test]
    fn reads_bibliography_keys() {
        let mut workspace = WorkspaceIndex::empty();
        workspace.add_bibliography_text("@article{doe2024,\n title = {T}\n}");
        assert!(workspace.contains_citation_key("doe2024"));
    }

    #[test]
    fn treats_pandoc_at_references_to_local_labels_as_resolved() {
        let text = "# Intro {#sec-intro}\n\n::: {#panel .note}\nContent.\n:::\n\n```{#lst-demo .rust}\nfn main() {}\n```\n\n![Plot](plot.png){#fig-plot}\n\n[Term]{#span-term}\n\nSee [@sec-intro], [@panel], [@lst-demo], [@fig-plot], [@span-term], and [@missing].\n";
        let mut workspace = WorkspaceIndex::empty();
        workspace.add_bibliography_text("@article{doe2024,\n title = {T}\n}");
        let mut parser = PandocMarkdownParser::new().unwrap();
        let document = parser.parse(text).unwrap();
        let analysis = DocumentAnalysis::analyze(&document, &workspace);

        assert!(analysis
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "unresolved-citation")
            .all(|diagnostic| !diagnostic.message.contains("@sec-intro")
                && !diagnostic.message.contains("@panel")
                && !diagnostic.message.contains("@lst-demo")
                && !diagnostic.message.contains("@fig-plot")
                && !diagnostic.message.contains("@span-term")));
        assert!(analysis
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message == "unresolved citation `@missing`"));
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
