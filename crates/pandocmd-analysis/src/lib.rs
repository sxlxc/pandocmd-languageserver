//! Semantic analysis for Pandoc Markdown documents.
//!
//! [`DocumentAnalysis::analyze`] turns a parsed document into:
//!
//! * structural symbols (headings, fenced divs),
//! * cross-referenceables (reference definitions/links, footnote
//!   definitions/references, citations, local anchors),
//! * links and semantic tokens,
//! * diagnostics (duplicates, unresolved references, and `extension-disabled`
//!   hints when a construct is used while its Pandoc extension is off).
//!
//! All recognition is gated by the configured [`ExtensionSet`], mirroring
//! the Pandoc User's Guide (<https://pandoc.org/MANUAL.html#pandocs-markdown>).

use std::collections::HashSet;

use pandocmd_extensions::{Extension, ExtensionSet, Flavor};
use pandocmd_syntax::ParsedDocument;

pub mod bibliography;
pub mod identifiers;
mod scanner;

pub use bibliography::{BibliographyEntry, BibliographySource, WorkspaceIndex};
pub use identifiers::{
    fold_to_ascii, slugify, IdentifierAlgorithm, IdentifierOptions, EMPTY_IDENTIFIER_FALLBACK,
};

/// Options controlling analysis behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyzeOptions {
    /// Which Pandoc extensions are enabled. Defaults to full `markdown`.
    pub extensions: ExtensionSet,
    /// Emit diagnostics for unresolved references/footnotes/anchors.
    pub unresolved_references: bool,
    /// Emit `extension-disabled` diagnostics when disabled constructs are used.
    pub disabled_extensions: bool,
}

impl Default for AnalyzeOptions {
    fn default() -> Self {
        AnalyzeOptions {
            extensions: ExtensionSet::flavor_defaults(Flavor::Markdown),
            unresolved_references: true,
            disabled_extensions: true,
        }
    }
}

impl AnalyzeOptions {
    /// Options for a raw extension set with all diagnostic categories on.
    pub fn with_extensions(extensions: ExtensionSet) -> Self {
        AnalyzeOptions {
            extensions,
            ..AnalyzeOptions::default()
        }
    }
}

/// Diagnostic severities, mirroring LSP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Information,
    Hint,
}

/// A diagnostic produced by analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub range: pandocmd_syntax::TextRange,
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
    /// The Pandoc extension involved, for `extension-disabled` diagnostics.
    pub extension: Option<&'static str>,
}

/// How a heading's identifier was determined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentifierSource {
    /// Explicit `{#id}` via the `header_attributes` extension.
    Explicit,
    /// Derived automatically via `auto_identifiers`.
    Auto,
    /// Derived automatically via `gfm_auto_identifiers`.
    Gfm,
    /// No identifier (all identifier extensions disabled).
    None,
}

/// An ATX heading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    pub level: u8,
    pub title: String,
    /// The heading identifier, or `None` when identifier extensions are off.
    pub anchor: Option<String>,
    pub identifier_source: IdentifierSource,
    pub range: pandocmd_syntax::TextRange,
    pub selection_range: pandocmd_syntax::TextRange,
    pub id_range: Option<pandocmd_syntax::TextRange>,
}

/// A `[label]: target` definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceDefinition {
    pub label: String,
    pub normalized_label: String,
    pub target: String,
    pub range: pandocmd_syntax::TextRange,
    pub label_range: pandocmd_syntax::TextRange,
}

/// A `[^label]: text` definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FootnoteDefinition {
    pub label: String,
    pub normalized_label: String,
    pub range: pandocmd_syntax::TextRange,
    pub label_range: pandocmd_syntax::TextRange,
}

/// A `[text][label]` or collapsed/shortcut reference link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceLink {
    pub label: String,
    pub normalized_label: String,
    pub range: pandocmd_syntax::TextRange,
    pub label_range: pandocmd_syntax::TextRange,
}

/// A `[^label]` reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FootnoteReference {
    pub label: String,
    pub normalized_label: String,
    pub range: pandocmd_syntax::TextRange,
    pub label_range: pandocmd_syntax::TextRange,
}

/// An inline footnote `^[note text]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineNote {
    pub range: pandocmd_syntax::TextRange,
    pub content: String,
}

/// A `](#anchor)` link to a document anchor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadingLink {
    pub anchor: String,
    pub range: pandocmd_syntax::TextRange,
    pub anchor_range: pandocmd_syntax::TextRange,
}

/// A citation key usage (`[@key]`, `-@key`, or in-text `@key`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Citation {
    pub key: String,
    pub range: pandocmd_syntax::TextRange,
    pub key_range: pandocmd_syntax::TextRange,
}

/// A key=value attribute on a fenced div.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DivAttribute {
    pub key: String,
    pub value: Option<String>,
    pub range: pandocmd_syntax::TextRange,
}

/// A fenced div (`::: {.class #id ...}` ... `:::`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FencedDiv {
    pub id: Option<String>,
    pub classes: Vec<String>,
    pub attributes: Vec<DivAttribute>,
    pub caption: Option<String>,
    pub fence_len: usize,
    pub range: pandocmd_syntax::TextRange,
    pub opening_range: pandocmd_syntax::TextRange,
    pub closing_range: Option<pandocmd_syntax::TextRange>,
    pub selection_range: pandocmd_syntax::TextRange,
    pub id_range: Option<pandocmd_syntax::TextRange>,
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

    pub fn title(&self) -> Option<&str> {
        self.attributes
            .iter()
            .find(|attribute| attribute.key.eq_ignore_ascii_case("title"))
            .and_then(|attribute| attribute.value.as_deref())
            .or(self.caption.as_deref())
            .map(str::trim)
            .filter(|title| !title.is_empty())
    }
}

/// A local cross-referenceable anchor (`#sec-intro`, `#tbl:x`, ...).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalReference {
    pub id: String,
    pub detail: String,
    pub range: pandocmd_syntax::TextRange,
    pub id_range: pandocmd_syntax::TextRange,
}

/// The kind of a Markdown link found by [`DocumentAnalysis::links`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    /// `[text](url)`
    Inline,
    /// `![alt](url)`
    Image,
    /// `<scheme://...>`
    Autolink,
    /// `[label]: url`
    Definition,
}

/// A link (or image destination) in the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownLink {
    pub kind: LinkKind,
    pub target: String,
    pub label: Option<String>,
    /// Whole construct range.
    pub range: pandocmd_syntax::TextRange,
    /// Range of the destination URL itself.
    pub target_range: pandocmd_syntax::TextRange,
}

/// Kinds of semantic tokens emitted by analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticTokenKind {
    Heading,
    FencedDiv,
    CodeFence,
    Citation,
    Footnote,
    Math,
    Link,
}

impl SemanticTokenKind {
    /// Stable name used in the LSP semantic-tokens legend.
    pub const fn name(self) -> &'static str {
        match self {
            SemanticTokenKind::Heading => "heading",
            SemanticTokenKind::FencedDiv => "fencedDiv",
            SemanticTokenKind::CodeFence => "codeFence",
            SemanticTokenKind::Citation => "citation",
            SemanticTokenKind::Footnote => "footnote",
            SemanticTokenKind::Math => "math",
            SemanticTokenKind::Link => "link",
        }
    }
}

/// A single semantic token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticToken {
    pub kind: SemanticTokenKind,
    pub range: pandocmd_syntax::TextRange,
}

/// The full analysis result for one document.
#[derive(Debug, Clone, Default)]
pub struct DocumentAnalysis {
    pub headings: Vec<Heading>,
    pub reference_definitions: Vec<ReferenceDefinition>,
    pub footnote_definitions: Vec<FootnoteDefinition>,
    pub reference_links: Vec<ReferenceLink>,
    pub footnote_references: Vec<FootnoteReference>,
    pub inline_notes: Vec<InlineNote>,
    pub heading_links: Vec<HeadingLink>,
    pub citations: Vec<Citation>,
    pub fenced_divs: Vec<FencedDiv>,
    pub local_references: Vec<LocalReference>,
    pub links: Vec<MarkdownLink>,
    pub semantic_tokens: Vec<SemanticToken>,
    pub diagnostics: Vec<Diagnostic>,
    /// The extension set the analysis ran with.
    pub extensions: ExtensionSet,
}

impl DocumentAnalysis {
    /// Analyze a document against a workspace index.
    pub fn analyze(
        document: &ParsedDocument,
        workspace: &WorkspaceIndex,
        options: &AnalyzeOptions,
    ) -> Self {
        let mut analysis = scanner::scan_document(document.text(), options).analysis;
        analysis.extensions = options.extensions;
        analysis.add_cross_reference_diagnostics(document, workspace, options);
        analysis
    }

    /// All local reference ids (heading anchors, div ids, span ids, ...).
    pub fn local_reference_ids(&self) -> HashSet<String> {
        self.local_references
            .iter()
            .map(|reference| reference.id.clone())
            .collect()
    }

    pub fn local_references_in_order(&self) -> &[LocalReference] {
        &self.local_references
    }

    pub fn heading_by_anchor(&self, anchor: &str) -> Option<&Heading> {
        self.headings
            .iter()
            .find(|heading| heading.anchor.as_deref() == Some(anchor))
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

    pub fn local_reference(&self, id: &str) -> Option<&LocalReference> {
        self.local_references
            .iter()
            .find(|reference| reference.id == id)
    }

    pub fn anchor_target_range(&self, anchor: &str) -> Option<pandocmd_syntax::TextRange> {
        self.local_reference(anchor)
            .map(|reference| reference.id_range)
            .or_else(|| {
                self.heading_by_anchor(anchor)
                    .map(|heading| heading.id_range.unwrap_or(heading.selection_range))
            })
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
            if div.selection_range.contains(offset)
                || div.opening_range.contains(offset)
                || div
                    .closing_range
                    .is_some_and(|closing_range| closing_range.contains(offset))
            {
                return Some(SymbolAtOffset::FencedDiv(div));
            }
        }
        for reference in &self.local_references {
            if reference.id_range.contains(offset) {
                return Some(SymbolAtOffset::LocalReference(reference));
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
        for note in &self.inline_notes {
            if note.range.contains(offset) {
                return Some(SymbolAtOffset::InlineNote(note));
            }
        }
        for link in &self.heading_links {
            if link.anchor_range.contains(offset) {
                return Some(SymbolAtOffset::HeadingLink(link));
            }
        }
        for citation in &self.citations {
            if citation.range.contains(offset) {
                return Some(SymbolAtOffset::Citation(citation));
            }
        }
        None
    }

    pub fn reference_ranges_for_label(&self, label: &str) -> Vec<pandocmd_syntax::TextRange> {
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

    pub fn footnote_ranges_for_label(&self, label: &str) -> Vec<pandocmd_syntax::TextRange> {
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

    pub fn heading_link_ranges_for_anchor(&self, anchor: &str) -> Vec<pandocmd_syntax::TextRange> {
        self.local_reference_ranges_for_id(anchor)
    }

    /// Every range that names the given local id: the defining `{#id}` /
    /// heading anchor plus all `[#id]` / `[@id]` references.
    pub fn local_reference_ranges_for_id(&self, id: &str) -> Vec<pandocmd_syntax::TextRange> {
        let mut ranges = Vec::new();
        for heading in self
            .headings
            .iter()
            .filter(|heading| heading.anchor.as_deref() == Some(id))
        {
            push_unique_range(
                &mut ranges,
                heading.id_range.unwrap_or(heading.selection_range),
            );
        }
        for div in self
            .fenced_divs
            .iter()
            .filter(|div| div.id.as_deref() == Some(id))
        {
            push_unique_range(&mut ranges, div.id_range.unwrap_or(div.selection_range));
        }
        for reference in self
            .local_references
            .iter()
            .filter(|reference| reference.id == id)
        {
            push_unique_range(&mut ranges, reference.id_range);
        }
        for link in self.heading_links.iter().filter(|link| link.anchor == id) {
            push_unique_range(&mut ranges, link.anchor_range);
        }
        for citation in self.citations.iter().filter(|citation| citation.key == id) {
            push_unique_range(&mut ranges, citation.key_range);
        }

        ranges
    }

    fn add_cross_reference_diagnostics(
        &mut self,
        document: &ParsedDocument,
        workspace: &WorkspaceIndex,
        options: &AnalyzeOptions,
    ) {
        for syntax in document.syntax_diagnostics() {
            self.diagnostics.push(Diagnostic {
                range: syntax.range,
                severity: Severity::Error,
                code: "syntax",
                message: syntax.message,
                extension: None,
            });
        }

        let duplicate_items: Vec<(
            String,
            pandocmd_syntax::TextRange,
            &'static str,
            &'static str,
        )> = self
            .reference_definitions
            .iter()
            .map(|definition| {
                (
                    definition.normalized_label.clone(),
                    definition.label_range,
                    "duplicate-reference",
                    "duplicate reference definition",
                )
            })
            .chain(self.footnote_definitions.iter().map(|definition| {
                (
                    definition.normalized_label.clone(),
                    definition.label_range,
                    "duplicate-footnote",
                    "duplicate footnote definition",
                )
            }))
            .chain(self.headings.iter().filter_map(|heading| {
                heading.anchor.clone().map(|anchor| {
                    (
                        anchor,
                        heading.selection_range,
                        "duplicate-heading",
                        "duplicate heading identifier",
                    )
                })
            }))
            .collect();
        self.diagnostics
            .extend(duplicate_diagnostics(duplicate_items));

        let duplicate_anchor_diagnostics = self.duplicate_anchor_diagnostics();
        self.diagnostics.extend(duplicate_anchor_diagnostics);

        if !options.unresolved_references {
            return;
        }

        let references = self
            .reference_definitions
            .iter()
            .map(|definition| definition.normalized_label.as_str())
            .collect::<HashSet<_>>();
        let implicit_headers = if options
            .extensions
            .contains(Extension::ImplicitHeaderReferences)
        {
            self.headings
                .iter()
                .map(|heading| normalize_label(&heading.title))
                .collect::<HashSet<_>>()
        } else {
            HashSet::new()
        };
        for link in &self.reference_links {
            if !references.contains(link.normalized_label.as_str())
                && !implicit_headers.contains(link.normalized_label.as_str())
            {
                self.diagnostics.push(Diagnostic {
                    range: link.label_range,
                    severity: Severity::Warning,
                    code: "unresolved-reference",
                    message: format!("unresolved reference label `{}`", link.label),
                    extension: None,
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
                    extension: None,
                });
            }
        }

        let anchors = self
            .local_references
            .iter()
            .map(|reference| reference.id.as_str())
            .collect::<HashSet<_>>();
        for link in &self.heading_links {
            if !anchors.contains(link.anchor.as_str()) {
                self.diagnostics.push(Diagnostic {
                    range: link.anchor_range,
                    severity: Severity::Warning,
                    code: "unresolved-heading",
                    message: format!("unresolved heading anchor `#{}`", link.anchor),
                    extension: None,
                });
            }
        }

        let local_refs = self.local_reference_ids();
        if workspace.has_citation_keys() {
            for citation in &self.citations {
                if workspace.has_duplicate_citation_key(&citation.key) {
                    self.diagnostics.push(Diagnostic {
                        range: citation.key_range,
                        severity: Severity::Warning,
                        code: "duplicate-bib-key",
                        message: format!("duplicate bibliography key `@{}`", citation.key),
                        extension: None,
                    });
                }
                if !workspace.contains_citation_key(&citation.key)
                    && !local_refs.contains(&citation.key)
                {
                    self.diagnostics.push(Diagnostic {
                        range: citation.key_range,
                        severity: Severity::Warning,
                        code: "unresolved-citation",
                        message: format!("unresolved citation `@{}`", citation.key),
                        extension: None,
                    });
                }
            }
        }
    }

    fn duplicate_anchor_diagnostics(&self) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        let mut seen = HashSet::<String>::new();
        for reference in &self.local_references {
            if !seen.insert(reference.id.clone()) {
                diagnostics.push(Diagnostic {
                    range: reference.id_range,
                    severity: Severity::Warning,
                    code: "duplicate-anchor",
                    message: format!("duplicate document anchor `#{}`", reference.id),
                    extension: None,
                });
            }
        }
        diagnostics
    }
}

/// A symbol found at a byte offset.
#[derive(Debug, Clone, Copy)]
pub enum SymbolAtOffset<'a> {
    Heading(&'a Heading),
    FencedDiv(&'a FencedDiv),
    LocalReference(&'a LocalReference),
    ReferenceDefinition(&'a ReferenceDefinition),
    FootnoteDefinition(&'a FootnoteDefinition),
    ReferenceLink(&'a ReferenceLink),
    FootnoteReference(&'a FootnoteReference),
    InlineNote(&'a InlineNote),
    HeadingLink(&'a HeadingLink),
    Citation(&'a Citation),
}

/// Normalize a reference label the way Pandoc does: case-folded internal
/// whitespace collapsed.
pub fn normalize_label(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Backwards-compatible helper: Pandoc's default `auto_identifiers` slug
/// with `smart` enabled.
pub fn slugify_heading(title: &str) -> String {
    slugify(title, IdentifierOptions::default())
}

fn push_unique_range(
    ranges: &mut Vec<pandocmd_syntax::TextRange>,
    range: pandocmd_syntax::TextRange,
) {
    if !ranges.contains(&range) {
        ranges.push(range);
    }
}

/// Turn (key, range, code, message) items into one diagnostic per key that
/// appears more than once.
fn duplicate_diagnostics(
    items: Vec<(
        String,
        pandocmd_syntax::TextRange,
        &'static str,
        &'static str,
    )>,
) -> Vec<Diagnostic> {
    let mut seen = HashSet::new();
    let mut diagnostics = Vec::new();
    for (key, range, code, message) in items {
        if !seen.insert(key) {
            diagnostics.push(Diagnostic {
                range,
                severity: Severity::Warning,
                code,
                message: message.to_string(),
                extension: None,
            });
        }
    }
    diagnostics
}
