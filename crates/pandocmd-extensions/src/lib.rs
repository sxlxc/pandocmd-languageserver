//! Model of every Pandoc Markdown extension, with per-flavor defaults that
//! mirror `pandoc --list-extensions=markdown` (verified against pandoc 3.x).
//!
//! The extension names, their defaults for each built-in Pandoc flavor, and
//! the `+ext`/`-ext` diff syntax all follow the Pandoc User's Guide section
//! *Pandoc's Markdown* (<https://pandoc.org/MANUAL.html#pandocs-markdown>).

use std::fmt;

/// Every Markdown extension understood by Pandoc 3.x.
///
/// Variant order matches the alphabetical order used by
/// `pandoc --list-extensions`, and that order is part of this crate's
/// stable API (it defines the [`Extension::index`] bit assignment).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Extension {
    Abbreviations,
    Alerts,
    AllSymbolsEscapable,
    AngleBracketsEscapable,
    AsciiIdentifiers,
    Attributes,
    AutoIdentifiers,
    AutolinkBareUris,
    BacktickCodeBlocks,
    BlankBeforeBlockquote,
    BlankBeforeHeader,
    BracketedSpans,
    Citations,
    DefinitionLists,
    EastAsianLineBreaks,
    Emoji,
    EscapedLineBreaks,
    ExampleLists,
    FancyLists,
    FencedCodeAttributes,
    FencedCodeBlocks,
    FencedDivs,
    Footnotes,
    FourSpaceRule,
    GfmAutoIdentifiers,
    GridTables,
    Gutenberg,
    HardLineBreaks,
    HeaderAttributes,
    TableAttributes,
    IgnoreLineBreaks,
    ImplicitFigures,
    ImplicitHeaderReferences,
    InlineCodeAttributes,
    InlineNotes,
    IntrawordUnderscores,
    LatexMacros,
    LineBlocks,
    LinkAttributes,
    ListsWithoutPrecedingBlankline,
    LiterateHaskell,
    Mark,
    MarkdownAttribute,
    MarkdownInHtmlBlocks,
    MmdHeaderIdentifiers,
    MmdLinkAttributes,
    MmdTitleBlock,
    MultilineTables,
    NativeDivs,
    NativeSpans,
    OldDashes,
    PandocTitleBlock,
    PipeTables,
    RawAttribute,
    RawHtml,
    RawTex,
    RebaseRelativePaths,
    ShortSubsuperscripts,
    ShortcutReferenceLinks,
    SimpleTables,
    Smart,
    Sourcepos,
    SpaceInAtxHeader,
    SpacedReferenceLinks,
    Startnum,
    Strikeout,
    Subscript,
    Superscript,
    TableCaptions,
    TaskLists,
    TexMathDollars,
    TexMathDoubleBackslash,
    TexMathGfm,
    TexMathSingleBackslash,
    WikilinksTitleAfterPipe,
    WikilinksTitleBeforePipe,
    YamlMetadataBlock,
}

impl Extension {
    /// All known extensions, in `pandoc --list-extensions` order.
    pub const ALL: &'static [Extension] = &[
        Extension::Abbreviations,
        Extension::Alerts,
        Extension::AllSymbolsEscapable,
        Extension::AngleBracketsEscapable,
        Extension::AsciiIdentifiers,
        Extension::Attributes,
        Extension::AutoIdentifiers,
        Extension::AutolinkBareUris,
        Extension::BacktickCodeBlocks,
        Extension::BlankBeforeBlockquote,
        Extension::BlankBeforeHeader,
        Extension::BracketedSpans,
        Extension::Citations,
        Extension::DefinitionLists,
        Extension::EastAsianLineBreaks,
        Extension::Emoji,
        Extension::EscapedLineBreaks,
        Extension::ExampleLists,
        Extension::FancyLists,
        Extension::FencedCodeAttributes,
        Extension::FencedCodeBlocks,
        Extension::FencedDivs,
        Extension::Footnotes,
        Extension::FourSpaceRule,
        Extension::GfmAutoIdentifiers,
        Extension::GridTables,
        Extension::Gutenberg,
        Extension::HardLineBreaks,
        Extension::HeaderAttributes,
        Extension::TableAttributes,
        Extension::IgnoreLineBreaks,
        Extension::ImplicitFigures,
        Extension::ImplicitHeaderReferences,
        Extension::InlineCodeAttributes,
        Extension::InlineNotes,
        Extension::IntrawordUnderscores,
        Extension::LatexMacros,
        Extension::LineBlocks,
        Extension::LinkAttributes,
        Extension::ListsWithoutPrecedingBlankline,
        Extension::LiterateHaskell,
        Extension::Mark,
        Extension::MarkdownAttribute,
        Extension::MarkdownInHtmlBlocks,
        Extension::MmdHeaderIdentifiers,
        Extension::MmdLinkAttributes,
        Extension::MmdTitleBlock,
        Extension::MultilineTables,
        Extension::NativeDivs,
        Extension::NativeSpans,
        Extension::OldDashes,
        Extension::PandocTitleBlock,
        Extension::PipeTables,
        Extension::RawAttribute,
        Extension::RawHtml,
        Extension::RawTex,
        Extension::RebaseRelativePaths,
        Extension::ShortSubsuperscripts,
        Extension::ShortcutReferenceLinks,
        Extension::SimpleTables,
        Extension::Smart,
        Extension::Sourcepos,
        Extension::SpaceInAtxHeader,
        Extension::SpacedReferenceLinks,
        Extension::Startnum,
        Extension::Strikeout,
        Extension::Subscript,
        Extension::Superscript,
        Extension::TableCaptions,
        Extension::TaskLists,
        Extension::TexMathDollars,
        Extension::TexMathDoubleBackslash,
        Extension::TexMathGfm,
        Extension::TexMathSingleBackslash,
        Extension::WikilinksTitleAfterPipe,
        Extension::WikilinksTitleBeforePipe,
        Extension::YamlMetadataBlock,
    ];

    /// Stable bit index derived from [`Extension::ALL`] order.
    pub const fn index(self) -> u32 {
        self as u32
    }

    /// The Pandoc extension name in kebab-case, as accepted by
    /// `-f markdown+NAME` / `--list-extensions` output.
    pub const fn name(self) -> &'static str {
        match self {
            Extension::Abbreviations => "abbreviations",
            Extension::Alerts => "alerts",
            Extension::AllSymbolsEscapable => "all_symbols_escapable",
            Extension::AngleBracketsEscapable => "angle_brackets_escapable",
            Extension::AsciiIdentifiers => "ascii_identifiers",
            Extension::Attributes => "attributes",
            Extension::AutoIdentifiers => "auto_identifiers",
            Extension::AutolinkBareUris => "autolink_bare_uris",
            Extension::BacktickCodeBlocks => "backtick_code_blocks",
            Extension::BlankBeforeBlockquote => "blank_before_blockquote",
            Extension::BlankBeforeHeader => "blank_before_header",
            Extension::BracketedSpans => "bracketed_spans",
            Extension::Citations => "citations",
            Extension::DefinitionLists => "definition_lists",
            Extension::EastAsianLineBreaks => "east_asian_line_breaks",
            Extension::Emoji => "emoji",
            Extension::EscapedLineBreaks => "escaped_line_breaks",
            Extension::ExampleLists => "example_lists",
            Extension::FancyLists => "fancy_lists",
            Extension::FencedCodeAttributes => "fenced_code_attributes",
            Extension::FencedCodeBlocks => "fenced_code_blocks",
            Extension::FencedDivs => "fenced_divs",
            Extension::Footnotes => "footnotes",
            Extension::FourSpaceRule => "four_space_rule",
            Extension::GfmAutoIdentifiers => "gfm_auto_identifiers",
            Extension::GridTables => "grid_tables",
            Extension::Gutenberg => "gutenberg",
            Extension::HardLineBreaks => "hard_line_breaks",
            Extension::HeaderAttributes => "header_attributes",
            Extension::TableAttributes => "table_attributes",
            Extension::IgnoreLineBreaks => "ignore_line_breaks",
            Extension::ImplicitFigures => "implicit_figures",
            Extension::ImplicitHeaderReferences => "implicit_header_references",
            Extension::InlineCodeAttributes => "inline_code_attributes",
            Extension::InlineNotes => "inline_notes",
            Extension::IntrawordUnderscores => "intraword_underscores",
            Extension::LatexMacros => "latex_macros",
            Extension::LineBlocks => "line_blocks",
            Extension::LinkAttributes => "link_attributes",
            Extension::ListsWithoutPrecedingBlankline => "lists_without_preceding_blankline",
            Extension::LiterateHaskell => "literate_haskell",
            Extension::Mark => "mark",
            Extension::MarkdownAttribute => "markdown_attribute",
            Extension::MarkdownInHtmlBlocks => "markdown_in_html_blocks",
            Extension::MmdHeaderIdentifiers => "mmd_header_identifiers",
            Extension::MmdLinkAttributes => "mmd_link_attributes",
            Extension::MmdTitleBlock => "mmd_title_block",
            Extension::MultilineTables => "multiline_tables",
            Extension::NativeDivs => "native_divs",
            Extension::NativeSpans => "native_spans",
            Extension::OldDashes => "old_dashes",
            Extension::PandocTitleBlock => "pandoc_title_block",
            Extension::PipeTables => "pipe_tables",
            Extension::RawAttribute => "raw_attribute",
            Extension::RawHtml => "raw_html",
            Extension::RawTex => "raw_tex",
            Extension::RebaseRelativePaths => "rebase_relative_paths",
            Extension::ShortSubsuperscripts => "short_subsuperscripts",
            Extension::ShortcutReferenceLinks => "shortcut_reference_links",
            Extension::SimpleTables => "simple_tables",
            Extension::Smart => "smart",
            Extension::Sourcepos => "sourcepos",
            Extension::SpaceInAtxHeader => "space_in_atx_header",
            Extension::SpacedReferenceLinks => "spaced_reference_links",
            Extension::Startnum => "startnum",
            Extension::Strikeout => "strikeout",
            Extension::Subscript => "subscript",
            Extension::Superscript => "superscript",
            Extension::TableCaptions => "table_captions",
            Extension::TaskLists => "task_lists",
            Extension::TexMathDollars => "tex_math_dollars",
            Extension::TexMathDoubleBackslash => "tex_math_double_backslash",
            Extension::TexMathGfm => "tex_math_gfm",
            Extension::TexMathSingleBackslash => "tex_math_single_backslash",
            Extension::WikilinksTitleAfterPipe => "wikilinks_title_after_pipe",
            Extension::WikilinksTitleBeforePipe => "wikilinks_title_before_pipe",
            Extension::YamlMetadataBlock => "yaml_metadata_block",
        }
    }

    /// One-line description of the extension, paraphrased from the
    /// Pandoc User's Guide. Suitable for hover and completion detail text.
    pub const fn description(self) -> &'static str {
        match self {
            Extension::Abbreviations => {
                "Markdown PHP Extra style abbreviations: `*[HTML]: HyperText Markup Language`"
            }
            Extension::Alerts => {
                "GitHub-style alerts in blockquotes: `> [!NOTE]`, `> [!TIP]`, `> [!WARNING]`, ..."
            }
            Extension::AllSymbolsEscapable => {
                "Backslash escapes work for any punctuation character, not just ASCII punctuation"
            }
            Extension::AngleBracketsEscapable => {
                "`\\<` and `\\>` are also escapable, so `<foo>` can be written literally"
            }
            Extension::AsciiIdentifiers => {
                "With `auto_identifiers`, transliterate identifiers to pure ASCII"
            }
            Extension::Attributes => "Generic attributes `{#id .class key=value}` on inline and block elements (CommonMark X)",
            Extension::AutoIdentifiers => "Automatically derive heading identifiers from the heading text",
            Extension::AutolinkBareUris => "All bare absolute URIs in text are turned into links",
            Extension::BacktickCodeBlocks => "GitHub-style fenced code blocks delimited by backticks",
            Extension::BlankBeforeBlockquote => "Require a blank line before a blockquote",
            Extension::BlankBeforeHeader => "Require a blank line before an ATX heading",
            Extension::BracketedSpans => "Inline spans with attributes: `[text]{.class key=val}`",
            Extension::Citations => "Citations with citeproc keys: `[@key]`, `-@key`, `@key`",
            Extension::DefinitionLists => "Definition lists: `Term` / `: definition`",
            Extension::EastAsianLineBreaks => "Newlines between East Asian characters are ignored",
            Extension::Emoji => "Emoji shortcodes like `:smile:` are replaced with Unicode emoji",
            Extension::EscapedLineBreaks => "A backslash at the end of a line is a hard line break",
            Extension::ExampleLists => "Numbered example lists with `(@)` / `(@label)` markers",
            Extension::FancyLists => "Lists with roman numerals and alpha markers: `a.`, `ii.`, `(A)`, ...",
            Extension::FencedCodeAttributes => "Attributes on fenced code blocks: `` ```{#id .class} ``",
            Extension::FencedCodeBlocks => "Fenced code blocks delimited by `~~~` (and backticks with `backtick_code_blocks`)",
            Extension::FencedDivs => "Fenced divs delimited by `:::` lines carrying classes and attributes",
            Extension::Footnotes => "Inline footnotes references `[^label]` with `[^label]: definition` blocks",
            Extension::FourSpaceRule => "Use the four-space rule for lists exactly as in original Markdown",
            Extension::GfmAutoIdentifiers => "Use GitHub's heading identifier algorithm instead of Pandoc's",
            Extension::GridTables => "Grid tables drawn with `+---+---+` borders",
            Extension::Gutenberg => "Use Project Gutenberg conventions for paragraph breaks and quotes",
            Extension::HardLineBreaks => "Every newline inside a paragraph is a hard line break",
            Extension::HeaderAttributes => "Explicit attributes on headings: `# Heading {#id .class}`",
            Extension::TableAttributes => "Tables may carry attributes: `{#id .class}` after the table or its caption",
            Extension::IgnoreLineBreaks => "Newlines inside paragraphs are ignored entirely",
            Extension::ImplicitFigures => "A paragraph containing only an image becomes a figure",
            Extension::ImplicitHeaderReferences => "Headings can be referenced by their text: `[heading text][]`",
            Extension::InlineCodeAttributes => "Attributes on inline code: `` `code`{#id .class} ``",
            Extension::InlineNotes => "Inline footnotes: `^[inline note text]`",
            Extension::IntrawordUnderscores => "Underscores inside words are emphasis markers (Markdown PHP Extra)",
            Extension::LatexMacros => "LaTeX macro definitions are parsed and applied",
            Extension::LineBlocks => "Line blocks where each line begins with `| `",
            Extension::LinkAttributes => "Attributes on links and images: `[text](url){.class}`",
            Extension::ListsWithoutPrecedingBlankline => "Lists do not require a preceding blank line",
            Extension::LiterateHaskell => "Bird-track literate Haskell: `> code` lines",
            Extension::Mark => "Highlighted text with `==mark==` (Pandoc 3.x)",
            Extension::MarkdownAttribute => "Interpret markdown inside raw attributes: `` ```{=markdown} ``",
            Extension::MarkdownInHtmlBlocks => "Markdown inside HTML block-level tags is processed",
            Extension::MmdHeaderIdentifiers => "MultiMarkdown heading identifiers: `# Heading #custom-id`",
            Extension::MmdLinkAttributes => "MultiMarkdown key-value link attributes: `[text](url key=val)`",
            Extension::MmdTitleBlock => "MultiMarkdown title block: `Title: ...` metadata keys at the top",
            Extension::MultilineTables => "Multi-line tables with `=====` delimiters",
            Extension::NativeDivs => "HTML `<div>` elements become native Pandoc divs",
            Extension::NativeSpans => "HTML `<span>` elements become native Pandoc spans",
            Extension::OldDashes => "Use `--` for en-dashes and `---` for em-dashes (pre-2.0 style)",
            Extension::PandocTitleBlock => "Pandoc title block: `% Title` / `% Author` / `% Date`",
            Extension::PipeTables => "Pipe tables: `| a | b |` with `|---|---|` delimiter rows",
            Extension::RawAttribute => "Raw inline/blocks with format attributes: `` `<html>...`{=html} ``",
            Extension::RawHtml => "Raw HTML inline and block elements are passed through",
            Extension::RawTex => "Raw LaTeX commands are passed through",
            Extension::RebaseRelativePaths => "Relative paths in links and images are rebased to the document",
            Extension::ShortSubsuperscripts => "Sub/superscripts without spaces: `H~2~O`, `2^10^`",
            Extension::ShortcutReferenceLinks => "Shortcut reference links: `[text]` without a second `[]`",
            Extension::SimpleTables => "Simple tables with one-line rows and `-----` separators",
            Extension::Smart => "Straight quotes and dots become curly quotes, ellipses, and dashes",
            Extension::Sourcepos => "Include source positions in output (affects HTML attrs)",
            Extension::SpaceInAtxHeader => "Require a space between `#` and heading text",
            Extension::SpacedReferenceLinks => "Reference links require a space: `[text] [label]`",
            Extension::Startnum => "List start numbers are honored: `5.` starts at 5",
            Extension::Strikeout => "Strikeout text with `~~deleted~~`",
            Extension::Subscript => "Subscript with `~text~`",
            Extension::Superscript => "Superscript with `^text^`",
            Extension::TableCaptions => "Table captions: a `Table: caption` (or `: caption`) paragraph below a table",
            Extension::TaskLists => "GitHub-style task lists: `- [ ]` and `- [x]` items",
            Extension::TexMathDollars => "TeX math between dollar signs: `$x^2$` and `$$display$$`",
            Extension::TexMathDoubleBackslash => "TeX math with double backslashes: `\\(..\\)` and `\\[..\\]`",
            Extension::TexMathGfm => "GitHub-flavored math: `` $`code`$ `` and `$$display$$`",
            Extension::TexMathSingleBackslash => "TeX math with single backslashes: `\\(...\\)` and `\\[...\\]`",
            Extension::WikilinksTitleAfterPipe => "Wikilinks `[[target|title]]`",
            Extension::WikilinksTitleBeforePipe => "Wikilinks `[[title|target]]`",
            Extension::YamlMetadataBlock => "YAML metadata block delimited by `---` ... `---` at the document start",
        }
    }

    /// Look up an extension by its Pandoc kebab/underscore name.
    ///
    /// Accepts both the canonical underscore spelling (`tex_math_dollars`)
    /// and kebab-case (`tex-math-dollars`) for editor convenience.
    pub fn from_name(name: &str) -> Option<Extension> {
        let normalized = name.trim().to_ascii_lowercase().replace('-', "_");
        Extension::ALL
            .iter()
            .copied()
            .find(|extension| extension.name() == normalized)
    }
}

impl fmt::Display for Extension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Built-in Pandoc Markdown flavors that the language server can use as a
/// base extension preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Flavor {
    /// `markdown` — Pandoc's extended Markdown (the default).
    #[default]
    Markdown,
    /// `gfm` — GitHub-Flavored Markdown.
    Gfm,
    /// `commonmark` — strict CommonMark.
    CommonMark,
    /// `commonmark_x` — CommonMark with useful extensions.
    CommonMarkX,
    /// `markdown_strict` — original Markdown as supported by Pandoc.
    MarkdownStrict,
    /// `markdown_mmd` — MultiMarkdown.
    MarkdownMmd,
    /// `markdown_phpextra` — Markdown PHP Extra.
    MarkdownPhpextra,
}

impl Flavor {
    /// The Pandoc format name (`-f NAME`).
    pub const fn name(self) -> &'static str {
        match self {
            Flavor::Markdown => "markdown",
            Flavor::Gfm => "gfm",
            Flavor::CommonMark => "commonmark",
            Flavor::CommonMarkX => "commonmark_x",
            Flavor::MarkdownStrict => "markdown_strict",
            Flavor::MarkdownMmd => "markdown_mmd",
            Flavor::MarkdownPhpextra => "markdown_phpextra",
        }
    }

    pub fn from_name(name: &str) -> Option<Flavor> {
        match name.trim().to_ascii_lowercase().as_str() {
            "markdown" | "pandoc" | "pandoc_markdown" | "pandoc-markdown" => Some(Flavor::Markdown),
            "gfm" | "github_flavored_markdown" | "github-flavored-markdown" => Some(Flavor::Gfm),
            "commonmark" => Some(Flavor::CommonMark),
            "commonmark_x" | "commonmark-x" => Some(Flavor::CommonMarkX),
            "markdown_strict" | "markdown-strict" => Some(Flavor::MarkdownStrict),
            "markdown_mmd" | "markdown-mmd" | "multimarkdown" => Some(Flavor::MarkdownMmd),
            "markdown_phpextra" | "markdown-phpextra" | "phpextra" => {
                Some(Flavor::MarkdownPhpextra)
            }
            _ => None,
        }
    }

    /// Default extension set for this flavor, matching
    /// `pandoc --list-extensions=FLAVOR`.
    pub fn default_extensions(self) -> ExtensionSet {
        ExtensionSet::flavor_defaults(self)
    }
}

impl fmt::Display for Flavor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A set of enabled extensions, represented as a 128-bit bitset.
///
/// Pandoc 3.x has 77 Markdown extensions, so a single `u128` covers every
/// extension with room to spare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct ExtensionSet {
    bits: u128,
}

impl ExtensionSet {
    /// The empty set: no extensions enabled at all.
    pub const fn none() -> ExtensionSet {
        ExtensionSet { bits: 0 }
    }

    /// A set containing every known extension.
    pub fn all() -> ExtensionSet {
        ExtensionSet {
            bits: (1u128 << Extension::ALL.len()) - 1,
        }
    }

    /// Default extensions for a Pandoc flavor, exactly as reported by
    /// `pandoc --list-extensions=FLAVOR` (pandoc 3.x).
    pub fn flavor_defaults(flavor: Flavor) -> ExtensionSet {
        let enabled: &[Extension] = match flavor {
            Flavor::Markdown => &[
                Extension::AllSymbolsEscapable,
                Extension::AutoIdentifiers,
                Extension::BacktickCodeBlocks,
                Extension::BlankBeforeBlockquote,
                Extension::BlankBeforeHeader,
                Extension::BracketedSpans,
                Extension::Citations,
                Extension::DefinitionLists,
                Extension::EscapedLineBreaks,
                Extension::ExampleLists,
                Extension::FancyLists,
                Extension::FencedCodeAttributes,
                Extension::FencedCodeBlocks,
                Extension::FencedDivs,
                Extension::Footnotes,
                Extension::GridTables,
                Extension::HeaderAttributes,
                Extension::TableAttributes,
                Extension::ImplicitFigures,
                Extension::ImplicitHeaderReferences,
                Extension::InlineCodeAttributes,
                Extension::InlineNotes,
                Extension::IntrawordUnderscores,
                Extension::LatexMacros,
                Extension::LineBlocks,
                Extension::LinkAttributes,
                Extension::MarkdownInHtmlBlocks,
                Extension::MultilineTables,
                Extension::NativeDivs,
                Extension::NativeSpans,
                Extension::PandocTitleBlock,
                Extension::PipeTables,
                Extension::RawAttribute,
                Extension::RawHtml,
                Extension::RawTex,
                Extension::ShortcutReferenceLinks,
                Extension::SimpleTables,
                Extension::Smart,
                Extension::SpaceInAtxHeader,
                Extension::Startnum,
                Extension::Strikeout,
                Extension::Subscript,
                Extension::Superscript,
                Extension::TableCaptions,
                Extension::TaskLists,
                Extension::TexMathDollars,
                Extension::YamlMetadataBlock,
            ],
            Flavor::Gfm => &[
                Extension::Alerts,
                Extension::AutolinkBareUris,
                Extension::Emoji,
                Extension::Footnotes,
                Extension::GfmAutoIdentifiers,
                Extension::PipeTables,
                Extension::TexMathGfm,
                Extension::RawHtml,
                Extension::Strikeout,
                Extension::TaskLists,
                Extension::TexMathDollars,
                Extension::YamlMetadataBlock,
            ],
            Flavor::CommonMark => &[Extension::RawHtml],
            Flavor::CommonMarkX => &[
                Extension::Alerts,
                Extension::Attributes,
                Extension::BracketedSpans,
                Extension::DefinitionLists,
                Extension::Emoji,
                Extension::FancyLists,
                Extension::FencedDivs,
                Extension::Footnotes,
                Extension::GfmAutoIdentifiers,
                Extension::ImplicitHeaderReferences,
                Extension::PipeTables,
                Extension::RawAttribute,
                Extension::RawHtml,
                Extension::Smart,
                Extension::Strikeout,
                Extension::Subscript,
                Extension::Superscript,
                Extension::TaskLists,
                Extension::TexMathDollars,
                Extension::YamlMetadataBlock,
            ],
            Flavor::MarkdownStrict => &[
                Extension::RawHtml,
                Extension::ShortcutReferenceLinks,
                Extension::SpacedReferenceLinks,
            ],
            Flavor::MarkdownMmd => &[
                Extension::AllSymbolsEscapable,
                Extension::AutoIdentifiers,
                Extension::BacktickCodeBlocks,
                Extension::DefinitionLists,
                Extension::Footnotes,
                Extension::ImplicitFigures,
                Extension::ImplicitHeaderReferences,
                Extension::IntrawordUnderscores,
                Extension::MarkdownAttribute,
                Extension::MmdHeaderIdentifiers,
                Extension::MmdLinkAttributes,
                Extension::MmdTitleBlock,
                Extension::PipeTables,
                Extension::RawAttribute,
                Extension::RawHtml,
                Extension::ShortSubsuperscripts,
                Extension::ShortcutReferenceLinks,
                Extension::SpacedReferenceLinks,
                Extension::Subscript,
                Extension::Superscript,
                Extension::TexMathDollars,
                Extension::TexMathDoubleBackslash,
            ],
            Flavor::MarkdownPhpextra => &[
                Extension::Abbreviations,
                Extension::DefinitionLists,
                Extension::FencedCodeBlocks,
                Extension::Footnotes,
                Extension::HeaderAttributes,
                Extension::IntrawordUnderscores,
                Extension::LinkAttributes,
                Extension::MarkdownAttribute,
                Extension::PipeTables,
                Extension::RawHtml,
                Extension::ShortcutReferenceLinks,
                Extension::SpacedReferenceLinks,
            ],
        };

        let mut set = ExtensionSet::none();
        for extension in enabled {
            set = set.with(*extension, true);
        }
        set
    }

    /// Enable or disable a single extension, returning the new set.
    #[must_use]
    pub const fn with(self, extension: Extension, enabled: bool) -> ExtensionSet {
        let bit = 1u128 << extension.index();
        ExtensionSet {
            bits: if enabled {
                self.bits | bit
            } else {
                self.bits & !bit
            },
        }
    }

    /// Enable an extension.
    #[must_use]
    pub const fn enable(self, extension: Extension) -> ExtensionSet {
        self.with(extension, true)
    }

    /// Disable an extension.
    #[must_use]
    pub const fn disable(self, extension: Extension) -> ExtensionSet {
        self.with(extension, false)
    }

    /// Whether an extension is enabled.
    pub const fn contains(self, extension: Extension) -> bool {
        (self.bits >> extension.index()) & 1 == 1
    }

    /// Number of enabled extensions.
    pub fn len(self) -> usize {
        self.bits.count_ones() as usize
    }

    /// Whether no extensions are enabled.
    pub const fn is_empty(self) -> bool {
        self.bits == 0
    }

    /// Iterate over the enabled extensions in canonical order.
    pub fn iter(self) -> impl Iterator<Item = Extension> {
        Extension::ALL
            .iter()
            .copied()
            .filter(move |extension| self.contains(*extension))
    }

    /// Apply a Pandoc-style diff token such as `+citations` or `-smart`.
    ///
    /// Returns `Err(unknown-name)` for unrecognized extensions.
    pub fn apply_diff_token(self, token: &str) -> Result<ExtensionSet, UnknownExtension> {
        let token = token.trim();
        if token.is_empty() {
            return Ok(self);
        }

        let (enabled, name) = match token.as_bytes()[0] {
            b'+' => (true, &token[1..]),
            b'-' => (false, &token[1..]),
            _ => (true, token),
        };

        match Extension::from_name(name) {
            Some(extension) => Ok(self.with(extension, enabled)),
            None => Err(UnknownExtension {
                name: token.to_string(),
            }),
        }
    }

    /// Render as a Pandoc diff string against another base set, e.g.
    /// `+citations-smart`.
    pub fn diff_from(self, base: ExtensionSet) -> String {
        let mut diff = String::new();
        for extension in Extension::ALL {
            let (self_on, base_on) = (self.contains(*extension), base.contains(*extension));
            if self_on != base_on {
                diff.push(if self_on { '+' } else { '-' });
                diff.push_str(extension.name());
            }
        }
        diff
    }
}

impl<'a> IntoIterator for &'a ExtensionSet {
    type Item = Extension;
    type IntoIter = Box<dyn Iterator<Item = Extension> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}

impl fmt::Display for ExtensionSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for extension in self.iter() {
            if !first {
                f.write_str(" ")?;
            }
            first = false;
            write!(f, "+{}", extension.name())?;
        }
        Ok(())
    }
}

/// An extension name that Pandoc (and this crate) does not recognize.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownExtension {
    pub name: String,
}

impl fmt::Display for UnknownExtension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown Pandoc extension `{}`", self.name)
    }
}

impl std::error::Error for UnknownExtension {}

/// The result of parsing a Pandoc format specification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatSpec {
    pub flavor: Flavor,
    pub extensions: ExtensionSet,
}

/// Parse a Pandoc format string such as `markdown`, `gfm`, or
/// `markdown+citations-smart-tex_math_dollars`.
///
/// Unknown extensions are rejected with [`UnknownExtension`].
pub fn parse_format_spec(spec: &str) -> Result<FormatSpec, ParseFormatSpecError> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Ok(FormatSpec {
            flavor: Flavor::Markdown,
            extensions: Flavor::Markdown.default_extensions(),
        });
    }

    // The flavor name is the leading [A-Za-z0-9_] run; the rest is a
    // sequence of `+ext` / `-ext` tokens (pandoc accepts both
    // `markdown+smart` and `markdown-smart`).
    let flavor_len = spec.find(['+', '-']).unwrap_or(spec.len());
    let flavor_name = &spec[..flavor_len];
    let flavor =
        Flavor::from_name(flavor_name).ok_or_else(|| ParseFormatSpecError::UnknownFlavor {
            name: flavor_name.to_string(),
        })?;

    let mut extensions = flavor.default_extensions();
    for token in split_diff_tokens(&spec[flavor_len..]) {
        extensions = extensions
            .apply_diff_token(&token)
            .map_err(ParseFormatSpecError::UnknownExtension)?;
    }

    Ok(FormatSpec { flavor, extensions })
}

fn split_diff_tokens(diff: &str) -> Vec<String> {
    // A diff string like "+a-b+c" is split into "+a", "-b", "+c".
    let mut tokens = Vec::new();
    let mut current = String::new();
    for ch in diff.chars() {
        if (ch == '+' || ch == '-') && !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
        current.push(ch);
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Errors from [`parse_format_spec`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseFormatSpecError {
    UnknownFlavor { name: String },
    UnknownExtension(UnknownExtension),
}

impl fmt::Display for ParseFormatSpecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseFormatSpecError::UnknownFlavor { name } => {
                write!(f, "unknown Pandoc flavor `{name}`; expected one of markdown, gfm, commonmark, commonmark_x, markdown_strict, markdown_mmd, markdown_phpextra")
            }
            ParseFormatSpecError::UnknownExtension(unknown) => write!(f, "{unknown}"),
        }
    }
}

impl std::error::Error for ParseFormatSpecError {}

/// Editor-facing configuration for selecting which Pandoc extensions are
/// enabled. Deserializable from LSP initialization options / settings.
///
/// ```json
/// {
///   "flavor": "markdown",
///   "enabled": ["citations", "fenced_divs"],
///   "disabled": ["smart"]
/// }
/// ```
///
/// `enabled`/`disabled` entries accept both underscore and kebab-case names
/// and are applied on top of the flavor preset (`disabled` wins when both
/// mention the same extension).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionConfig {
    /// Base Pandoc flavor preset. Defaults to `markdown`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flavor: Option<String>,
    /// A full format spec such as `markdown+citations-smart`; overrides
    /// `flavor`/`enabled`/`disabled` when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// Extensions to enable on top of the preset.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enabled: Vec<String>,
    /// Extensions to disable on top of the preset.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disabled: Vec<String>,
}

impl ExtensionConfig {
    /// Resolve the configuration into an extension set.
    ///
    /// Returns the resolved set together with every unrecognized name so
    /// callers can surface diagnostics instead of failing hard.
    pub fn resolve(&self) -> (ExtensionSet, Vec<UnknownExtension>) {
        let spec = self.format.as_deref().map(str::trim);
        if let Some(spec) = spec.filter(|spec| !spec.is_empty()) {
            return match parse_format_spec(spec) {
                Ok(parsed) => (parsed.extensions, Vec::new()),
                Err(ParseFormatSpecError::UnknownExtension(unknown)) => {
                    (Flavor::Markdown.default_extensions(), vec![unknown])
                }
                Err(_) => (Flavor::Markdown.default_extensions(), Vec::new()),
            };
        }

        let flavor = self
            .flavor
            .as_deref()
            .and_then(Flavor::from_name)
            .unwrap_or_default();
        let mut set = flavor.default_extensions();
        let mut unknown = Vec::new();

        for name in &self.enabled {
            match Extension::from_name(name) {
                Some(extension) => set = set.enable(extension),
                None => unknown.push(UnknownExtension { name: name.clone() }),
            }
        }
        for name in &self.disabled {
            match Extension::from_name(name) {
                Some(extension) => set = set.disable(extension),
                None => unknown.push(UnknownExtension { name: name.clone() }),
            }
        }

        (set, unknown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_extension_round_trips() {
        for extension in Extension::ALL {
            assert_eq!(
                Extension::from_name(extension.name()),
                Some(*extension),
                "name lookup failed for {}",
                extension.name()
            );
        }
    }

    #[test]
    fn all_extensions_have_unique_names_and_round_trip() {
        assert_eq!(Extension::ALL.len(), 77);
        let mut names: Vec<&str> = Extension::ALL.iter().map(|e| e.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), Extension::ALL.len(), "duplicate names");
        assert_extension_round_trips();
    }

    #[test]
    fn extension_lookup_accepts_kebab_case() {
        assert_eq!(
            Extension::from_name("tex-math-dollars"),
            Some(Extension::TexMathDollars)
        );
        assert_eq!(
            Extension::from_name("Fenced-Divs"),
            Some(Extension::FencedDivs)
        );
        assert_eq!(Extension::from_name("nope"), None);
    }

    #[test]
    fn markdown_defaults_match_pandoc_3() {
        let set = Flavor::Markdown.default_extensions();
        // Defaults verified against `pandoc --list-extensions=markdown` (3.10).
        let expected_on = [
            "all_symbols_escapable",
            "auto_identifiers",
            "backtick_code_blocks",
            "blank_before_blockquote",
            "blank_before_header",
            "bracketed_spans",
            "citations",
            "definition_lists",
            "escaped_line_breaks",
            "example_lists",
            "fancy_lists",
            "fenced_code_attributes",
            "fenced_code_blocks",
            "fenced_divs",
            "footnotes",
            "grid_tables",
            "header_attributes",
            "table_attributes",
            "implicit_figures",
            "implicit_header_references",
            "inline_code_attributes",
            "inline_notes",
            "intraword_underscores",
            "latex_macros",
            "line_blocks",
            "link_attributes",
            "markdown_in_html_blocks",
            "multiline_tables",
            "native_divs",
            "native_spans",
            "pandoc_title_block",
            "pipe_tables",
            "raw_attribute",
            "raw_html",
            "raw_tex",
            "shortcut_reference_links",
            "simple_tables",
            "smart",
            "space_in_atx_header",
            "startnum",
            "strikeout",
            "subscript",
            "superscript",
            "table_captions",
            "task_lists",
            "tex_math_dollars",
            "yaml_metadata_block",
        ];
        let expected_off = [
            "abbreviations",
            "alerts",
            "angle_brackets_escapable",
            "ascii_identifiers",
            "attributes",
            "autolink_bare_uris",
            "east_asian_line_breaks",
            "emoji",
            "four_space_rule",
            "gfm_auto_identifiers",
            "gutenberg",
            "hard_line_breaks",
            "ignore_line_breaks",
            "lists_without_preceding_blankline",
            "literate_haskell",
            "mark",
            "markdown_attribute",
            "mmd_header_identifiers",
            "mmd_link_attributes",
            "mmd_title_block",
            "old_dashes",
            "rebase_relative_paths",
            "short_subsuperscripts",
            "sourcepos",
            "spaced_reference_links",
            "tex_math_double_backslash",
            "tex_math_gfm",
            "tex_math_single_backslash",
            "wikilinks_title_after_pipe",
            "wikilinks_title_before_pipe",
        ];

        for name in expected_on {
            let extension = Extension::from_name(name).unwrap();
            assert!(set.contains(extension), "{name} should be on by default");
        }
        for name in expected_off {
            let extension = Extension::from_name(name).unwrap();
            assert!(!set.contains(extension), "{name} should be off by default");
        }
        assert_eq!(set.len(), expected_on.len());
    }

    #[test]
    fn gfm_defaults_match_pandoc() {
        let set = Flavor::Gfm.default_extensions();
        for (name, on) in [
            ("alerts", true),
            ("emoji", true),
            ("gfm_auto_identifiers", true),
            ("task_lists", true),
            ("tex_math_gfm", true),
            ("subscript", false),
            ("footnotes", true),
            ("citations", false),
            ("fenced_divs", false),
        ] {
            let extension = Extension::from_name(name).unwrap();
            assert_eq!(
                set.contains(extension),
                on,
                "{name} default for gfm should be {on}"
            );
        }
    }

    #[test]
    fn set_operations_togle_extensions() {
        let set = ExtensionSet::none()
            .enable(Extension::Citations)
            .enable(Extension::Footnotes);
        assert!(set.contains(Extension::Citations));
        assert!(set.contains(Extension::Footnotes));
        assert!(!set.contains(Extension::Strikeout));

        let set = set.disable(Extension::Footnotes);
        assert!(!set.contains(Extension::Footnotes));
        assert_eq!(set.len(), 1);
        assert!(ExtensionSet::none().is_empty());
    }

    #[test]
    fn format_spec_parses_flavors_and_diffs() {
        let spec = parse_format_spec("markdown").unwrap();
        assert_eq!(spec.flavor, Flavor::Markdown);
        assert_eq!(spec.extensions.len(), 47);

        let spec = parse_format_spec("markdown-smart").unwrap();
        assert!(!spec.extensions.contains(Extension::Smart));

        let spec = parse_format_spec("markdown+citations-smart").unwrap();
        assert!(spec.extensions.contains(Extension::Citations));
        assert!(!spec.extensions.contains(Extension::Smart));

        let spec = parse_format_spec("markdown-footnotes").unwrap();
        assert!(!spec.extensions.contains(Extension::Footnotes));

        let spec = parse_format_spec("gfm").unwrap();
        assert!(spec.extensions.contains(Extension::TaskLists));
        assert!(spec.extensions.contains(Extension::GfmAutoIdentifiers));
        assert!(!spec.extensions.contains(Extension::Citations));

        let spec = parse_format_spec("markdown+tex_math_dollars").unwrap();
        assert!(spec.extensions.contains(Extension::TexMathDollars));
        // Like pandoc, a '-' inside a diff name ends the token, so
        // `tex-math-dollars` is NOT a valid spec.
        assert!(parse_format_spec("markdown+tex-math-dollars").is_err());

        assert!(matches!(
            parse_format_spec("nope+foo"),
            Err(ParseFormatSpecError::UnknownFlavor { .. })
        ));
        assert!(matches!(
            parse_format_spec("markdown+does_not_exist"),
            Err(ParseFormatSpecError::UnknownExtension(_))
        ));
    }

    #[test]
    fn diff_from_renders_pandoc_style_diff() {
        let base = Flavor::Markdown.default_extensions();
        let set = base
            .disable(Extension::Smart)
            .enable(Extension::AsciiIdentifiers);
        assert_eq!(set.diff_from(base), "+ascii_identifiers-smart");

        // Round trip through the renderer.
        let spec = parse_format_spec(&format!("markdown{}", set.diff_from(base))).unwrap();
        assert_eq!(spec.extensions, set);
    }

    #[test]
    fn extension_config_resolves_overrides() {
        let config: ExtensionConfig = serde_json::from_str(
            r#"{"flavor": "gfm", "enabled": ["citations", "fenced-divs"], "disabled": ["emoji"]}"#,
        )
        .unwrap();
        let (set, unknown) = config.resolve();
        assert!(unknown.is_empty());
        assert!(set.contains(Extension::Citations));
        assert!(set.contains(Extension::FencedDivs));
        assert!(!set.contains(Extension::Emoji));
        assert!(set.contains(Extension::TaskLists));

        let config: ExtensionConfig =
            serde_json::from_str(r#"{"format": "markdown+citations-smart"}"#).unwrap();
        let (set, unknown) = config.resolve();
        assert!(unknown.is_empty());
        assert!(set.contains(Extension::Citations));
        assert!(!set.contains(Extension::Smart));

        let config: ExtensionConfig =
            serde_json::from_str(r#"{"enabled": ["bogus_one", "footnotes"]}"#).unwrap();
        let (set, unknown) = config.resolve();
        assert_eq!(unknown.len(), 1);
        assert_eq!(unknown[0].name, "bogus_one");
        assert!(set.contains(Extension::Footnotes));
    }

    #[test]
    fn config_rejects_unknown_fields() {
        assert!(serde_json::from_str::<ExtensionConfig>(r#"{"bogus": 1}"#).is_err());
    }

    #[test]
    fn every_extension_has_a_description() {
        for extension in Extension::ALL {
            assert!(
                !extension.description().is_empty(),
                "{} needs a description",
                extension.name()
            );
        }
    }

    /// Ground-truth check against a locally installed pandoc. Skipped when
    /// pandoc is not on PATH.
    #[test]
    fn defaults_agree_with_installed_pandoc() {
        let output = std::process::Command::new("pandoc")
            .arg("--version")
            .output();
        if output.is_err() || !output.unwrap().status.success() {
            eprintln!("skipping: pandoc not installed");
            return;
        }

        for flavor in [
            Flavor::Markdown,
            Flavor::Gfm,
            Flavor::CommonMark,
            Flavor::CommonMarkX,
            Flavor::MarkdownStrict,
            Flavor::MarkdownMmd,
            Flavor::MarkdownPhpextra,
        ] {
            let output = std::process::Command::new("pandoc")
                .arg(format!("--list-extensions={}", flavor.name()))
                .output()
                .expect("pandoc --list-extensions");
            assert!(output.status.success(), "pandoc failed for {}", flavor);
            let text = String::from_utf8(output.stdout).unwrap();
            let mut mismatches = Vec::new();
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let enabled = line.starts_with('+');
                let name = &line[1..];
                let Some(extension) = Extension::from_name(name) else {
                    mismatches.push(format!("pandoc knows {name} but we do not"));
                    continue;
                };
                if Flavor::default_extensions(flavor).contains(extension) != enabled {
                    mismatches.push(format!("{name} default mismatch for {}", flavor));
                }
            }
            assert!(
                mismatches.is_empty(),
                "{}: {}",
                flavor,
                mismatches.join("; ")
            );
        }
    }
}
