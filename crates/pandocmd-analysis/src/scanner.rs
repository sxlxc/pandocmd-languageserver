//! Line-oriented scanning of Pandoc Markdown documents.
//!
//! The scanner is extension-aware: every construct is only recognized when
//! the corresponding Pandoc extension is enabled, constructs that are used
//! while their extension is disabled produce `extension-disabled`
//! diagnostics, and scanning never runs inside fenced code blocks or the
//! YAML metadata block.

use std::collections::{HashMap, HashSet};
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
    LazyLock::new(|| Regex::new(r"^[ \t]{0,3}(?:>[ \t]*)*(#{1,6})[ \t]+(.+?)[ \t#]*$").unwrap());
static SETEXT_UNDERLINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[ \t]{0,3}(=+|-+)[ \t]*$").unwrap());
/// A line made only of hyphens and spaces (at least three hyphens): a
/// possible simple/multiline table rule. Two such lines with table content
/// between them activate multiline-table mode, in which hyphen rules are row
/// separators instead of setext underlines (matching pandoc).
static TABLE_RULE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[ \t]{0,3}(?:-[ \t]*){3,}$").unwrap());
/// Definition-list definition marker (`:`, `::`, or `~` followed by space).
static DEF_LIST_MARKER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[ \t]*(:{1,2}|~)[ \t]+([^ \t]*)").unwrap());
/// A reference-definition continuation line that carries a trailing link
/// title: `<target> "title"` / `<target> 'title'` / `<target> (title)`.
static REF_DEF_CONTINUATION_TITLE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^[ \t]*(.*?)[ \t]+("[^"\n]*"|'[^'\n]*'|\([^)\n]*\))[ \t]*$"#).unwrap()
});
static REF_DEF_TITLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^[ \t]*("[^"]*"|'[^']*')[ \t]*$"#).unwrap());
static FOOTNOTE_DEF_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[ \t]*\[\^([^\]\n]+)\]:").unwrap());
static REF_DEF_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^[ \t]*\[([^\]\n]+)\]:[ \t]*(<[^>\n]*>|(?:[^\s\n][^\n]*?)?)(?:[ \t]+("[^"\n]*"|\([^)\n]*\)|'[^'\n]*'))?[ \t]*$"#).unwrap()
});
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
    LazyLock::new(|| Regex::new(r"^[ \t]*(?:>[ \t]*)*(:{3,})(.*)$").unwrap());
static BRACED_ATTR_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{([^}\n]*)\}").unwrap());
static IMAGE_ATTR_PREFIX_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"!\[[^\]\n]*\]\([^\)\n]*\)\s*$").unwrap());
static CODE_FENCE_ATTR_PREFIX_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[ \t]{0,3}(`{3,}|~{3,})\s*$").unwrap());
static INLINE_LINK_RE: LazyLock<Regex> = LazyLock::new(|| {
    // Link text may contain one level of nested brackets (notably
    // `[![alt](src)](target)` badge links); the destination itself may not
    // contain unbalanced parentheses.
    Regex::new(r#"(!?)\[([^\[\]\n]*(?:\[[^\[\]\n]*\][^\[\]\n]*)*(?:\[[^\]\n]*)?)\]\([ \t]*(<[^>\n]*>|(?:\\[()]|[^()\s])*(?:\((?:\\[()]|[^()\s\\])*\)(?:\\[()]|[^()\s])*)*|(?:[^()\n\\]|\\.)*?)(?:[ \t]+("[^"\n]*"|\([^)\n]*\)|'[^'\n]*'))?[ \t]*\)"#).unwrap()
});
static AUTOLINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<([a-zA-Z][a-zA-Z0-9+.-]{1,31}:[^<>\s]+)>").unwrap());
static EMAIL_AUTOLINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<([^<>\s@:]+@[^<>\s@]+)>").unwrap());
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

    // Example-list labels are collected in a pre-pass: pandoc resolves
    // `(@label)` references against the complete set, even when the marker
    // appears later in the document.
    for line in text.lines() {
        if let Some(label) = example_list_label(line) {
            scan.example_labels.insert(label);
        }
    }

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
    /// True when the previous line was blank (or at document start): a
    /// deeply-indented non-blank line then starts an indented code block,
    /// like in pandoc.
    after_blank: bool,
    /// Inside an indented code block; lines are skipped until a non-blank
    /// line with insufficient indentation arrives.
    in_indented_code: bool,
    /// Content columns of the currently open (nested) definition lists,
    /// used to distinguish list content from indented code.
    def_list_stack: Vec<usize>,
    /// Content column of the currently open bullet/ordered list.
    list_column: Option<usize>,
    /// A reference definition `[label]:` seen with an empty target; pandoc
    /// takes the entire next non-blank line as its destination.
    pending_ref_def: Option<PendingRefDef>,
    /// The pending reference definition got its target; a following line may
    /// still be a quoted link title.
    pending_ref_title: bool,
    /// Non-blank, non-rule lines since the last table-rule-like line, when a
    /// rule was seen recently (`Some(gap)` with `gap <= RULE_GAP_MAX`).
    table_rule_gap: Option<usize>,
    /// Between the rules of a simple/multiline table: hyphen rules are row
    /// separators, never setext underlines.
    in_multiline_table: bool,
    table_rule_just_seen: bool,
    /// Text of an unclosed `[` at the end of the previous line, plus its
    /// absolute offset: inline link text may wrap onto the next line. Both
    /// the fully masked variant (for links, citations) and the raw variant
    /// (reference labels keep formatting characters) are carried.
    link_carry: Option<(usize, String, String)>,
    /// Length of an unclosed backtick run at the end of the previous line:
    /// inline code spans may wrap across lines.
    code_span_carry: Option<usize>,
    /// Inside a grid table (`+---+` rules, `| ... |` rows): row cells get
    /// block-aware treatment (headings, code cells).
    in_grid_table: bool,
    /// Labels of example-list markers (`(@label) text`) seen so far, used to
    /// distinguish `(@label)` example references from citations.
    example_labels: HashSet<String>,
    /// A setext underline seen with a pending paragraph that may still turn
    /// out to be a table rule instead; committed on the next line.
    held_setext: Option<HeldSetext>,
}

/// A setext heading held for one line to disambiguate it from the first
/// rule of a table header.
struct HeldSetext {
    level: u8,
    document_end: usize,
    paragraph: Vec<(usize, String)>,
}

/// A `[label]:` definition still waiting for its target line.
struct PendingRefDef {
    label: String,
    normalized_label: String,
    label_range: pandocmd_syntax::TextRange,
}

/// Maximum non-blank lines between two table-rule-like lines for the second
/// rule to be treated as the end of a table header.
const RULE_GAP_MAX: usize = 4;
/// Maximum bytes of carried link text across line wraps.
const LINK_CARRY_MAX: usize = 1000;

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
            after_blank: true,
            in_indented_code: false,
            def_list_stack: Vec::new(),
            list_column: None,
            pending_ref_def: None,
            pending_ref_title: false,
            table_rule_gap: None,
            in_multiline_table: false,
            table_rule_just_seen: false,
            link_carry: None,
            code_span_carry: None,
            in_grid_table: false,
            example_labels: HashSet::new(),
            held_setext: None,
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

        let carried = self.link_carry.take();
        let code_carry = self.code_span_carry.take();

        if line.trim().is_empty() {
            self.after_blank = true;
            if self.in_multiline_table && self.table_rule_just_seen {
                // A table rule followed by a blank line ends the table.
                self.in_multiline_table = false;
            }
            self.table_rule_just_seen = false;
            // Blank lines end grid tables (pandoc parses a later `|` line
            // as a line block, not as another row).
            self.in_grid_table = false;
            // Table headers never contain blank lines between their rules.
            self.table_rule_gap = None;
            // A held setext underline commits: `Text\n-----\n\nMore` is a
            // heading followed by a paragraph in pandoc.
            if let Some(held) = self.held_setext.take() {
                self.pending_setext_level = Some(held.level);
                self.pending_paragraph = held.paragraph;
                self.push_setext_heading(held.document_end);
                self.pending_paragraph.clear();
            }
            self.previous_was_paragraph = false;
            self.pending_paragraph.clear();
            // A pending `[label]:` never finds its URL after a blank
            // line; pandoc closes it with an empty target.
            if let Some(pending) = self.pending_ref_def.take() {
                self.push_reference_definition(pending, String::new(), None);
            }
            self.pending_ref_title = false;
            return;
        }

        // Indented code blocks. Inside a definition or bullet list, content
        // indented to the list's content column is list content; four or
        // more columns beyond it (or beyond column 0 outside lists) after a
        // blank line starts indented code, like pandoc.
        let (quote_depth, content_indent) = line_geometry(line);
        let list_base = self
            .def_list_stack
            .last()
            .copied()
            .or(self.list_column)
            .unwrap_or(0);
        let code_threshold = list_base + 4 + 2 * quote_depth;
        if self.in_indented_code {
            if content_indent >= code_threshold {
                self.after_blank = false;
                return;
            }
            self.in_indented_code = false;
        } else if self.after_blank && content_indent >= code_threshold {
            self.in_indented_code = true;
            self.after_blank = false;
            return;
        }

        // A held setext underline commits unless this line reveals that it
        // was the opening rule of a table header instead.
        let mut held = self.held_setext.take();
        if TABLE_RULE_RE.is_match(line) || GRID_RULE_RE.is_match(line) {
            if let Some(held) = held.take() {
                self.table_rule_gap = Some(0);
                let _ = held;
            }
        } else if let Some(held) = held.take() {
            self.pending_setext_level = Some(held.level);
            self.pending_paragraph = held.paragraph.clone();
            self.push_setext_heading(held.document_end);
            self.pending_paragraph = held.paragraph;
            self.previous_was_paragraph = true;
        }

        // Track list content columns so that deeply-indented fences, divs,
        // and definitions inside list items are recognized as list content
        // rather than indented code.
        if quote_depth == 0 {
            // A definition marker only starts a (nested) definition when it
            // sits at the enclosing item's content column; deeper or
            // shallower markers are prose. A nested definition raises the
            // content column to the marker's text column.
            while self
                .def_list_stack
                .last()
                .is_some_and(|column| content_indent < *column)
            {
                self.def_list_stack.pop();
            }
            let active_column = self
                .def_list_stack
                .last()
                .copied()
                .or(self.list_column)
                .filter(|column| *column > 0);
            let marker_indent = line.len() - line.trim_start_matches([' ', '\t']).len();
            let marker_valid = match active_column {
                Some(column) => marker_indent == column,
                None => marker_indent <= 3,
            };
            if let (true, Some(column)) = (marker_valid, definition_list_content_column(line)) {
                self.def_list_stack.push(column);
            } else if let Some(column) = list_content_column(line) {
                // Nested bullet lists never raise the effective content
                // column beyond the outermost item's column.
                self.list_column = Some(
                    self.list_column
                        .map_or(column, |old: usize| old.min(column)),
                );
            } else if self
                .list_column
                .is_some_and(|column| content_indent < column)
            {
                self.list_column = None;
            }
        }

        // Example-list markers register their labels so that `(@label)`
        // references elsewhere are not mistaken for citations.
        if let Some(label) = example_list_label(line) {
            self.example_labels.insert(label);
        }

        // A reference definition waiting for its target consumes the entire
        // next non-blank line as its destination — before any other block
        // interpretation, so even a fence line becomes the target (matching
        // pandoc).
        if (self.pending_ref_def.is_some() || self.pending_ref_title)
            && self.resolve_pending_ref_def(line, byte_offset)
        {
            self.after_blank = false;
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
            self.after_blank = false;
            return;
        }

        // Grid tables: `+---+` rules start one, `| ... |` lines are rows
        // whose cells can contain block content (headings, code cells).
        if GRID_RULE_RE.is_match(line) {
            if !self.extensions.contains(Extension::GridTables) {
                if self.options.disabled_extensions {
                    self.push_disabled_diagnostic(
                        Extension::GridTables,
                        TextRange::new(byte_offset, byte_offset + line.len()),
                        Severity::Warning,
                        "grid tables are disabled",
                    );
                }
            } else {
                self.in_grid_table = true;
            }
            self.previous_was_paragraph = false;
            self.pending_paragraph.clear();
            self.after_blank = false;
            return;
        }
        let grid_row = self.in_grid_table && line.trim_start().starts_with('|');
        if self.in_grid_table && !grid_row {
            self.in_grid_table = false;
        }
        let mut masked = mask_inline_code_with_carry(line, code_carry, &mut self.code_span_carry);
        if grid_row {
            let mut row_masked = masked.clone().into_bytes();
            self.scan_grid_row_cells(line, byte_offset, &masked, &mut row_masked);
            masked = String::from_utf8(row_masked).expect("row mask is valid UTF-8");
        }
        // Inline scanners see the virtual text that joins any link-text
        // carry from the previous line, so wrapped reference links are found.
        let inline_text;
        let inline_raw;
        let inline_base;
        match &carried {
            Some((carry_offset, carry_strict, carry_raw)) => {
                let joiner = wrap_joiner(carry_offset + carry_strict.len(), byte_offset);
                inline_text = format!("{carry_strict}{joiner}{masked}");
                inline_raw = format!("{carry_raw}{joiner}{line}");
                inline_base = *carry_offset;
            }
            None => {
                inline_text = masked.clone();
                inline_raw = line.to_string();
                inline_base = byte_offset;
            }
        }

        // Simple/multiline table rules: two hyphen rules with (header)
        // content between them mean table mode, in which further rules are
        // row separators instead of setext underlines.
        if TABLE_RULE_RE.is_match(line) {
            if self.in_multiline_table {
                self.table_rule_just_seen = true;
                self.pending_paragraph.clear();
                self.previous_was_paragraph = false;
                self.after_blank = false;
                return;
            }
            if self.table_rule_gap.is_some_and(|gap| gap >= 1) {
                // Entering the table body: the header-end rule must not arm
                // the blank-line exit (bodies may contain blank lines).
                self.in_multiline_table = true;
                self.table_rule_just_seen = false;
                self.pending_paragraph.clear();
                self.previous_was_paragraph = false;
                self.table_rule_gap = None;
                self.after_blank = false;
                return;
            }
            self.table_rule_gap = Some(0);
        } else if self
            .table_rule_gap
            .is_some_and(|gap| gap + 1 > RULE_GAP_MAX)
        {
            self.table_rule_gap = None;
        } else if let Some(gap) = self.table_rule_gap {
            self.table_rule_gap = Some(gap + 1);
        }

        // Setext heading underline: `Heading text` + `======`. A hyphen
        // underline right after a hyphen rule (or inside a table) is a table
        // separator instead; hyphen underlines are held for one line so a
        // following rule can still claim them for a table.
        if self.previous_was_paragraph {
            if let Some(captures) = SETEXT_UNDERLINE_RE.captures(line) {
                let marker = captures.get(1).unwrap();
                let level = if marker.as_str().starts_with('=') {
                    1
                } else {
                    2
                };
                if level == 2 && !self.in_multiline_table {
                    // Could still be the first rule of a table header; hold
                    // for one line before committing the heading.
                    self.held_setext = Some(HeldSetext {
                        level: 2,
                        document_end: byte_offset + line.len(),
                        paragraph: std::mem::take(&mut self.pending_paragraph),
                    });
                    self.previous_was_paragraph = false;
                    self.after_blank = false;
                    return;
                }
                if self.in_multiline_table {
                    self.pending_paragraph.clear();
                    self.previous_was_paragraph = false;
                    self.after_blank = false;
                    return;
                }
                self.pending_setext_level = Some(level);
                self.table_rule_gap = None;
                self.push_setext_heading(byte_offset + line.len());
                self.after_blank = false;
                return;
            }
        }

        self.scan_div_line(line, byte_offset);
        self.scan_block_line(line, &masked, byte_offset);
        self.scan_inline_line(&inline_text, &inline_raw, line, byte_offset, inline_base);
        self.scan_disabled_inline(line, &masked, byte_offset);
        self.scan_links_with_carry(line, byte_offset, carried, &masked, line);

        self.previous_was_paragraph = !line.trim().is_empty()
            && !HEADING_RE.is_match(line)
            && !REF_DEF_RE.is_match(line)
            && !FOOTNOTE_DEF_RE.is_match(line);
        if self.previous_was_paragraph {
            self.pending_paragraph.push((byte_offset, line.to_string()));
        } else {
            self.pending_paragraph.clear();
        }
        self.after_blank = false;
    }

    /// Feed a pending `[label]:` definition with this line, if the line is
    /// its target (or target plus title). Returns true when the line was
    /// consumed as part of the definition.
    fn resolve_pending_ref_def(&mut self, line: &str, byte_offset: usize) -> bool {
        if let Some(pending) = self.pending_ref_def.take() {
            // Pandoc takes the entire next non-blank line as the
            // destination, splitting an optional trailing quoted or
            // parenthesized title (verified: `[foo]:` + `- list item`
            // defines target `- list item`).
            let (target, range, has_title) = continuation_target(line, byte_offset);
            self.push_reference_definition(pending, target, range);
            self.pending_ref_title = !has_title;
            return true;
        }
        if self.pending_ref_title {
            self.pending_ref_title = false;
            if REF_DEF_TITLE_RE.is_match(line) {
                return true;
            }
        }
        false
    }

    fn push_reference_definition(
        &mut self,
        pending: PendingRefDef,
        target: String,
        target_range: Option<TextRange>,
    ) {
        let PendingRefDef {
            label,
            normalized_label,
            label_range,
        } = pending;
        if !target.is_empty() {
            let target_range = target_range.unwrap_or(label_range);
            self.output.analysis.links.push(MarkdownLink {
                kind: LinkKind::Definition,
                target: target.clone(),
                label: Some(label.clone()),
                range: label_range,
                target_range,
            });
            self.push_semantic_token(SemanticTokenKind::Link, target_range);
        }
        self.output
            .analysis
            .reference_definitions
            .push(ReferenceDefinition {
                label,
                normalized_label,
                target,
                range: label_range,
                label_range,
            });
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
        if let Some(pending) = self.pending_ref_def.take() {
            // A `[label]:` at end of document never got its target line.
            self.push_reference_definition(pending, String::new(), None);
        }
        self.pending_ref_title = false;
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

    fn scan_block_line(&mut self, line: &str, masked: &str, byte_offset: usize) {
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
            if self.previous_was_paragraph {
                // Definitions cannot interrupt a paragraph in pandoc; this
                // line is a lazy paragraph continuation.
                return;
            }
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

        if self.previous_was_paragraph {
            // Reference definitions cannot interrupt a paragraph in pandoc:
            // a `[label]:` line that lazily continues a paragraph is prose
            // (whose `[label]` may still be a shortcut reference).
            return;
        }
        if let Some(captures) = REF_DEF_RE.captures(line) {
            let label = captures.get(1).unwrap();
            let target = captures.get(2).unwrap();
            if target.as_str().is_empty() {
                // `[label]:` with the URL on a following line (possibly after
                // blank lines): wait for the target instead of recording an
                // empty definition now.
                self.pending_ref_def = Some(PendingRefDef {
                    label: label.as_str().to_string(),
                    normalized_label: crate::normalize_label(label.as_str()),
                    label_range: TextRange::new(
                        byte_offset + label.start(),
                        byte_offset + label.end(),
                    ),
                });
                self.pending_ref_title = false;
                return;
            }
            // A same-line title completes the definition; without one,
            // pandoc still consumes a title-only line that follows.
            self.pending_ref_title = captures.get(3).is_none();
            let target_text = target
                .as_str()
                .trim()
                .trim_start_matches('<')
                .trim_end_matches('>');
            let target_offset = if target.as_str().starts_with('<') {
                target.start() + 1
            } else {
                target.start()
            };
            self.output.analysis.links.push(MarkdownLink {
                kind: LinkKind::Definition,
                target: target_text.to_string(),
                label: Some(label.as_str().to_string()),
                range: TextRange::new(byte_offset, byte_offset + line.len()),
                target_range: TextRange::new(
                    byte_offset + target_offset,
                    byte_offset + target_offset + target_text.len(),
                ),
            });
            self.push_semantic_token(
                SemanticTokenKind::Link,
                TextRange::new(
                    byte_offset + target_offset,
                    byte_offset + target_offset + target_text.len(),
                ),
            );
            self.output
                .analysis
                .reference_definitions
                .push(ReferenceDefinition {
                    label: label.as_str().to_string(),
                    normalized_label: crate::normalize_label(label.as_str()),
                    target: target_text.to_string(),
                    range: TextRange::new(byte_offset, byte_offset + line.len()),
                    label_range: TextRange::new(
                        byte_offset + label.start(),
                        byte_offset + label.end(),
                    ),
                });
            self.scan_links_with_carry(line, byte_offset, None, masked, masked);
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
                // The attribute block is always consumed, even when it only
                // carries classes or key=value pairs (`## Options {.opts}`):
                // pandoc never includes it in the title or the identifier.
                let attr_start = attributes.whole_range.start.saturating_sub(byte_offset);
                let display_end = trim_ascii_whitespace_end(line, title_start, attr_start);
                title_end = display_end;
                if attributes.id.is_some() {
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
        // HTML comments never contribute to the heading text or identifier
        // (`# Title <!-- omit in toc -->`), and raw inline HTML tags are
        // dropped from identifier computation just like pandoc does.
        strip_html_comments(&mut title);
        let slug_text = strip_emphasis_markers(&strip_inline_html_tags(&title));
        if source == IdentifierSource::None {
            if let Some((auto_id, algorithm)) = self.automatic_identifier(&slug_text) {
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

    fn scan_inline_line(
        &mut self,
        masked: &str,
        raw_text: &str,
        line: &str,
        byte_offset: usize,
        inline_base: usize,
    ) {
        // Reference definitions cannot interrupt a paragraph in pandoc: a
        // `[label]:` line that lazily continues a paragraph is prose, and
        // the `[label]` inside it is a (possible) shortcut reference.
        let is_footnote_definition = FOOTNOTE_DEF_RE.is_match(line) && !self.previous_was_paragraph;
        let is_reference_definition = REF_DEF_RE.is_match(line) && !self.previous_was_paragraph;

        if !is_reference_definition {
            for captures in FULL_REF_RE.captures_iter(raw_text) {
                let whole = captures.get(0).unwrap();
                if inside_inline_code(masked, whole.start()) {
                    continue;
                }
                let label = captures.get(2).unwrap();
                if label.as_str().starts_with('^') {
                    continue;
                }
                self.output.analysis.reference_links.push(ReferenceLink {
                    label: label.as_str().to_string(),
                    normalized_label: crate::normalize_label(label.as_str()),
                    range: TextRange::new(inline_base + whole.start(), inline_base + whole.end()),
                    label_range: TextRange::new(
                        inline_base + label.start(),
                        inline_base + label.end(),
                    ),
                });
            }

            for captures in COLLAPSED_REF_RE.captures_iter(raw_text) {
                let whole = captures.get(0).unwrap();
                if inside_inline_code(masked, whole.start()) {
                    continue;
                }
                let label = captures.get(1).unwrap();
                self.output.analysis.reference_links.push(ReferenceLink {
                    label: label.as_str().to_string(),
                    normalized_label: crate::normalize_label(label.as_str()),
                    range: TextRange::new(inline_base + whole.start(), inline_base + whole.end()),
                    label_range: TextRange::new(
                        inline_base + label.start(),
                        inline_base + label.end(),
                    ),
                });
            }

            if self.extensions.contains(Extension::ShortcutReferenceLinks) {
                for captures in BRACKET_SPAN_RE.captures_iter(raw_text) {
                    let whole = captures.get(0).unwrap();
                    if inside_inline_code(masked, whole.start()) {
                        continue;
                    }
                    let label = captures.get(1).unwrap();
                    // Manual neighbor checks replace lookarounds (unsupported
                    // in Rust's regex crate). `[@cite]` and `^[note]` belong
                    // to other extensions; `![img]` is an image; `- [ ]` is
                    // a task list item.
                    let prefix = raw_text.get(..whole.start()).unwrap_or("");
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
                    let after = raw_text.get(whole.end()..).unwrap_or("");
                    // A following `:` does not disqualify the reference:
                    // effective definition lines never reach this branch,
                    // and a lazily-continued paragraph line like
                    // `... with\n[label]:` still holds a shortcut reference
                    // in pandoc.
                    let after_ok = !after.starts_with(['(', '[', '{']);
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
                                inline_base + whole.start(),
                                inline_base + whole.end(),
                            ),
                            label_range: TextRange::new(
                                inline_base + label.start(),
                                inline_base + label.end(),
                            ),
                        });
                    }
                }
            }
        }

        let footnotes_on = self.extensions.contains(Extension::Footnotes);
        if !is_footnote_definition && footnotes_on {
            for captures in FOOTNOTE_REF_RE.captures_iter(masked) {
                let whole = captures.get(0).unwrap();
                let label = captures.get(1).unwrap();
                self.push_semantic_token(
                    SemanticTokenKind::Footnote,
                    TextRange::new(inline_base + whole.start(), inline_base + whole.end()),
                );
                self.output
                    .analysis
                    .footnote_references
                    .push(FootnoteReference {
                        label: label.as_str().to_string(),
                        normalized_label: crate::normalize_label(label.as_str()),
                        range: TextRange::new(
                            inline_base + whole.start(),
                            inline_base + whole.end(),
                        ),
                        label_range: TextRange::new(
                            inline_base + label.start(),
                            inline_base + label.end(),
                        ),
                    });
            }
        } else if !is_footnote_definition && self.options.disabled_extensions {
            for captures in FOOTNOTE_REF_RE.captures_iter(masked) {
                let whole = captures.get(0).unwrap();
                let range = TextRange::new(inline_base + whole.start(), inline_base + whole.end());
                self.push_disabled_diagnostic(
                    Extension::Footnotes,
                    range,
                    Severity::Warning,
                    "footnotes are disabled",
                );
            }
        }

        let inline_notes_on = self.extensions.contains(Extension::InlineNotes);
        for captures in INLINE_NOTE_RE.captures_iter(masked) {
            let whole = captures.get(0).unwrap();
            let content = captures.get(1).unwrap();
            let range = TextRange::new(inline_base + whole.start(), inline_base + whole.end());
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

        for captures in HEADING_LINK_RE.captures_iter(masked) {
            let whole = captures.get(0).unwrap();
            let anchor = captures.get(1).unwrap();
            self.output.analysis.heading_links.push(HeadingLink {
                anchor: anchor.as_str().to_string(),
                range: TextRange::new(inline_base + whole.start(), inline_base + whole.end()),
                anchor_range: TextRange::new(
                    inline_base + anchor.start(),
                    inline_base + anchor.end(),
                ),
            });
        }

        let citations_on = self.extensions.contains(Extension::Citations);
        for captures in CITATION_RE.captures_iter(masked) {
            let sigil = captures.get(2).unwrap();
            let key = captures.get(3).unwrap();
            // Pandoc strips trailing punctuation (.,;:!?) from citation keys.
            let mut key_end = key.end();
            while key_end > key.start()
                && matches!(
                    key.as_str().as_bytes()[key_end - key.start() - 1],
                    b'.' | b',' | b';' | b':' | b'!' | b'?'
                )
            {
                key_end -= 1;
            }
            if key_end == key.start() {
                continue;
            }
            let key_text = &key.as_str()[..key_end - key.start()];
            // `(@label)` is an example-list reference, not a citation, but
            // only for labels that an example-list marker defined.
            let is_example_reference = masked[..sigil.start()].ends_with('(')
                && masked[key_end..].starts_with(')')
                && self.example_labels.contains(key_text);
            if is_example_reference {
                continue;
            }
            let key_range = TextRange::new(inline_base + key.start(), inline_base + key_end);
            if citations_on {
                self.push_semantic_token(
                    SemanticTokenKind::Citation,
                    TextRange::new(inline_base + sigil.start(), inline_base + key_end),
                );
                self.output.analysis.citations.push(Citation {
                    key: key_text.to_string(),
                    range: TextRange::new(inline_base + sigil.start(), inline_base + key_end),
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
                for captures in regex.captures_iter(masked) {
                    let whole = captures.get(0).unwrap();
                    self.push_semantic_token(
                        SemanticTokenKind::Math,
                        TextRange::new(inline_base + whole.start(), inline_base + whole.end()),
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

    /// Inspect grid-table row cells: ATX headings inside cells become
    /// headings (pandoc parses full block content per cell), and cells
    /// indented four or more spaces from their border are code cells whose
    /// text must not produce citations, notes, or links. Code and heading
    /// cells are blanked out of `row_masked` for the inline scanners.
    fn scan_grid_row_cells(
        &mut self,
        line: &str,
        byte_offset: usize,
        masked: &str,
        row_masked: &mut [u8],
    ) {
        let mut cell_start = 0usize;
        for (position, ch) in line.char_indices() {
            if ch != '|' || position == 0 {
                continue;
            }
            let cell = &line[cell_start + 1..position];
            let indent = cell.len() - cell.trim_start_matches([' ', '\t']).len();
            let _ = masked;
            if indent >= 4 {
                for byte in &mut row_masked[cell_start + 1..position] {
                    *byte = b' ';
                }
            } else if let Some(captures) = HEADING_RE.captures(cell) {
                let marker = captures.get(1).unwrap();
                let title = captures.get(2).unwrap();
                self.push_heading(
                    cell,
                    byte_offset + cell_start + 1,
                    marker.as_str().len() as u8,
                    title.start(),
                    title.end(),
                );
                for byte in &mut row_masked[cell_start + 1..position] {
                    *byte = b' ';
                }
            }
            cell_start = position;
        }
    }

    // ---- Links -----------------------------------------------------------

    fn scan_links_with_carry(
        &mut self,
        line: &str,
        byte_offset: usize,
        carried: Option<(usize, String, String)>,
        masked: &str,
        raw_line: &str,
    ) {
        // Inline links, possibly with link text carried over from the
        // previous line (`[User's\nGuide](url)` is one link in pandoc).
        let virtual_text;
        let virtual_raw;
        let scan_base;
        match &carried {
            Some((carry_offset, carry_strict, carry_raw)) => {
                let joiner = wrap_joiner(carry_offset + carry_strict.len(), byte_offset);
                virtual_text = format!("{carry_strict}{joiner}{masked}");
                virtual_raw = format!("{carry_raw}{joiner}{raw_line}");
                scan_base = *carry_offset;
            }
            None => {
                virtual_text = masked.to_string();
                virtual_raw = raw_line.to_string();
                scan_base = byte_offset;
            }
        }
        let mut matched = Vec::new();
        self.scan_inline_links(&virtual_text, scan_base, &mut matched);

        for captures in AUTOLINK_RE.captures_iter(masked) {
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

        // Email autolinks `<user@example.com>` resolve to mailto: targets.
        for captures in EMAIL_AUTOLINK_RE.captures_iter(masked) {
            let whole = captures.get(0).unwrap();
            let address = captures.get(1).unwrap();
            self.output.analysis.links.push(MarkdownLink {
                kind: LinkKind::Autolink,
                target: format!("mailto:{}", address.as_str()),
                label: Some(address.as_str().to_string()),
                range: TextRange::new(byte_offset + whole.start(), byte_offset + whole.end()),
                target_range: TextRange::new(
                    byte_offset + address.start(),
                    byte_offset + address.end(),
                ),
            });
        }

        // Remember an unclosed `[` so that a link opening on this line can
        // close on the next one. Already-matched spans are blanked out so
        // the carry never re-finds the same link on a later line.
        let mut carry_text = virtual_text.into_bytes();
        for (start, end) in matched {
            let start = start.min(carry_text.len());
            let end = end.min(carry_text.len());
            for byte in &mut carry_text[start..end] {
                *byte = b' ';
            }
        }
        let carry_text = String::from_utf8(carry_text).expect("carry text is valid UTF-8");
        if let Some((offset, text)) = unclosed_bracket(&carry_text) {
            if offset + text.len() <= LINK_CARRY_MAX {
                // The raw variant of the same region: same offsets in the
                // equally long raw text.
                let raw_slice = virtual_raw
                    .get(offset..offset + text.len())
                    .unwrap_or(text.as_str())
                    .to_string();
                self.link_carry = Some((scan_base + offset, text, raw_slice));
            }
        }
        let _ = line;
    }

    /// Find inline `[text](target)` links (and images) in `masked` text
    /// whose byte 0 corresponds to absolute offset `base`. Nested images
    /// inside link text (`[![alt](src)](target)`) are recorded too, like
    /// pandoc's AST does.
    fn scan_inline_links(&mut self, masked: &str, base: usize, matched: &mut Vec<(usize, usize)>) {
        for captures in INLINE_LINK_RE.captures_iter(masked) {
            let whole = captures.get(0).unwrap();
            let bang = captures.get(1).unwrap();
            let text = captures.get(2).unwrap();
            let url = captures.get(3).unwrap();
            let target = url
                .as_str()
                .trim()
                .trim_matches(|ch| ch == '<' || ch == '>')
                .replace("\\(", "(")
                .replace("\\)", ")");
            let kind = if bang.as_str().is_empty() {
                LinkKind::Inline
            } else {
                LinkKind::Image
            };
            let angle = usize::from(url.as_str().starts_with('<'));
            let url_offset = url.start() + angle;
            // The range covers the raw source bytes of the destination
            // (escapes and all), not the unescaped target length.
            let raw_dest_len = url.as_str().len().saturating_sub(2 * angle);
            self.output.analysis.links.push(MarkdownLink {
                kind,
                target: target.to_string(),
                label: Some(text.as_str().to_string()),
                range: TextRange::new(base + whole.start(), base + whole.end()),
                target_range: TextRange::new(base + url_offset, base + url_offset + raw_dest_len),
            });
            self.push_semantic_token(
                SemanticTokenKind::Link,
                TextRange::new(base + whole.start(), base + whole.end()),
            );
            matched.push((whole.start(), whole.end()));
            if kind == LinkKind::Inline && text.as_str().contains("![") {
                let text_base = base + whole.start() + bang.as_str().len() + 1;
                self.scan_inline_links(text.as_str(), text_base, matched);
            }
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

    fn scan_disabled_inline(&mut self, line: &str, masked: &str, byte_offset: usize) {
        if !self.options.disabled_extensions {
            return;
        }

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
            for captures in MARK_RE.captures_iter(masked) {
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
            for captures in EMOJI_RE.captures_iter(masked) {
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
            for captures in SUBSCRIPT_RE.captures_iter(masked) {
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
            for captures in INLINE_MATH_RE.captures_iter(masked) {
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
            for captures in MATH_BACKSLASH_RE.captures_iter(masked) {
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
    // Fences may be indented to align with list content, and may sit inside
    // a block quote; indented-code handling in `line()` already keeps
    // top-level indented fences out of reach.
    let after_indent = line.trim_start_matches([' ', '\t']);
    let quote_len = usize::from(after_indent.starts_with('>'));
    let trimmed = if quote_len == 1 {
        after_indent
            .strip_prefix('>')
            .unwrap()
            .trim_start_matches([' ', '\t'])
    } else {
        after_indent
    };
    let delimiter = trimmed.chars().next()?;
    if !matches!(delimiter, '`' | '~') {
        return None;
    }
    let length = trimmed.chars().take_while(|ch| *ch == delimiter).count();
    (length >= 3).then_some((delimiter, length))
}

/// Block-quote depth and the visual column of the first character that is
/// neither whitespace nor part of the leading `>` prefixes (tabs count as
/// four columns, each `>` as two).
fn line_geometry(line: &str) -> (usize, usize) {
    let mut column = 0usize;
    let mut quotes = 0usize;
    let mut chars = line.chars().peekable();
    while let Some(&ch) = chars.peek() {
        match ch {
            ' ' => column += 1,
            '\t' => column = (column / 4 + 1) * 4,
            '>' if column <= 3 => {
                quotes += 1;
                column += 2;
            }
            _ => break,
        }
        chars.next();
    }
    (quotes, column)
}

/// Content column of a definition-list definition marker line
/// (`: definition`, `:: definition`, `~ definition`), if this is one.
fn definition_list_content_column(line: &str) -> Option<usize> {
    if !DEF_LIST_MARKER_RE.is_match(line) {
        return None;
    }
    let trimmed = line.trim_start_matches([' ', '\t']);
    let after_marker = trimmed.trim_start_matches([':', '~']);
    let marker_len = trimmed.len() - after_marker.len();
    let spaces = after_marker.len() - after_marker.trim_start_matches([' ', '\t']).len();
    let indent = line.len() - trimmed.len();
    Some(indent + marker_len + spaces)
}

/// Content column of a bullet or ordered list item line, if this is one.
fn list_content_column(line: &str) -> Option<usize> {
    let trimmed = line.trim_start_matches([' ', '\t']);
    let indent = line.len() - trimmed.len();
    if indent > 3 {
        return None;
    }
    let bytes = trimmed.as_bytes();
    let marker_len = if matches!(bytes.first(), Some(b'-') | Some(b'*') | Some(b'+'))
        && bytes.get(1).is_some_and(|b| *b == b' ' || *b == b'\t')
    {
        2
    } else if bytes.first().is_some_and(|b| b.is_ascii_digit())
        && trimmed.find(['.', ')']).is_some_and(|position| {
            trimmed[..position].chars().all(|ch| ch.is_ascii_digit())
                && bytes
                    .get(position + 1)
                    .is_some_and(|b| *b == b' ' || *b == b'\t')
        })
    {
        trimmed.find(['.', ')']).map(|position| position + 2)?
    } else {
        return None;
    };
    let after_marker = &trimmed[marker_len..];
    let spaces = after_marker.len() - after_marker.trim_start_matches([' ', '\t']).len();
    Some(indent + marker_len + spaces)
}

/// Remove `<!-- ... -->` comments from a heading title in place.
fn strip_html_comments(title: &mut String) {
    if !title.contains("<!--") {
        return;
    }
    let mut cleaned = String::with_capacity(title.len());
    let mut rest = title.as_str();
    while let Some(start) = rest.find("<!--") {
        cleaned.push_str(&rest[..start]);
        match rest[start..].find("-->") {
            Some(end) => rest = &rest[start + end + 3..],
            None => {
                rest = "";
                break;
            }
        }
    }
    cleaned.push_str(rest);
    *title = cleaned;
}

/// Drop emphasis markers (`**bold**`, `_italic_`) from the text used for
/// identifier computation: pandoc computes identifiers from the plain text
/// of the heading inlines, where formatting markers do not exist.
fn strip_emphasis_markers(title: &str) -> String {
    if !title.contains('*') && !title.contains('_') {
        return title.to_string();
    }
    let mut cleaned = title.replace('*', "");
    // `_word_` emphasis pairs (not snake_case identifiers, which never wrap
    // a whole word in single underscores with word boundaries around them).
    let emphasis = regex::Regex::new(r"(^|[^[:alnum:]_])_([^_\s]+)_([^[:alnum:]_]|$)").unwrap();
    let mut previous = cleaned.clone();
    loop {
        let next = emphasis.replace_all(&previous, "${1}${2}${3}").to_string();
        if next == previous {
            break;
        }
        previous = next;
    }
    cleaned = previous;
    cleaned
}

/// Drop inline HTML tags (`<b>`, `</em>`, `<br/>`) from the text used for
/// identifier computation, mirroring pandoc, which excludes RawInline
/// elements from identifiers.
fn strip_inline_html_tags(title: &str) -> String {
    if !title.contains('<') {
        return title.to_string();
    }
    let mut cleaned = String::with_capacity(title.len());
    let mut rest = title;
    while let Some(start) = rest.find('<') {
        cleaned.push_str(&rest[..start]);
        match rest[start..].find('>') {
            Some(end) => rest = &rest[start + end + 1..],
            None => break,
        }
    }
    cleaned.push_str(rest);
    cleaned
}

/// Split a reference-definition continuation line into (target, range,
/// has_title). The target may be wrapped in `<...>`; the range excludes the
/// brackets and any surrounding whitespace.
fn continuation_target(line: &str, byte_offset: usize) -> (String, Option<TextRange>, bool) {
    if let Some(captures) = REF_DEF_CONTINUATION_TITLE_RE.captures(line) {
        let head = captures.get(1).unwrap();
        let trimmed = head.as_str().trim_end();
        let trailing_ws = head.as_str().len() - trimmed.len();
        let (text, range) =
            angle_target_with_range(trimmed, byte_offset + head.start() + trailing_ws);
        return (text, Some(range), true);
    }
    let trimmed = line.trim();
    let lead = line.len() - line.trim_start().len();
    let (text, range) = angle_target_with_range(trimmed, byte_offset + lead);
    (text, Some(range), false)
}

/// Strip optional angle brackets from a target while reporting the range of
/// the destination text itself.
fn angle_target_with_range(text: &str, start: usize) -> (String, TextRange) {
    let inner = text
        .strip_prefix('<')
        .and_then(|rest| rest.strip_suffix('>'));
    match inner {
        Some(inner) => (
            inner.to_string(),
            TextRange::new(start + 1, start + 1 + inner.len()),
        ),
        None => (text.to_string(), TextRange::new(start, start + text.len())),
    }
}

/// The joiner between text carried over from the previous line (ending at
/// byte `carry_end`) and the current line at `line_offset`: it occupies
/// exactly the bytes between them (the newline, or CRLF), so every offset in
/// the joined text is an exact document offset. Never shorter than one
/// space, so the wrap is still visible as whitespace.
fn wrap_joiner(carry_end: usize, line_offset: usize) -> String {
    " ".repeat(line_offset.saturating_sub(carry_end).max(1))
}

/// A grid-table rule line (`+---+---+`, `+===+===+`).
static GRID_RULE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[ \t]{0,3}\+[-+=]+\+$").unwrap());

/// The label of an example-list marker line (`(@label) text`), if this is
/// one.
fn example_list_label(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix("(@")?;
    let close = rest.find(')')?;
    let after = &rest[close + 1..];
    if !after.starts_with([' ', '\t']) {
        return None;
    }
    Some(rest[..close].to_string())
}

/// Position of the `(` of a `](` whose parentheses never close on this
/// line (a wrapped inline-link destination).
fn unclosed_link_paren(masked: &str) -> Option<usize> {
    let bytes = masked.as_bytes();
    let mut index = 0;
    while index + 1 < bytes.len() {
        if bytes[index] == b']' && bytes[index + 1] == b'(' {
            let mut depth = 0i32;
            let mut scan = index + 1;
            let mut closed = false;
            while scan < bytes.len() {
                match bytes[scan] {
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            closed = true;
                            break;
                        }
                    }
                    _ => {}
                }
                scan += 1;
            }
            if !closed {
                return Some(index + 1);
            }
            index = scan;
        } else {
            index += 1;
        }
    }
    None
}

/// Whether the byte at `position` in the raw line was masked as inline code.
fn inside_inline_code(masked: &str, position: usize) -> bool {
    masked
        .as_bytes()
        .get(position)
        .is_some_and(|byte| *byte == b' ')
        && position < masked.len()
}

/// If the masked text ends inside an unclosed `[`, return the offset and
/// text of the leftmost unclosed opening bracket (excluding footnote
/// brackets `[^`). Used to carry link text across line wraps.
fn unclosed_bracket(masked: &str) -> Option<(usize, String)> {
    let mut stack = Vec::new();
    for (index, ch) in masked.char_indices() {
        match ch {
            '[' => stack.push(index),
            ']' => {
                stack.pop();
            }
            _ => {}
        }
    }
    if stack.is_empty() {
        // No unclosed bracket, but an inline link destination may wrap:
        // `[foo](/bar` continues on the next line. Carry the whole construct
        // from its link-text bracket.
        if let Some(open_paren) = unclosed_link_paren(masked) {
            let close = masked[..open_paren].rfind(']')?;
            let mut depth = 0i32;
            let mut start = None;
            for (index, ch) in masked[..close].char_indices().rev() {
                match ch {
                    ']' => depth += 1,
                    '[' => {
                        depth -= 1;
                        if depth < 0 {
                            start = Some(index);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let start = start?;
            if masked[start..].starts_with("[^") {
                return None;
            }
            return Some((start, masked[start..].to_string()));
        }
        return None;
    }
    let mut start = *stack.first()?;
    if masked[start..].starts_with("[^") {
        return None;
    }
    if start > 0 && masked.as_bytes().get(start - 1) == Some(&b']') {
        let mut depth = 0i32;
        for (index, ch) in masked[..start].char_indices().rev() {
            match ch {
                ']' => depth += 1,
                '[' => {
                    depth -= 1;
                    if depth == 0 {
                        start = index;
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    Some((start, masked[start..].to_string()))
}

/// Mask inline code spans, carrying an unclosed backtick run from the
/// previous line (inline code may wrap across lines, like pandoc's).
fn mask_inline_code_with_carry(
    line: &str,
    code_carry: Option<usize>,
    next_carry: &mut Option<usize>,
) -> String {
    if code_carry.is_none() && !line.contains('`') {
        return line.to_string();
    }
    let masked = mask_inline_code(line);
    if let Some(run) = code_carry {
        // Find the closing run of the same length anywhere in this line and
        // blank everything up to and including it.
        let mut scan = 0usize;
        let bytes = masked.as_bytes().to_vec();
        while scan < bytes.len() {
            if bytes[scan] == b'`' {
                let mut length = 0;
                while scan + length < bytes.len() && bytes[scan + length] == b'`' {
                    length += 1;
                }
                if length >= run {
                    let mut blanked = masked.into_bytes();
                    for byte in &mut blanked[..scan + length] {
                        *byte = b' ';
                    }
                    *next_carry = None;
                    return String::from_utf8(blanked).expect("mask is valid UTF-8");
                }
                scan += length;
            } else {
                scan += 1;
            }
        }
        // Still unclosed: blank the whole line and keep carrying.
        *next_carry = Some(run);
        return " ".repeat(line.len());
    }
    // Compute a fresh carry: the first backtick run still standing after
    // same-line pairing opens a code span that continues onto following
    // lines, so everything from it to the end of this line is masked too.
    if let Some((position, run)) = first_unmatched_backtick_run(&masked) {
        let mut blanked = masked.into_bytes();
        for byte in &mut blanked[position..] {
            *byte = b' ';
        }
        *next_carry = Some(run);
        return String::from_utf8(blanked).expect("mask is valid UTF-8");
    }
    *next_carry = None;
    masked
}

/// Position and length of the first backtick run (1 or 2 backticks) in the
/// masked line; mask_inline_code blanked all closed pairs, so a remaining
/// run opens a span that continues on a following line.
fn first_unmatched_backtick_run(masked: &str) -> Option<(usize, usize)> {
    let bytes = masked.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'`' {
            let mut length = 0;
            while index + length < bytes.len() && bytes[index + length] == b'`' {
                length += 1;
            }
            if (1..=2).contains(&length) {
                return Some((index, length));
            }
            index += length;
        } else {
            index += 1;
        }
    }
    None
}

/// Byte extents (start, end) of same-line closed inline code spans with
/// one or two backticks.
fn code_span_extents(line: &str) -> Vec<(usize, usize)> {
    let mut extents = Vec::new();
    if !line.contains('`') {
        return extents;
    }
    let bytes = line.as_bytes();
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
                extents.push((run_start, close + run_len));
                index = close + run_len;
            }
        } else {
            index += 1;
        }
    }
    extents
}

/// Replace inline code spans (`...`, ``...``) with spaces so that inline
/// regexes do not match inside them. Byte length is preserved.
fn mask_inline_code(line: &str) -> String {
    let extents = code_span_extents(line);
    if extents.is_empty() {
        return line.to_string();
    }
    let mut masked = line.as_bytes().to_vec();
    for (start, end) in extents {
        for byte in &mut masked[start..end] {
            if *byte != b' ' {
                *byte = b' ';
            }
        }
    }
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
