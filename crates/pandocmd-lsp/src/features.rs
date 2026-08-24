//! Feature builders for the Pandoc Markdown language server: hover text,
//! completions, code actions, renames, document links, and semantic tokens.

use std::collections::BTreeSet;

use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Command, CompletionItem, CompletionItemKind,
    CompletionTextEdit, InsertTextFormat, TextEdit, Url,
};
use pandocmd_analysis::{
    BibliographyEntry, Citation, FencedDiv, HeadingLink, LocalReference, Severity, SymbolAtOffset,
    WorkspaceIndex,
};
use pandocmd_syntax::TextRange;

use crate::config::ResolvedSettings;
use crate::document::OpenDocument;

// ------------------------------------------------------------------- hover

pub fn hover_text(
    document: &OpenDocument,
    workspace: &WorkspaceIndex,
    symbol: SymbolAtOffset<'_>,
) -> String {
    match symbol {
        SymbolAtOffset::Heading(heading) => {
            let identifier = match &heading.anchor {
                Some(anchor) => format!("Anchor: `{anchor}`"),
                None => "No identifier (auto identifier extensions are disabled)".to_string(),
            };
            format!(
                "Heading {}\n\n{}\n\n{}",
                "#".repeat(heading.level as usize),
                markdown_code(&heading.title),
                identifier
            )
        }
        SymbolAtOffset::FencedDiv(div) => fenced_div_hover_text(div),
        SymbolAtOffset::LocalReference(reference) => local_reference_hover_text(reference),
        SymbolAtOffset::ReferenceDefinition(definition) => {
            if definition.target.is_empty() {
                format!(
                    "Reference definition `{}`",
                    markdown_code(&definition.label)
                )
            } else {
                format!(
                    "Reference definition\n\nTarget: {}",
                    markdown_code(&definition.target)
                )
            }
        }
        SymbolAtOffset::ReferenceLink(link) => {
            match document.analysis.reference_definition(&link.label) {
                Some(definition) => format!(
                    "Reference link\n\nTarget: {}",
                    markdown_code(&definition.target)
                ),
                None => format!(
                    "Unresolved reference {}\n\nNo definition for this label",
                    markdown_code(&link.label)
                ),
            }
        }
        SymbolAtOffset::FootnoteDefinition(definition) => format!(
            "Footnote definition {}\n\nJump to references with definition",
            markdown_code(&definition.label)
        ),
        SymbolAtOffset::FootnoteReference(reference) => {
            if document
                .analysis
                .footnote_definition(&reference.label)
                .is_some()
            {
                format!(
                    "Footnote {}\n\nJump to definition",
                    markdown_code(&reference.label)
                )
            } else {
                format!(
                    "Unresolved footnote {}\n\nNo definition for this label",
                    markdown_code(&reference.label)
                )
            }
        }
        SymbolAtOffset::InlineNote(note) => {
            format!("Inline note\n\n{}", markdown_code(&note.content))
        }
        SymbolAtOffset::HeadingLink(link) => heading_link_hover_text(document, link),
        SymbolAtOffset::Citation(citation) => citation_hover_text(document, workspace, citation),
    }
}

fn citation_hover_text(
    document: &OpenDocument,
    workspace: &WorkspaceIndex,
    citation: &Citation,
) -> String {
    if let Some(entry) = workspace.citation_entry(&citation.key) {
        return bibliography_entry_hover_text(&citation.key, entry);
    }

    if let Some(div) = document.analysis.div_by_id(&citation.key) {
        return fenced_div_hover_text(div);
    }

    if let Some(heading) = document.analysis.heading_by_anchor(&citation.key) {
        return match &heading.anchor {
            Some(anchor) => format!(
                "Heading: {}\n\nAnchor: {}",
                markdown_code(&heading.title),
                markdown_code(anchor)
            ),
            None => format!("Heading: {}", markdown_code(&heading.title)),
        };
    }

    if let Some(reference) = document.analysis.local_reference(&citation.key) {
        return local_reference_hover_text(reference);
    }

    format!(
        "Unresolved citation {}\n\nNo bibliography entry or local label",
        markdown_code(&citation.key)
    )
}

fn heading_link_hover_text(document: &OpenDocument, link: &HeadingLink) -> String {
    if let Some(heading) = document.analysis.heading_by_anchor(&link.anchor) {
        return format!(
            "Heading: {}\n\nAnchor: {}",
            markdown_code(&heading.title),
            markdown_code(&link.anchor)
        );
    }

    if let Some(div) = document.analysis.div_by_id(&link.anchor) {
        return fenced_div_hover_text(div);
    }

    if let Some(reference) = document.analysis.local_reference(&link.anchor) {
        return local_reference_hover_text(reference);
    }

    format!(
        "Unresolved anchor {}\n\nNo heading, div, or attribute defines it",
        markdown_code(&link.anchor)
    )
}

fn bibliography_entry_hover_text(key: &str, entry: &BibliographyEntry) -> String {
    let mut details = Vec::new();
    if let Some(authors) = &entry.authors {
        details.push(format!("Author: {authors}"));
    }
    if let Some(title) = &entry.title {
        details.push(format!("Title: {title}"));
    }
    if let Some(year) = &entry.year {
        details.push(format!("Year: {year}"));
    }

    if details.is_empty() {
        format!("Citation {}", markdown_code(&format!("@{key}")))
    } else {
        format!(
            "{}\n\n{}",
            markdown_code(&format!("@{key}")),
            details.join("  \n")
        )
    }
}

fn local_reference_hover_text(reference: &LocalReference) -> String {
    let detail = capitalized(&reference.detail);
    format!("{}: {}", detail, markdown_code(&reference.id))
}

fn fenced_div_hover_text(div: &FencedDiv) -> String {
    let mut sections = vec![fenced_div_summary(div)];
    let mut details = Vec::new();

    if let Some(id) = &div.id {
        details.push(format!("ID: {}", markdown_code(&format!("#{id}"))));
    }
    if !div.classes.is_empty() {
        let classes = div
            .classes
            .iter()
            .map(|class| markdown_code(class))
            .collect::<Vec<_>>()
            .join(", ");
        details.push(format!("Classes: {classes}"));
    }
    if !div.attributes.is_empty() {
        let attributes = div
            .attributes
            .iter()
            .map(|attribute| {
                let text = if let Some(value) = &attribute.value {
                    format!("{}=\"{}\"", attribute.key, value)
                } else {
                    attribute.key.clone()
                };
                markdown_code(&text)
            })
            .collect::<Vec<_>>()
            .join(", ");
        details.push(format!("Attributes: {attributes}"));
    }

    if !details.is_empty() {
        sections.push(details.join("  \n"));
    }

    sections.join("\n\n")
}

fn fenced_div_summary(div: &FencedDiv) -> String {
    let kind = div
        .classes
        .first()
        .map(String::as_str)
        .unwrap_or("fenced div");
    let kind = markdown_code(kind);

    if let Some(title) = div.title() {
        format!("{kind}: {title}")
    } else {
        kind
    }
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => text.to_string(),
    }
}

fn markdown_code(text: &str) -> String {
    format!("`{}`", text.replace('`', "\\`"))
}

// -------------------------------------------------------------- completion

/// What kind of completion the cursor position implies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionContext {
    /// Inside `[@key` (or `-@key`): citation keys.
    Citation { prefix: String },
    /// Inside `](#anchor`: document anchors.
    Anchor { prefix: String },
    /// Inside `[^label`: footnote labels.
    Footnote { prefix: String },
    /// Inside `[text][label`: reference labels.
    ReferenceLabel { prefix: String },
}

pub fn completion_context_at(text: &str, offset: usize) -> Option<CompletionContext> {
    let before = &text[..offset];

    // `](#anchor`
    if let Some(hash) = before.rfind("](#") {
        if !before[hash + 3..].contains(')') {
            return Some(CompletionContext::Anchor {
                prefix: before[hash + 3..].to_string(),
            });
        }
    }

    // `[@key` / `[-@key`
    if let Some(start) = citation_completion_start(text, offset) {
        let prefix = text.get(start + 1..offset)?.to_string();
        return Some(CompletionContext::Citation { prefix });
    }

    // `[^label`
    if let Some(caret) = before.rfind("[^") {
        let between = &before[caret + 2..];
        if !between.contains(']') && is_label_text(between) {
            return Some(CompletionContext::Footnote {
                prefix: between.to_string(),
            });
        }
    }

    // `[text][label`
    if let Some(close) = before.rfind(']') {
        if before[close..].starts_with("][") {
            let label = &before[close + 2..];
            if !label.contains(']') && is_label_text(label) {
                return Some(CompletionContext::ReferenceLabel {
                    prefix: label.to_string(),
                });
            }
        }
    }

    None
}

fn is_label_text(text: &str) -> bool {
    text.chars().all(|ch| !matches!(ch, '\n' | '[' | ')' | ' '))
        || text.chars().all(|ch| !matches!(ch, '\n' | '[' | ')'))
}

pub fn completion_items(
    document: &OpenDocument,
    workspace: &WorkspaceIndex,
    context: &CompletionContext,
    edit_range: lsp_types::Range,
    settings: &ResolvedSettings,
) -> Vec<CompletionItem> {
    let (prefix, kind): (String, CompletionItemKind) = match context {
        CompletionContext::Citation { prefix } => {
            if !settings.completion_citations {
                return Vec::new();
            }
            (prefix.clone(), CompletionItemKind::REFERENCE)
        }
        CompletionContext::Anchor { prefix } => {
            if !settings.completion_anchors {
                return Vec::new();
            }
            (prefix.clone(), CompletionItemKind::REFERENCE)
        }
        CompletionContext::Footnote { prefix } => (prefix.clone(), CompletionItemKind::REFERENCE),
        CompletionContext::ReferenceLabel { prefix } => {
            if !settings.completion_reference_labels {
                return Vec::new();
            }
            (prefix.clone(), CompletionItemKind::REFERENCE)
        }
    };

    let insert_prefix = match context {
        CompletionContext::Citation { .. } => "@",
        _ => "",
    };

    let mut items = Vec::new();
    let mut seen = BTreeSet::new();

    let mut candidates: Vec<(String, String)> = match context {
        CompletionContext::Citation { .. } => {
            let mut local: Vec<(String, String)> = document
                .analysis
                .local_references
                .iter()
                .map(|r| (r.id.clone(), r.detail.clone()))
                .collect();
            let mut bib: Vec<(String, String)> = workspace
                .citation_entries()
                .map(|entry| {
                    let detail = entry
                        .completion_detail()
                        .unwrap_or_else(|| "Bibliography citation".to_string());
                    (entry.key.clone(), detail)
                })
                .collect();
            local.append(&mut bib);
            local
        }
        CompletionContext::Anchor { .. } => document
            .analysis
            .local_references
            .iter()
            .map(|r| (r.id.clone(), r.detail.clone()))
            .collect(),
        CompletionContext::Footnote { .. } => document
            .analysis
            .footnote_definitions
            .iter()
            .map(|definition| (definition.label.clone(), "footnote".to_string()))
            .collect(),
        CompletionContext::ReferenceLabel { .. } => document
            .analysis
            .reference_definitions
            .iter()
            .map(|definition| (definition.label.clone(), definition.target.clone()))
            .collect(),
    };
    candidates.sort_by(|left, right| left.0.cmp(&right.0));
    candidates.dedup_by(|left, right| left.0 == right.0);

    for (key, detail) in &candidates {
        if !key.starts_with(prefix.as_str()) || !seen.insert(key.clone()) {
            continue;
        }

        let label = format!("{insert_prefix}{key}");
        let detail = detail.clone();
        items.push(CompletionItem {
            label: label.clone(),
            kind: Some(kind),
            detail: Some(if detail.is_empty() {
                label.clone()
            } else {
                detail
            }),
            filter_text: Some(label.clone()),
            text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                range: edit_range,
                new_text: label,
            })),
            insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
            ..CompletionItem::default()
        });
    }

    items
}

fn citation_completion_start(text: &str, offset: usize) -> Option<usize> {
    let mut start = offset;
    while let Some((idx, ch)) = previous_char(text, start) {
        if is_citation_key_char(ch) {
            start = idx;
        } else {
            break;
        }
    }

    let (at_idx, ch) = previous_char(text, start)?;
    if ch == '@' && can_start_citation(text, at_idx) {
        Some(at_idx)
    } else {
        None
    }
}

fn previous_char(text: &str, offset: usize) -> Option<(usize, char)> {
    text.get(..offset)?.char_indices().next_back()
}

fn can_start_citation(text: &str, at_offset: usize) -> bool {
    match previous_char(text, at_offset) {
        Some((_, '[')) => true,
        Some((dash_offset, '-')) => matches!(previous_char(text, dash_offset), Some((_, '['))),
        _ => false,
    }
}

fn is_citation_key_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric()
        || matches!(
            ch,
            '_' | ':' | '.' | '#' | '$' | '%' | '&' | '+' | '-' | '?' | '<' | '>' | '~' | '/'
        )
}

// ------------------------------------------------------------- code actions

pub fn code_actions(
    document: &OpenDocument,
    diagnostics: &[pandocmd_analysis::Diagnostic],
) -> Vec<CodeActionOrCommand> {
    let text = document.parsed.text();
    let mut actions = Vec::new();

    for diagnostic in diagnostics {
        let label = &text[diagnostic.range.start..diagnostic.range.end];
        match diagnostic.code {
            "unresolved-footnote" | "unresolved-reference" => {
                let is_footnote = diagnostic.code == "unresolved-footnote";
                let (title, insertion) = if is_footnote {
                    (
                        format!("Create footnote definition `[^{label}]`"),
                        format!("\n\n[^{label}]: "),
                    )
                } else {
                    (
                        format!("Create reference definition `[{label}]`"),
                        format!("\n\n[{label}]: "),
                    )
                };

                // Insert at the end of the document.
                let end = {
                    let position = document
                        .parsed
                        .line_index()
                        .offset_to_position(text, text.len());
                    lsp_types::Position::new(position.line, position.character)
                };
                let lsp_diagnostic = crate::analysis_diagnostic_to_lsp(document, diagnostic);
                actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title,
                    kind: Some(CodeActionKind::QUICKFIX),
                    diagnostics: Some(vec![lsp_diagnostic]),
                    edit: Some(lsp_types::WorkspaceEdit {
                        changes: Some(
                            [(
                                document.uri.clone(),
                                vec![TextEdit {
                                    range: lsp_types::Range::new(end, end),
                                    new_text: insertion,
                                }],
                            )]
                            .into_iter()
                            .collect(),
                        ),
                        ..lsp_types::WorkspaceEdit::default()
                    }),
                    ..CodeAction::default()
                }));
            }
            "extension-disabled" => {
                if let Some(extension) = diagnostic.extension {
                    actions.push(CodeActionOrCommand::Command(Command {
                        title: format!(
                            "Enable the `{extension}` extension in Pandoc Markdown settings"
                        ),
                        command: "pandocmd.enableExtension".to_string(),
                        arguments: Some(vec![serde_json::json!({ "extension": extension })]),
                    }));
                }
            }
            _ => {}
        }
    }
    actions
}

// ------------------------------------------------------------------ rename

/// Compute the ranges a rename should replace, or an error message when the
/// symbol cannot be renamed.
pub fn rename_ranges(
    document: &OpenDocument,
    symbol: SymbolAtOffset<'_>,
) -> Result<Vec<TextRange>, String> {
    match symbol {
        SymbolAtOffset::ReferenceDefinition(definition) => Ok(document
            .analysis
            .reference_ranges_for_label(&definition.label)),
        SymbolAtOffset::ReferenceLink(link) => {
            Ok(document.analysis.reference_ranges_for_label(&link.label))
        }
        SymbolAtOffset::FootnoteDefinition(definition) => Ok(document
            .analysis
            .footnote_ranges_for_label(&definition.label)),
        SymbolAtOffset::FootnoteReference(reference) => Ok(document
            .analysis
            .footnote_ranges_for_label(&reference.label)),
        SymbolAtOffset::Heading(heading) => match heading.identifier_source {
            pandocmd_analysis::IdentifierSource::Explicit => Ok(document
                .analysis
                .local_reference_ranges_for_id(heading.anchor.as_deref().unwrap_or_default())),
            _ => Err(
                "headings without an explicit {#id} use automatic anchors and cannot be renamed"
                    .to_string(),
            ),
        },
        SymbolAtOffset::FencedDiv(div) => match &div.id {
            Some(id) => Ok(document.analysis.local_reference_ranges_for_id(id)),
            None => Err("fenced divs without an id cannot be renamed".to_string()),
        },
        SymbolAtOffset::LocalReference(reference) => Ok(document
            .analysis
            .local_reference_ranges_for_id(&reference.id)),
        SymbolAtOffset::HeadingLink(link) => Ok(document
            .analysis
            .local_reference_ranges_for_id(&link.anchor)),
        SymbolAtOffset::Citation(citation) => {
            let ranges: Vec<TextRange> = document
                .analysis
                .citations
                .iter()
                .filter(|other| other.key == citation.key)
                .map(|other| other.key_range)
                .collect();
            if ranges.is_empty() {
                Err("citation cannot be renamed".to_string())
            } else {
                Ok(ranges)
            }
        }
        SymbolAtOffset::InlineNote(_) => Err("inline notes cannot be renamed".to_string()),
    }
}

// ----------------------------------------------------------- document links

pub fn document_links(document: &OpenDocument) -> Vec<lsp_types::DocumentLink> {
    let text = document.parsed.text();
    let line_index = document.parsed.line_index();
    let mut links = Vec::new();

    for link in &document.analysis.links {
        let target = match resolved_link_target(&document.uri, &link.target) {
            Some(target) => target,
            None => continue,
        };
        let (start, end) = line_index.range_to_positions(text, link.target_range);
        links.push(lsp_types::DocumentLink {
            range: lsp_types::Range::new(
                lsp_types::Position::new(start.line, start.character),
                lsp_types::Position::new(end.line, end.character),
            ),
            target: Some(target),
            tooltip: link.label.clone(),
            data: None,
        });
    }

    links
}

fn resolved_link_target(base: &Url, target: &str) -> Option<Url> {
    if target.is_empty() {
        return None;
    }
    if target.starts_with('#') {
        // Internal anchor: link to the document itself.
        return Url::parse(&format!("{}{}", base, target)).ok();
    }
    if target.contains("://") {
        return Url::parse(target).ok();
    }
    base.join(target).ok()
}

// ------------------------------------------------------- semantic tokens

pub const SEMANTIC_TOKEN_LEGEND: &[&str] = &[
    "heading",
    "fencedDiv",
    "codeFence",
    "citation",
    "footnote",
    "math",
    "link",
];

/// Encode analysis semantic tokens into the LSP delta format.
///
/// LSP semantic tokens cannot span lines, so a token whose analysis range
/// crosses a line boundary (for example a link whose text wraps onto the
/// next line) is clamped to the end of its first line.
pub fn semantic_tokens(document: &OpenDocument) -> Vec<lsp_types::SemanticToken> {
    let text = document.parsed.text();
    let line_index = document.parsed.line_index();
    let mut tokens: Vec<_> = document
        .analysis
        .semantic_tokens
        .iter()
        .map(|token| {
            let (start, end) = line_index.range_to_positions(text, token.range);
            let end = if end.line > start.line {
                match line_index.line_end(text, start.line as usize) {
                    Some(offset) => line_index.offset_to_position(text, offset),
                    None => end,
                }
            } else {
                end
            };
            let kind_index = SEMANTIC_TOKEN_LEGEND
                .iter()
                .position(|name| *name == token.kind.name())
                .unwrap_or(0) as u32;
            (
                start.line,
                start.character,
                end.character.saturating_sub(start.character),
                kind_index,
            )
        })
        .filter(|(_, _, length, _)| *length > 0)
        .collect();
    tokens.sort();

    let mut data = Vec::new();
    let mut last_line = 0u32;
    let mut last_char = 0u32;
    for (line, character, length, token_type) in tokens {
        let delta_line = line - last_line;
        let delta_start = if delta_line == 0 {
            character - last_char
        } else {
            character
        };
        data.push(lsp_types::SemanticToken {
            delta_line,
            delta_start,
            length,
            token_type,
            token_modifiers_bitset: 0,
        });
        last_line = line;
        last_char = character;
    }
    data
}

// -------------------------------------------------------------- diagnostics

pub fn diagnostics_from_analysis(document: &OpenDocument) -> Vec<lsp_types::Diagnostic> {
    let text = document.parsed.text();
    let line_index = document.parsed.line_index();
    document
        .analysis
        .diagnostics
        .iter()
        .map(|diagnostic| {
            let (start, end) = line_index.range_to_positions(text, diagnostic.range);
            #[allow(clippy::redundant_closure)]
            let severity = match diagnostic.severity {
                Severity::Error => lsp_types::DiagnosticSeverity::ERROR,
                Severity::Warning => lsp_types::DiagnosticSeverity::WARNING,
                Severity::Information => lsp_types::DiagnosticSeverity::INFORMATION,
                Severity::Hint => lsp_types::DiagnosticSeverity::HINT,
            };
            lsp_types::Diagnostic {
                range: lsp_types::Range::new(
                    lsp_types::Position::new(start.line, start.character),
                    lsp_types::Position::new(end.line, end.character),
                ),
                severity: Some(severity),
                code: Some(lsp_types::NumberOrString::String(
                    diagnostic.code.to_string(),
                )),
                source: Some("pandocmd".to_string()),
                message: diagnostic.message.clone(),
                data: diagnostic
                    .extension
                    .map(|extension| serde_json::json!({ "extension": extension })),
                ..lsp_types::Diagnostic::default()
            }
        })
        .collect()
}
