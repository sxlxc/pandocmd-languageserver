//! Line-oriented scanning of Pandoc Markdown documents.
//!
//! The scanner is extension-aware: every construct is only recognized when
//! the corresponding Pandoc extension is enabled, constructs that are used
//! while their extension is disabled produce `extension-disabled`
//! diagnostics, and scanning never runs inside fenced code blocks or the
//! YAML metadata block.

use std::collections::HashMap;
use std::sync::LazyLock;

use pandocmd_extensions::{Extension, ExtensionSet};
use pandocmd_syntax::TextRange;
use regex::Regex;

use crate::identifiers::{slugify, uniquify, IdentifierAlgorithm, IdentifierOptions};
use crate::{
    AnalyzeOptions, Citation, Diagnostic, DivAttribute, FencedDiv, FootnoteDefinition,
    FootnoteReference, Heading, HeadingLink, IdentifierSource, InlineNote, LinkKind,
    LocalReference, MarkdownLink, ReferenceDefinition, ReferenceLink, SemanticToken,
    SemanticTokenKind, Severity,
};

static HEADING_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(#{1,6})[ \t]+(.+?)[ \t#]*$").unwrap());
static SETEXT_UNDERLINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[ \t]{0,3}(=+|-+)[ \t]*$").unwrap());
static FOOTNOTE_DEF_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[ \t]{0,3}\[\^([^\]\n]+)\]:").unwrap());
static REF_DEF_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[ \t]{0,3}\[([^\]\n]+)\]:[ \t]*(\S*)(?:[ \t]+(.*))?$").unwrap());
static FULL_REF_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([^\]\n]+)\]\[([^\]\n]+)\]").unwrap());
static COLLAPSED_REF_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([^\]\n]+)\]\[\]").unwrap());
static BRACKET_SPAN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([^\]\^\n]+)\]").unwrap());
static FOOTNOTE_REF_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\^([^\]\n]+)\]").unwrap());
static INLINE_NOTE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\^\[([^\]\n]+)\]").unwrap());
static HEADING_LINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\]\(#([A-Za-z0-9_.:\-]+)\)").unwrap());
static CITATION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(^|[\s;\[\(\s"'])(-?@)([A-Za-z0-9_:.#$%&+\-?<>~/]+)"#).unwrap());
static FENCED_DIV_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[ \t]{0,3}(:{3,})(.*)$").unwrap());
static BRACED_ATTR_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{([^}\n]*)\}").unwrap());
static IMAGE_ATTR_PREFIX_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"!\[[^\]\n]*\]\([^\)\n]*\)\s*$").unwrap());
static CODE_FENCE_ATTR_PREFIX_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[ \t]{0,3}(`{3,}|~{3,})\s*$").unwrap());
static INLINE_LINK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(!?)\[([^\]\n]*)\]\((<[^>\s]*>|[^()\s]*)(?:[ \t]+"([^\"]*)")?\)"#).unwrap()
});
static AUTOLINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<([a-zA-Z][a-zA-Z0-9+.-]{1,31}:[^<>\s]+)>").unwrap());
static STRIKEOUT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"~~([^\s~][^~]*?)~~").unwrap());
static SUPERSCRIPT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\^([^\s^\[\]]+)\^").unwrap());
static SUBSCRIPT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"~([^\s~]+)~").unwrap());
static MARK_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"==([^\s=][^=]*?)==").unwrap());
static DISPLAY_MATH_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\$\$([^$\n]+)\$\$").unwrap());
static INLINE_MATH_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\$([^\s$][^$\n]*?)\$").unwrap());
static MATH_BACKSLASH_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\\{1,2}([\(\[])([^\]\)\n]+)\\{1,2}([\)\]])").unwrap());
static MATH_GFM_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\$`([^`\n]+)`\$").unwrap());
static RAW_ATTR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"`[^`\n]*`\{=([a-zA-Z0-9_-]+)\}").unwrap());
static RAW_FENCE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[ \t]{0,3}(`{3,}|~{3,})\{=([a-zA-Z0-9_-]+)\}\s*$").unwrap());
static EMOJI_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r":([a-z0-9_+-]{2,}):").unwrap());
static TASK_LIST_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\s*)([-*+])([ \t]+)\[([ xX])\][ \t]+(\S.*)$").unwrap());
static ALERT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\s*>+\s*)\[!(NOTE|TIP|IMPORTANT|WARNING|CAUTION)\]").unwrap());
static EXAMPLE_LIST_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\(@(\w*)\)").unwrap());
static WIKILINK_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[\[([^\]\n]+)\]\]").unwrap());
static PIPE_TABLE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[ \t]*\|?[ \t]*:?-{3,}:?[ \t]*(\|[ \t]*:?-{3,}:?[ \t]*)*\|?[ \t]*$").unwrap()
});
static GRID_TABLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[ \t]*\+(-{3,}\+)+[ \t]*$").unwrap());
static DEF_LIST_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[ \t]{0,3}:[ \t]+\S").unwrap());
static HEADING_ATTR_TRAILING_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{#[A-Za-z0-9_.:\-]+[^}\n]*\}[ \t]*$").unwrap());

/// Scan a full document, filling the analysis with all recognized constructs
/// and extension-disabled diagnostics.
pub fn scan_document(text: &str, options: &AnalyzeOptions) -> ScanOutput {
    let mut scan = ScanState::new(options);
    let mut byte_offset = 0;

    for line in text.split_inclusive('\n') {
        let line_without_newline = line.trim_end_matches(['\r', '\n']);
        scan.line(line_without_newline, byte_offset);
        byte_offset += line.len();
    }

    if !text.is_empty() && !text.ends_with('\n') {
        // split_inclusive already covered the final line without newline.
    }

    scan.finish(text.len());
    scan.output
}

/// Result of scanning: the partially-filled analysis (no cross-reference
/// diagnostics yet) plus ranges needed by later phases.
pub struct ScanOutput {
    pub analysis: crate::DocumentAnalysis,
}

struct ScanState<'a> {
    options: &'a AnalyzeOptions,
    extensions: ExtensionSet,
    output: ScanOutput,
    div_stack: Vec<OpenDiv>,
    anchor_counts: HashMap<String, usize>,
    code_fence: Option<CodeFence>,
    in_metadata: bool,
    metadata_seen: bool,
    previous_was_paragraph: bool,
    /// Lines of the paragraph immediately above the cursor, used to turn
    /// `text\n====` into a setext heading.
    pending_paragraph: Vec<(usize, String)>,
    pending_setext_level: Option<u8>,
}

struct CodeFence {
    delimiter: char,
    length: usize,
}

#[derive(Debug)]
struct OpenDiv {
    index: usize,
    fence_len: usize,
    opening_range: TextRange,
}

impl<'a> ScanState<'a> {
    fn new(options: &'a AnalyzeOptions) -> Self {
        let extensions = options.extensions;
        ScanState {
            options,
            extensions,
            output: ScanOutput {
                analysis: crate::DocumentAnalysis::default(),
            },
            div_stack: Vec::new(),
            anchor_counts: HashMap::new(),
            code_fence: None,
            in_metadata: false,
            metadata_seen: false,
            previous_was_paragraph: false,
            pending_paragraph: Vec::new(),
            pending_setext_level: None,
        }
    }

    fn line(&mut self, line: &str, byte_offset: usize) {
        // YAML metadata block at the very start of the document.
        if !self.metadata_seen && !self.in_metadata {
            if byte_offset == 0
                && self.extensions.contains(Extension::YamlMetadataBlock)
                && line.trim_end() == "---"
            {
                self.in_metadata = true;
                return;
            }
            if byte_offset == 0 && !line.is_empty() {
                self.metadata_seen = true;
            }
        } else if self.in_metadata {
            if line.trim() == "---" || line.trim() == "..." {
                self.in_metadata = false;
                self.metadata_seen = true;
            }
            return;
        }

        // Fenced code blocks hide every other construct.
        if let Some(fence) = &self.code_fence {
            if let Some((delimiter, length)) = fence_marker(line) {
                if delimiter == fence.delimiter && length >= fence.length {
                    self.push_semantic_token(
                        SemanticTokenKind::CodeFence,
                        TextRange::new(byte_offset, byte_offset + line.len()),
                    );
                    self.code_fence = None;
                }
            }
            return;
        }

        if let Some((delimiter, length)) = fence_marker(line) {
            let raw_format = RAW_FENCE_RE
                .captures(line)
                .and_then(|captures| captures.get(2))
                .map(|format| format.as_str().to_string());
            if self.options.disabled_extensions
                && raw_format.is_some()
                && !self.extensions.contains(Extension::RawAttribute)
            {
                self.push_disabled_diagnostic(
                    Extension::RawAttribute,
                    TextRange::new(byte_offset, byte_offset + line.len()),
                    Severity::Warning,
                    "raw attribute block is disabled",
                );
            }
            self.code_fence = Some(CodeFence { delimiter, length });
            // Fenced code attributes: ```{#id .lang} info strings define a
            // local reference target.
            if self.extensions.contains(Extension::FencedCodeAttributes) {
                self.scan_braced_attribute_references(line, byte_offset);
            }
            self.push_semantic_token(
                SemanticTokenKind::CodeFence,
                TextRange::new(byte_offset, byte_offset + line.len()),
            );
            self.previous_was_paragraph = false;
            return;
        }

        // Setext heading underline: `Heading text` + `======`.
        if self.previous_was_paragraph {
            if let Some(captures) = SETEXT_UNDERLINE_RE.captures(line) {
                let marker = captures.get(1).unwrap();
                self.pending_setext_level = Some(if marker.as_str().starts_with('=') {
                    1
                } else {
                    2
                });
                self.push_setext_heading(byte_offset + line.len());
                return;
            }
        }

        self.scan_div_line(line, byte_offset);
        self.scan_block_line(line, byte_offset);
        self.scan_inline_line(line, byte_offset);
        self.scan_disabled_inline(line, byte_offset);
        self.scan_links(line, byte_offset);

        self.previous_was_paragraph = !line.trim().is_empty()
            && !HEADING_RE.is_match(line)
            && !REF_DEF_RE.is_match(line)
            && !FOOTNOTE_DEF_RE.is_match(line);
        if self.previous_was_paragraph {
            self.pending_paragraph.push((byte_offset, line.to_string()));
        } else {
            self.pending_paragraph.clear();
        }
    }

    /// Convert the pending paragraph plus the underline line just seen into
    /// a setext heading.
    fn push_setext_heading(&mut self, document_end: usize) {
        let Some((first_offset, first_line)) = self.pending_paragraph.first().cloned() else {
            self.previous_was_paragraph = false;
            self.pending_paragraph.clear();
            return;
        };
        let last_end = self
            .pending_paragraph
            .last()
            .map(|(offset, line)| offset + line.len())
            .unwrap_or(document_end);
        let title = self
            .pending_paragraph
            .iter()
            .map(|(_, line)| line.trim().to_string())
            .collect::<Vec<_>>()
            .join(" ");
        let trimmed_start = first_line.len() - first_line.trim_start().len();

        let mut id = None;
        let mut source = IdentifierSource::None;
        if let Some((auto_id, algorithm)) = self.automatic_identifier(&title) {
            id = Some(auto_id);
            source = algorithm;
        }
        let anchor = match source {
            IdentifierSource::Explicit => id,
            _ => id.map(|base| uniquify(&base, &mut self.anchor_counts)),
        };

        let title_start = first_offset + trimmed_start;
        let title_end = first_offset + first_line.trim_end().len();
        let title_range = pandocmd_syntax::TextRange::new(title_start, title_end);
        if let Some(anchor) = &anchor {
            self.output.analysis.local_references.push(LocalReference {
                id: anchor.clone(),
                detail: "section".to_string(),
                range: pandocmd_syntax::TextRange::new(first_offset, last_end),
                id_range: title_range,
            });
        }
        self.output.analysis.headings.push(Heading {
            level: 1, // corrected below via marker
            title,
            anchor,
            identifier_source: source,
            range: pandocmd_syntax::TextRange::new(first_offset, last_end),
            selection_range: title_range,
            id_range: None,
        });
        if let Some(heading) = self.output.analysis.headings.last_mut() {
            heading.level = self.pending_setext_level.take().unwrap_or(1);
        }
        self.push_semantic_token(
            SemanticTokenKind::Heading,
            pandocmd_syntax::TextRange::new(first_offset, last_end),
        );
        self.pending_paragraph.clear();
        self.previous_was_paragraph = false;
    }

    fn finish(&mut self, document_len: usize) {
        self.finish_unclosed_fenced_divs(document_len);
        let analysis = &mut self.output.analysis;
        analysis
            .diagnostics
            .sort_by_key(|diagnostic| diagnostic.range.start);
        analysis.links.sort_by_key(|link| link.range.start);
        analysis
            .semantic_tokens
            .sort_by_key(|token| token.range.start);
    }

    // ---- Headings -------------------------------------------------------

    fn scan_block_line(&mut self, line: &str, byte_offset: usize) {
        if let Some(captures) = HEADING_RE.captures(line) {
            let marker = captures.get(1).unwrap();
            let title_match = captures.get(2).unwrap();
            self.push_heading(
                line,
                byte_offset,
                marker.as_str().len() as u8,
                title_match.start(),
                title_match.end(),
            );
            return;
        }

        // With `space_in_atx_header` disabled, "#Heading" (no space) is a
        // heading too.
        if !self.extensions.contains(Extension::SpaceInAtxHeader) && line.starts_with('#') {
            let hashes = line.chars().take_while(|ch| *ch == '#').count();
            let rest = &line[hashes..];
            if (1..=6).contains(&hashes) && !rest.is_empty() && !rest.starts_with([' ', '\t', '#'])
            {
                self.push_heading(
                    line,
                    byte_offset,
                    hashes as u8,
                    hashes,
                    line.trim_end().len(),
                );
                return;
            }
        }

        if let Some(captures) = FOOTNOTE_DEF_RE.captures(line) {
            let label = captures.get(1).unwrap();
            if self.extensions.contains(Extension::Footnotes) {
                self.output
                    .analysis
                    .footnote_definitions
                    .push(FootnoteDefinition {
                        label: label.as_str().to_string(),
                        normalized_label: crate::normalize_label(label.as_str()),
                        range: TextRange::new(byte_offset, byte_offset + line.len()),
                        label_range: TextRange::new(
                            byte_offset + label.start(),
                            byte_offset + label.end(),
                        ),
                    });
            } else if self.options.disabled_extensions {
                let label_range =
                    TextRange::new(byte_offset, byte_offset + captures.get(0).unwrap().end());
                self.push_disabled_diagnostic(
                    Extension::Footnotes,
                    label_range,
                    Severity::Warning,
                    "footnote definitions are disabled",
                );
            }
            return;
        }

        if let Some(captures) = REF_DEF_RE.captures(line) {
            let label = captures.get(1).unwrap();
            let target = captures.get(2).unwrap();
            if !target.as_str().is_empty() {
                self.output.analysis.links.push(MarkdownLink {
                    kind: LinkKind::Definition,
                    target: target.as_str().to_string(),
                    label: Some(label.as_str().to_string()),
                    range: TextRange::new(byte_offset, byte_offset + line.len()),
                    target_range: TextRange::new(
                        byte_offset + target.start(),
                        byte_offset + target.end(),
                    ),
                });
                self.push_semantic_token(
                    SemanticTokenKind::Link,
                    TextRange::new(byte_offset + target.start(), byte_offset + target.end()),
                );
            }
            self.output
                .analysis
                .reference_definitions
                .push(ReferenceDefinition {
                    label: label.as_str().to_string(),
                    normalized_label: crate::normalize_label(label.as_str()),
                    target: target.as_str().to_string(),
                    range: TextRange::new(byte_offset, byte_offset + line.len()),
                    label_range: TextRange::new(
                        byte_offset + label.start(),
                        byte_offset + label.end(),
                    ),
                });
            self.scan_links(line, byte_offset);
        }
    }

    /// Compute the heading identifier according to extension precedence:
    /// explicit `header_attributes` > `gfm_auto_identifiers` >
    /// `auto_identifiers` (+`ascii_identifiers`) > none.
    fn push_heading(
        &mut self,
        line: &str,
        byte_offset: usize,
        level: u8,
        title_start: usize,
        title_end: usize,
    ) {
        let mut title_end = title_end;
        let mut id = None;
        let mut id_range = None;
        let mut source = IdentifierSource::None;

        let header_attrs_on = self.extensions.contains(Extension::HeaderAttributes);
        if header_attrs_on {
            if let Some(attributes) =
                braced_attribute_sets(line, byte_offset)
                    .into_iter()
                    .rfind(|attributes| {
                        let start = attributes.whole_range.start.saturating_sub(byte_offset);
                        let end = attributes.whole_range.end.saturating_sub(byte_offset);
                        title_start <= start && end == title_end
                    })
            {
                if attributes.id.is_some() {
                    let attr_start = attributes.whole_range.start.saturating_sub(byte_offset);
                    let display_end = trim_ascii_whitespace_end(line, title_start, attr_start);
                    title_end = display_end;
                    id = attributes.id.clone();
                    id_range = attributes.id_range;
                    source = IdentifierSource::Explicit;
                }
            }
        } else if self.options.disabled_extensions && HEADING_ATTR_TRAILING_RE.is_match(line) {
            self.push_disabled_diagnostic(
                Extension::HeaderAttributes,
                TextRange::new(byte_offset + title_start, byte_offset + title_end),
                Severity::Warning,
                "heading attributes are disabled",
            );
        }

        let mut title = line[title_start..title_end].to_string();
        if source == IdentifierSource::None {
            if let Some((auto_id, algorithm)) = self.automatic_identifier(&title) {
                id = Some(auto_id);
                source = algorithm;
            }
        }
        let title_range = TextRange::new(byte_offset + title_start, byte_offset + title_end);

        let anchor = match source {
            IdentifierSource::Explicit => id,
            _ => id.map(|base| uniquify(&base, &mut self.anchor_counts)),
        };
        if let Some(anchor) = &anchor {
            let id_range = id_range.unwrap_or(title_range);
            self.output.analysis.local_references.push(LocalReference {
                id: anchor.clone(),
                detail: "section".to_string(),
                range: TextRange::new(byte_offset, byte_offset + line.len()),
                id_range,
            });
        }

        title = title.trim_end().to_string();
        self.output.analysis.headings.push(Heading {
            level,
            title,
            anchor: anchor.clone(),
            identifier_source: source,
            range: TextRange::new(byte_offset, byte_offset + line.len()),
            selection_range: title_range,
            id_range,
        });
        self.push_semantic_token(
            SemanticTokenKind::Heading,
            TextRange::new(byte_offset, byte_offset + line.len()),
        );
    }

    fn automatic_identifier(&self, title: &str) -> Option<(String, IdentifierSource)> {
        let identifier_options = IdentifierOptions {
            algorithm: IdentifierAlgorithm::Pandoc,
            smart: self.extensions.contains(Extension::Smart),
            ascii: self.extensions.contains(Extension::AsciiIdentifiers),
        };
        if self.extensions.contains(Extension::GfmAutoIdentifiers) {
            return Some((
                slugify(
                    title,
                    IdentifierOptions {
                        algorithm: IdentifierAlgorithm::Gfm,
                        ..identifier_options
                    },
                ),
                IdentifierSource::Gfm,
            ));
        }
        if self.extensions.contains(Extension::AutoIdentifiers) {
            return Some((slugify(title, identifier_options), IdentifierSource::Auto));
        }
        None
    }

    // ---- Inline constructs ----------------------------------------------

    fn scan_inline_line(&mut self, line: &str, byte_offset: usize) {
        let masked = mask_inline_code(line);
        let is_footnote_definition = FOOTNOTE_DEF_RE.is_match(line);
        let is_reference_definition = REF_DEF_RE.is_match(line);

        if !is_reference_definition {
            for captures in FULL_REF_RE.captures_iter(&masked) {
                let whole = captures.get(0).unwrap();
                let label = captures.get(2).unwrap();
                if label.as_str().starts_with('^') {
                    continue;
                }
                self.output.analysis.reference_links.push(ReferenceLink {
                    label: label.as_str().to_string(),
                    normalized_label: crate::normalize_label(label.as_str()),
                    range: TextRange::new(byte_offset + whole.start(), byte_offset + whole.end()),
                    label_range: TextRange::new(
                        byte_offset + label.start(),
                        byte_offset + label.end(),
                    ),
                });
            }

            for captures in COLLAPSED_REF_RE.captures_iter(&masked) {
                let whole = captures.get(0).unwrap();
                let label = captures.get(1).unwrap();
                self.output.analysis.reference_links.push(ReferenceLink {
                    label: label.as_str().to_string(),
                    normalized_label: crate::normalize_label(label.as_str()),
                    range: TextRange::new(byte_offset + whole.start(), byte_offset + whole.end()),
                    label_range: TextRange::new(
                        byte_offset + label.start(),
                        byte_offset + label.end(),
                    ),
                });
            }

            if self.extensions.contains(Extension::ShortcutReferenceLinks) {
                for captures in BRACKET_SPAN_RE.captures_iter(&masked) {
                    let whole = captures.get(0).unwrap();
                    let label = captures.get(1).unwrap();
                    // Manual neighbor checks replace lookarounds (unsupported
                    // in Rust's regex crate). `[@cite]` and `^[note]` belong
                    // to other extensions; `![img]` is an image; `- [ ]` is
                    // a task list item.
                    let prefix = masked.get(..whole.start()).unwrap_or("");
                    let before_ok = prefix
                        .chars()
                        .next_back()
                        .is_none_or(|ch| !matches!(ch, '!' | '[' | ']' | '^'));
                    let task_item = matches!(label.as_str(), " " | "x" | "X")
                        && prefix
                            .trim_end()
                            .chars()
                            .next_back()
                            .is_some_and(|ch| matches!(ch, '-' | '*' | '+'));
                    let after = masked.get(whole.end()..).unwrap_or("");
                    let after_ok = !after.starts_with(['(', '[', ':', '{']);
                    // Citation groups like `[see @doe99]` belong to the
                    // citations extension; `[!NOTE]` to alerts; `[x]` to
                    // task lists.
                    let citations_on = self.extensions.contains(Extension::Citations);
                    if before_ok
                        && after_ok
                        && !task_item
                        && (!citations_on || !label.as_str().contains('@'))
                        && !label.as_str().starts_with('@')
                        && !label.as_str().starts_with('!')
                        && !label.as_str().trim().is_empty()
                    {
                        self.output.analysis.reference_links.push(ReferenceLink {
                            label: label.as_str().to_string(),
                            normalized_label: crate::normalize_label(label.as_str()),
                            range: TextRange::new(
                                byte_offset + whole.start(),
                                byte_offset + whole.end(),
                            ),
                            label_range: TextRange::new(
                                byte_offset + label.start(),
                                byte_offset + label.end(),
                            ),
                        });
                    }
                }
            }
        }

        let footnotes_on = self.extensions.contains(Extension::Footnotes);
        if !is_footnote_definition && footnotes_on {
            for captures in FOOTNOTE_REF_RE.captures_iter(&masked) {
                let whole = captures.get(0).unwrap();
                let label = captures.get(1).unwrap();
                self.push_semantic_token(
                    SemanticTokenKind::Footnote,
                    TextRange::new(byte_offset + whole.start(), byte_offset + whole.end()),
                );
                self.output
                    .analysis
                    .footnote_references
                    .push(FootnoteReference {
                        label: label.as_str().to_string(),
                        normalized_label: crate::normalize_label(label.as_str()),
                        range: TextRange::new(
                            byte_offset + whole.start(),
                            byte_offset + whole.end(),
                        ),
                        label_range: TextRange::new(
                            byte_offset + label.start(),
                            byte_offset + label.end(),
                        ),
                    });
            }
        } else if !is_footnote_definition && self.options.disabled_extensions {
            for captures in FOOTNOTE_REF_RE.captures_iter(&masked) {
                let whole = captures.get(0).unwrap();
                let range = TextRange::new(byte_offset + whole.start(), byte_offset + whole.end());
                self.push_disabled_diagnostic(
                    Extension::Footnotes,
                    range,
                    Severity::Warning,
                    "footnotes are disabled",
                );
            }
        }

        let inline_notes_on = self.extensions.contains(Extension::InlineNotes);
        for captures in INLINE_NOTE_RE.captures_iter(&masked) {
            let whole = captures.get(0).unwrap();
            let content = captures.get(1).unwrap();
            let range = TextRange::new(byte_offset + whole.start(), byte_offset + whole.end());
            if inline_notes_on {
                self.push_semantic_token(SemanticTokenKind::Footnote, range);
                self.output.analysis.inline_notes.push(InlineNote {
                    range,
                    content: content.as_str().to_string(),
                });
            } else if self.options.disabled_extensions {
                self.push_disabled_diagnostic(
                    Extension::InlineNotes,
                    range,
                    Severity::Warning,
                    "inline notes are disabled",
                );
            }
        }

        for captures in HEADING_LINK_RE.captures_iter(&masked) {
            let whole = captures.get(0).unwrap();
            let anchor = captures.get(1).unwrap();
            self.output.analysis.heading_links.push(HeadingLink {
                anchor: anchor.as_str().to_string(),
                range: TextRange::new(byte_offset + whole.start(), byte_offset + whole.end()),
                anchor_range: TextRange::new(
                    byte_offset + anchor.start(),
                    byte_offset + anchor.end(),
                ),
            });
        }

        let citations_on = self.extensions.contains(Extension::Citations);
        for captures in CITATION_RE.captures_iter(&masked) {
            let sigil = captures.get(2).unwrap();
            let key = captures.get(3).unwrap();
            let key_range = TextRange::new(byte_offset + key.start(), byte_offset + key.end());
            if citations_on {
                self.push_semantic_token(
                    SemanticTokenKind::Citation,
                    TextRange::new(byte_offset + sigil.start(), byte_offset + key.end()),
                );
                self.output.analysis.citations.push(Citation {
                    key: key.as_str().to_string(),
                    range: TextRange::new(byte_offset + sigil.start(), byte_offset + key.end()),
                    key_range,
                });
            } else if self.options.disabled_extensions {
                self.push_disabled_diagnostic(
                    Extension::Citations,
                    key_range,
                    Severity::Warning,
                    "citations are disabled",
                );
            }
        }

        if self.extensions.contains(Extension::TexMathDollars) {
            for regex in [DISPLAY_MATH_RE.clone(), INLINE_MATH_RE.clone()] {
                for captures in regex.captures_iter(&masked) {
                    let whole = captures.get(0).unwrap();
                    self.push_semantic_token(
                        SemanticTokenKind::Math,
                        TextRange::new(byte_offset + whole.start(), byte_offset + whole.end()),
                    );
                }
            }
        }

        self.scan_braced_attribute_references(line, byte_offset);
    }

    fn scan_braced_attribute_references(&mut self, line: &str, byte_offset: usize) {
        if HEADING_RE.is_match(line) || FENCED_DIV_RE.is_match(line) {
            return;
        }

        let bracketed_spans_on = self.extensions.contains(Extension::BracketedSpans);
        for attributes in braced_attribute_sets(line, byte_offset) {
            let attr_start = attributes.whole_range.start.saturating_sub(byte_offset);
            let is_span = line.get(..attr_start).is_some_and(|prefix| {
                prefix.trim_end().ends_with(']') || IMAGE_ATTR_PREFIX_RE.is_match(prefix)
            });
            if is_span && !bracketed_spans_on {
                if self.options.disabled_extensions {
                    self.push_disabled_diagnostic(
                        Extension::BracketedSpans,
                        attributes.whole_range,
                        Severity::Warning,
                        "bracketed spans are disabled",
                    );
                }
                continue;
            }

            let Some(id) = attributes.id else {
                continue;
            };
            let Some(id_range) = attributes.id_range else {
                continue;
            };
            let classes = attributes
                .classes
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>();
            self.output.analysis.local_references.push(LocalReference {
                id: id.clone(),
                detail: braced_attribute_reference_detail(line, attr_start, &id, &classes),
                range: attributes.whole_range,
                id_range,
            });
        }
    }

    // ---- Links -----------------------------------------------------------

    fn scan_links(&mut self, line: &str, byte_offset: usize) {
        let masked = mask_inline_code(line);

        for captures in INLINE_LINK_RE.captures_iter(&masked) {
            let whole = captures.get(0).unwrap();
            let bang = captures.get(1).unwrap();
            let url = captures.get(3).unwrap();
            let target = url.as_str().trim_matches(|ch| ch == '<' || ch == '>');
            let kind = if bang.as_str().is_empty() {
                LinkKind::Inline
            } else {
                LinkKind::Image
            };
            let url_offset = if url.as_str().starts_with('<') {
                url.start() + 1
            } else {
                url.start()
            };
            self.output.analysis.links.push(MarkdownLink {
                kind,
                target: target.to_string(),
                label: captures.get(2).map(|label| label.as_str().to_string()),
                range: TextRange::new(byte_offset + whole.start(), byte_offset + whole.end()),
                target_range: TextRange::new(
                    byte_offset + url_offset,
                    byte_offset + url_offset + target.len(),
                ),
            });
            self.push_semantic_token(
                SemanticTokenKind::Link,
                TextRange::new(byte_offset + whole.start(), byte_offset + whole.end()),
            );
        }

        for captures in AUTOLINK_RE.captures_iter(&masked) {
            let whole = captures.get(0).unwrap();
            let url = captures.get(1).unwrap();
            self.output.analysis.links.push(MarkdownLink {
                kind: LinkKind::Autolink,
                target: url.as_str().to_string(),
                label: Some(url.as_str().to_string()),
                range: TextRange::new(byte_offset + whole.start(), byte_offset + whole.end()),
                target_range: TextRange::new(byte_offset + url.start(), byte_offset + url.end()),
            });
        }
    }

    // ---- Fenced divs ------------------------------------------------------

    fn scan_div_line(&mut self, line: &str, byte_offset: usize) {
        let Some(captures) = FENCED_DIV_RE.captures(line) else {
            return;
        };

        if !self.extensions.contains(Extension::FencedDivs) {
            if self.options.disabled_extensions {
                let fence = captures.get(0).unwrap();
                self.push_disabled_diagnostic(
                    Extension::FencedDivs,
                    TextRange::new(byte_offset, byte_offset + fence.end()),
                    Severity::Warning,
                    "fenced divs are disabled",
                );
            }
            return;
        }

        let fence = captures.get(1).unwrap();
        let rest = captures.get(2).unwrap();
        let fence_len = fence.as_str().len();
        let line_range = TextRange::new(byte_offset, byte_offset + line.len());

        if rest.as_str().trim().is_empty() {
            self.close_fenced_div(fence_len, line_range);
            return;
        }

        let parsed = parse_div_attributes(rest.as_str(), byte_offset + rest.start(), line_range);
        self.output.analysis.diagnostics.extend(parsed.diagnostics);
        let selection_range = parsed
            .id_range
            .or_else(|| parsed.class_ranges.first().copied())
            .unwrap_or(parsed.selection_range);

        let index = self.output.analysis.fenced_divs.len();
        self.output.analysis.fenced_divs.push(FencedDiv {
            id: parsed.id,
            classes: parsed.classes,
            attributes: parsed.attributes,
            caption: parsed.caption,
            fence_len,
            range: line_range,
            opening_range: line_range,
            closing_range: None,
            selection_range,
            id_range: parsed.id_range,
        });
        if let Some(div) = self.output.analysis.fenced_divs.get(index) {
            if let Some(id) = &div.id {
                let detail = fenced_div_reference_detail(div);
                self.output.analysis.local_references.push(LocalReference {
                    id: id.clone(),
                    detail,
                    range: div.opening_range,
                    id_range: div.id_range.unwrap_or(div.selection_range),
                });
            }
        }
        self.push_semantic_token(SemanticTokenKind::FencedDiv, line_range);
        self.div_stack.push(OpenDiv {
            index,
            fence_len,
            opening_range: line_range,
        });
    }

    fn close_fenced_div(&mut self, fence_len: usize, closing_range: TextRange) {
        let Some(open) = self.div_stack.last() else {
            self.output.analysis.diagnostics.push(Diagnostic {
                range: closing_range,
                severity: Severity::Warning,
                code: "unmatched-fenced-div-close",
                message: "fenced div closing fence has no matching opening fence".to_string(),
                extension: None,
            });
            return;
        };

        if fence_len < open.fence_len {
            self.output.analysis.diagnostics.push(Diagnostic {
                range: closing_range,
                severity: Severity::Warning,
                code: "short-fenced-div-close",
                message: format!(
                    "fenced div closing fence needs at least {} colons",
                    open.fence_len
                ),
                extension: None,
            });
            return;
        }

        let open = self.div_stack.pop().unwrap();
        if let Some(div) = self.output.analysis.fenced_divs.get_mut(open.index) {
            div.range = TextRange::new(open.opening_range.start, closing_range.end);
            div.closing_range = Some(closing_range);
        }
        self.push_semantic_token(SemanticTokenKind::FencedDiv, closing_range);
    }

    fn finish_unclosed_fenced_divs(&mut self, document_len: usize) {
        let mut unclosed = Vec::new();
        for open in self.div_stack.drain(..).rev() {
            if let Some(div) = self.output.analysis.fenced_divs.get_mut(open.index) {
                div.range = TextRange::new(open.opening_range.start, document_len);
            }
            unclosed.push(open);
        }
        for open in unclosed {
            self.output.analysis.diagnostics.push(Diagnostic {
                range: open.opening_range,
                severity: Severity::Warning,
                code: "unclosed-fenced-div",
                message: "fenced div has no closing fence".to_string(),
                extension: None,
            });
        }
    }

    // ---- Disabled extension usage ---------------------------------------

    fn scan_disabled_inline(&mut self, line: &str, byte_offset: usize) {
        if !self.options.disabled_extensions {
            return;
        }
        let masked = mask_inline_code(line);

        macro_rules! when_disabled {
            ($extension:expr, $severity:expr, $regex:expr, $message:expr) => {
                if !self.extensions.contains($extension) {
                    for captures in $regex.captures_iter(&masked) {
                        let whole = captures.get(0).unwrap();
                        self.push_disabled_diagnostic(
                            $extension,
                            TextRange::new(byte_offset + whole.start(), byte_offset + whole.end()),
                            $severity,
                            $message,
                        );
                    }
                }
            };
        }

        when_disabled!(
            Extension::Strikeout,
            Severity::Warning,
            STRIKEOUT_RE,
            "strikeout is disabled"
        );
        when_disabled!(
            Extension::Superscript,
            Severity::Hint,
            SUPERSCRIPT_RE,
            "superscript is disabled"
        );
        // Mark: `==text==`. Skip runs that are part of table borders
        // (`+=====+`) or that touch other `=`/`+` characters.
        if !self.extensions.contains(Extension::Mark) {
            for captures in MARK_RE.captures_iter(&masked) {
                let whole = captures.get(0).unwrap();
                let before = &masked[..whole.start()];
                let after = &masked[whole.end()..];
                if before.ends_with(['=', '+']) || after.starts_with(['=', '+']) {
                    continue;
                }
                self.push_disabled_diagnostic(
                    Extension::Mark,
                    TextRange::new(byte_offset + whole.start(), byte_offset + whole.end()),
                    Severity::Hint,
                    "mark (highlight) is disabled",
                );
            }
        }
        when_disabled!(
            Extension::TexMathDollars,
            Severity::Hint,
            DISPLAY_MATH_RE,
            "TeX math with $$..$$ is disabled"
        );
        // Emoji shortcodes: standalone ":name:" only, not "12:30:45".
        if !self.extensions.contains(Extension::Emoji) {
            for captures in EMOJI_RE.captures_iter(&masked) {
                let whole = captures.get(0).unwrap();
                // Standalone `:name:` shortcodes only: neighbors must not be
                // alphanumeric (times like 12:30) nor table/path delimiters
                // (pipe-table alignment rows like `|:---:|`).
                let before_ok = masked
                    .get(..whole.start())
                    .and_then(|prefix| prefix.chars().next_back())
                    .is_none_or(|ch| !ch.is_alphanumeric() && !matches!(ch, '|' | '/' | ':'));
                let after_ok = masked
                    .get(whole.end()..)
                    .and_then(|suffix| suffix.chars().next())
                    .is_none_or(|ch| !ch.is_alphanumeric() && !matches!(ch, '|' | '/' | ':'));
                if before_ok && after_ok {
                    self.push_disabled_diagnostic(
                        Extension::Emoji,
                        TextRange::new(byte_offset + whole.start(), byte_offset + whole.end()),
                        Severity::Hint,
                        "emoji shortcodes are disabled",
                    );
                }
            }
        }
        when_disabled!(
            Extension::ExampleLists,
            Severity::Warning,
            EXAMPLE_LIST_RE,
            "example lists are disabled"
        );
        when_disabled!(
            Extension::WikilinksTitleAfterPipe,
            Severity::Hint,
            WIKILINK_RE,
            "wikilinks are disabled"
        );
        if !self.extensions.contains(Extension::RawAttribute) {
            for captures in RAW_ATTR_RE.captures_iter(line) {
                let whole = captures.get(0).unwrap();
                self.push_disabled_diagnostic(
                    Extension::RawAttribute,
                    TextRange::new(byte_offset + whole.start(), byte_offset + whole.end()),
                    Severity::Warning,
                    "raw attribute spans are disabled",
                );
            }
        }

        // Subscript: skip runs that are part of ~~ (strikeout markers).
        if !self.extensions.contains(Extension::Subscript) {
            for captures in SUBSCRIPT_RE.captures_iter(&masked) {
                let whole = captures.get(0).unwrap();
                let before = &masked[..whole.start()];
                let after = &masked[whole.end()..];
                if before.ends_with('~') || after.starts_with('~') {
                    continue;
                }
                self.push_disabled_diagnostic(
                    Extension::Subscript,
                    TextRange::new(byte_offset + whole.start(), byte_offset + whole.end()),
                    Severity::Hint,
                    "subscript is disabled",
                );
            }
        }

        // Inline math with single dollars: only flag tight, math-looking
        // spans to avoid flagging currency like "$5 and $6".
        if !self.extensions.contains(Extension::TexMathDollars) {
            for captures in INLINE_MATH_RE.captures_iter(&masked) {
                let whole = captures.get(0).unwrap();
                let content = captures.get(1).unwrap().as_str();
                let mathish = if content.contains(' ') {
                    // Spaced content must contain strong math markers; a
                    // lone backslash also occurs in prose.
                    content.contains(['^', '_', '{', '}'])
                } else {
                    !content.contains('`')
                };
                let currency_like = content
                    .chars()
                    .all(|ch| ch.is_ascii_digit() || ch == ',' || ch == '.');
                if mathish && !currency_like {
                    self.push_disabled_diagnostic(
                        Extension::TexMathDollars,
                        TextRange::new(byte_offset + whole.start(), byte_offset + whole.end()),
                        Severity::Hint,
                        "TeX math with $..$ is disabled",
                    );
                }
            }
        }

        // Backslash math: \(..\)/\[..\] single or double.
        if !self.extensions.contains(Extension::TexMathSingleBackslash)
            && !self.extensions.contains(Extension::TexMathDoubleBackslash)
        {
            for captures in MATH_BACKSLASH_RE.captures_iter(&masked) {
                let whole = captures.get(0).unwrap();
                self.push_disabled_diagnostic(
                    Extension::TexMathSingleBackslash,
                    TextRange::new(byte_offset + whole.start(), byte_offset + whole.end()),
                    Severity::Hint,
                    "TeX math with \\(..\\) is disabled",
                );
            }
        }
        // GFM math contains backticks, so scan the raw line (the masked
        // line would hide it).
        if !self.extensions.contains(Extension::TexMathGfm) {
            for captures in MATH_GFM_RE.captures_iter(line) {
                let whole = captures.get(0).unwrap();
                self.push_disabled_diagnostic(
                    Extension::TexMathGfm,
                    TextRange::new(byte_offset + whole.start(), byte_offset + whole.end()),
                    Severity::Hint,
                    "GFM math with $`..`$ is disabled",
                );
            }
        }

        // Task list items.
        if !self.extensions.contains(Extension::TaskLists) {
            if let Some(captures) = TASK_LIST_RE.captures(line) {
                let whole = captures.get(0).unwrap();
                self.push_disabled_diagnostic(
                    Extension::TaskLists,
                    TextRange::new(byte_offset, byte_offset + whole.end()),
                    Severity::Warning,
                    "task lists are disabled",
                );
            }
        }

        // Alerts: > [!NOTE]
        if !self.extensions.contains(Extension::Alerts) {
            if let Some(captures) = ALERT_RE.captures(line) {
                let whole = captures.get(0).unwrap();
                self.push_disabled_diagnostic(
                    Extension::Alerts,
                    TextRange::new(byte_offset, byte_offset + whole.end()),
                    Severity::Warning,
                    "alerts are disabled",
                );
            }
        }

        // Pipe and grid tables.
        if !self.extensions.contains(Extension::PipeTables)
            && line.contains('|')
            && PIPE_TABLE_RE.is_match(line)
            && line.trim().len() >= 5
        {
            self.push_disabled_diagnostic(
                Extension::PipeTables,
                TextRange::new(byte_offset, byte_offset + line.len()),
                Severity::Warning,
                "pipe tables are disabled",
            );
        }
        if !self.extensions.contains(Extension::GridTables) && GRID_TABLE_RE.is_match(line) {
            self.push_disabled_diagnostic(
                Extension::GridTables,
                TextRange::new(byte_offset, byte_offset + line.len()),
                Severity::Warning,
                "grid tables are disabled",
            );
        }

        // Definition lists: `: definition` lines directly after text.
        if !self.extensions.contains(Extension::DefinitionLists)
            && self.previous_was_paragraph
            && DEF_LIST_RE.is_match(line)
        {
            self.push_disabled_diagnostic(
                Extension::DefinitionLists,
                TextRange::new(
                    byte_offset,
                    byte_offset + line.len().min(line.trim_end().len()),
                ),
                Severity::Hint,
                "definition lists are disabled",
            );
        }
    }

    fn push_disabled_diagnostic(
        &mut self,
        extension: Extension,
        range: TextRange,
        severity: Severity,
        summary: &str,
    ) {
        self.output.analysis.diagnostics.push(Diagnostic {
            range,
            severity,
            code: "extension-disabled",
            message: format!(
                "{summary}; the `{}` extension is disabled (enable it via pandocmd.extensions)",
                extension.name()
            ),
            extension: Some(extension.name()),
        });
    }

    fn push_semantic_token(&mut self, kind: SemanticTokenKind, range: TextRange) {
        self.output
            .analysis
            .semantic_tokens
            .push(SemanticToken { kind, range });
    }
}

/// Detect a fenced code block marker line (``` or ~~~ with length >= 3).
fn fence_marker(line: &str) -> Option<(char, usize)> {
    let trimmed = line.trim_start_matches([' ', '\t']);
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let delimiter = trimmed.chars().next()?;
    if !matches!(delimiter, '`' | '~') {
        return None;
    }
    let length = trimmed.chars().take_while(|ch| *ch == delimiter).count();
    (length >= 3).then_some((delimiter, length))
}

/// Replace inline code spans (`...`, ``...``) with spaces so that inline
/// regexes do not match inside them. Byte length is preserved.
fn mask_inline_code(line: &str) -> String {
    if !line.contains('`') {
        return line.to_string();
    }

    let bytes = line.as_bytes();
    let mut masked = bytes.to_vec();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'`' {
            let run_start = index;
            while index < bytes.len() && bytes[index] == b'`' {
                index += 1;
            }
            let run_len = index - run_start;
            if run_len > 2 {
                continue;
            }
            // Find a closing run of the same length.
            let mut scan = index;
            let mut close = None;
            while scan < bytes.len() {
                if bytes[scan] == b'`' {
                    let mut close_run = 0;
                    let mut cursor = scan;
                    while cursor < bytes.len() && bytes[cursor] == b'`' {
                        close_run += 1;
                        cursor += 1;
                    }
                    if close_run == run_len {
                        close = Some(scan);
                        break;
                    }
                    scan = cursor;
                } else {
                    scan += 1;
                }
            }
            if let Some(close) = close {
                for byte in &mut masked[run_start..close + run_len] {
                    if *byte != b' ' {
                        *byte = b' ';
                    }
                }
                index = close + run_len;
            }
        } else {
            index += 1;
        }
    }

    // Only backtick bytes were replaced with spaces, so this is valid UTF-8.
    String::from_utf8(masked).expect("masked line is valid UTF-8")
}

#[derive(Debug)]
struct BracedAttributeSet {
    whole_range: TextRange,
    id: Option<String>,
    id_range: Option<TextRange>,
    classes: Vec<String>,
}

fn braced_attribute_sets(line: &str, byte_offset: usize) -> Vec<BracedAttributeSet> {
    let mut attributes = Vec::new();
    for captures in BRACED_ATTR_RE.captures_iter(line) {
        let Some(whole) = captures.get(0) else {
            continue;
        };
        let inner = captures.get(1).unwrap();
        let tokens = tokenize_attributes(inner.as_str(), byte_offset + inner.start());
        let mut id = None;
        let mut id_range = None;
        let mut classes = Vec::new();

        for token in tokens {
            if let Some(token_id) = token.text.strip_prefix('#') {
                if !token_id.is_empty() {
                    id = Some(token_id.to_string());
                    id_range = Some(TextRange::new(token.range.start + 1, token.range.end));
                }
            } else if let Some(class) = token.text.strip_prefix('.') {
                if !class.is_empty() {
                    classes.push(class.to_string());
                }
            }
        }

        attributes.push(BracedAttributeSet {
            whole_range: TextRange::new(byte_offset + whole.start(), byte_offset + whole.end()),
            id,
            id_range,
            classes,
        });
    }

    attributes
}

#[derive(Debug)]
struct AttrToken<'a> {
    text: &'a str,
    range: TextRange,
}

#[derive(Debug)]
struct ParsedDivAttributes {
    id: Option<String>,
    id_range: Option<TextRange>,
    classes: Vec<String>,
    class_ranges: Vec<TextRange>,
    attributes: Vec<DivAttribute>,
    caption: Option<String>,
    selection_range: TextRange,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Debug)]
struct DivAttributeText<'a> {
    text: &'a str,
    offset: usize,
    range: TextRange,
    trailing_caption: Option<&'a str>,
}

fn parse_div_attributes(
    rest: &str,
    rest_offset: usize,
    fallback_range: TextRange,
) -> ParsedDivAttributes {
    let attributes = trim_div_attribute_text(rest, rest_offset);
    let mut parsed = ParsedDivAttributes {
        id: None,
        id_range: None,
        classes: Vec::new(),
        class_ranges: Vec::new(),
        attributes: Vec::new(),
        caption: attributes.trailing_caption.map(str::to_string),
        selection_range: attributes.range,
        diagnostics: Vec::new(),
    };

    if attributes.text.is_empty() {
        parsed.selection_range = fallback_range;
        parsed.diagnostics.push(Diagnostic {
            range: fallback_range,
            severity: Severity::Warning,
            code: "missing-fenced-div-attributes",
            message: "fenced div opening fence should include attributes or a class name"
                .to_string(),
            extension: None,
        });
        return parsed;
    }

    if attributes.text.starts_with('{') {
        if !attributes.text.ends_with('}') {
            parsed.diagnostics.push(Diagnostic {
                range: attributes.range,
                severity: Severity::Warning,
                code: "malformed-fenced-div-attributes",
                message: "fenced div attributes should be enclosed with `{` and `}`".to_string(),
                extension: None,
            });
        }

        let inner_start = usize::from(attributes.text.starts_with('{'));
        let inner_end = if attributes.text.ends_with('}') {
            attributes.text.len().saturating_sub(1)
        } else {
            attributes.text.len()
        };
        let inner = &attributes.text[inner_start..inner_end];
        parse_braced_div_attributes(inner, attributes.offset + inner_start, &mut parsed);
    } else {
        parse_unbraced_div_attributes(attributes.text, attributes.range, &mut parsed);
    }

    parsed
}

fn trim_div_attribute_text(rest: &str, rest_offset: usize) -> DivAttributeText<'_> {
    let leading = rest.len() - rest.trim_start().len();
    let mut end = rest.trim_end().len();
    let mut text = &rest[leading..end];

    if let Some(trailing_start) = trailing_colon_fence_start(text) {
        end = leading + text[..trailing_start].trim_end().len();
        text = &rest[leading..end];
    }

    let attr_offset = rest_offset + leading;
    let mut attr_text = text;
    let mut trailing_caption = None;

    if text.starts_with('{') {
        if let Some(closing_brace) = matching_attribute_close_brace(text) {
            let attr_end = closing_brace + 1;
            attr_text = &text[..attr_end];
            let caption = text[attr_end..].trim();
            if !caption.is_empty() {
                trailing_caption = Some(caption);
            }
        }
    }

    DivAttributeText {
        text: attr_text,
        offset: attr_offset,
        range: TextRange::new(attr_offset, attr_offset + attr_text.len()),
        trailing_caption,
    }
}

fn trailing_colon_fence_start(text: &str) -> Option<usize> {
    let trimmed = text.trim_end();
    let token_start = trimmed.rfind(char::is_whitespace).map(|index| index + 1)?;
    let token = &trimmed[token_start..];
    (token.len() >= 3 && token.chars().all(|ch| ch == ':')).then_some(token_start)
}

fn matching_attribute_close_brace(text: &str) -> Option<usize> {
    debug_assert!(text.starts_with('{'));

    let mut quote = None;
    let mut escaped = false;

    for (index, ch) in text.char_indices().skip(1) {
        if escaped {
            escaped = false;
            continue;
        }

        if ch == '\\' {
            escaped = true;
            continue;
        }

        if let Some(active_quote) = quote {
            if ch == active_quote {
                quote = None;
            }
            continue;
        }

        if ch == '"' || ch == '\'' {
            quote = Some(ch);
        } else if ch == '}' {
            return Some(index);
        }
    }

    None
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
                    extension: None,
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
                    extension: None,
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
                    extension: None,
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
            extension: None,
        });
    }
}

fn tokenize_attributes(input: &str, offset: usize) -> Vec<AttrToken<'_>> {
    let mut tokens = Vec::new();
    let mut token_start = None;
    let mut quote = None;
    let mut escaped = false;

    for (index, ch) in input.char_indices() {
        if token_start.is_none() {
            if ch.is_whitespace() {
                continue;
            }
            token_start = Some(index);
        }

        if escaped {
            escaped = false;
            continue;
        }

        if ch == '\\' {
            escaped = true;
            continue;
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

fn fenced_div_reference_detail(div: &FencedDiv) -> String {
    let kind = div
        .classes
        .first()
        .map(String::as_str)
        .unwrap_or("fenced div");

    if let Some(title) = div.title() {
        format!("{kind}: {title}")
    } else {
        kind.to_string()
    }
}

fn braced_attribute_reference_detail(
    line: &str,
    attr_start: usize,
    id: &str,
    classes: &[&str],
) -> String {
    if let Some(detail) = reference_type_from_id(id) {
        return detail.to_string();
    }
    let prefix = line.get(..attr_start).unwrap_or("").trim_end();
    if IMAGE_ATTR_PREFIX_RE.is_match(prefix) {
        return "figure".to_string();
    }
    if CODE_FENCE_ATTR_PREFIX_RE.is_match(prefix) {
        return "listing".to_string();
    }
    classes
        .first()
        .map(|class| (*class).to_string())
        .unwrap_or_else(|| "local Pandoc reference".to_string())
}

fn reference_type_from_id(id: &str) -> Option<&'static str> {
    let prefix = id
        .split(['-', ':', '_', '.'])
        .next()
        .unwrap_or(id)
        .to_ascii_lowercase();

    match prefix.as_str() {
        "sec" | "section" => Some("section"),
        "fig" | "figure" => Some("figure"),
        "tbl" | "tab" | "table" => Some("table"),
        "eq" | "equation" => Some("equation"),
        "lst" | "listing" => Some("listing"),
        "fn" | "footnote" => Some("footnote"),
        "thm" | "theorem" => Some("theorem"),
        "lem" | "lemma" => Some("lemma"),
        "def" | "definition" => Some("definition"),
        "cor" | "corollary" => Some("corollary"),
        "prop" | "proposition" => Some("proposition"),
        "ex" | "example" => Some("example"),
        "span" => Some("span"),
        _ => None,
    }
}

fn trim_ascii_whitespace_end(line: &str, start: usize, mut end: usize) -> usize {
    while end > start && line.as_bytes()[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    end
}
