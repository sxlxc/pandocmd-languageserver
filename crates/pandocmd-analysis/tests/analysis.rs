//! Integration tests for extension-aware document analysis.
//!
//! Many expectations are verified against the Pandoc User's Guide and
//! `pandoc` 3.x behavior (see `identifiers.rs` for automated ground-truth
//! slug comparisons against an installed pandoc).

use pandocmd_analysis::{AnalyzeOptions, DocumentAnalysis, WorkspaceIndex};
use pandocmd_extensions::{Extension, ExtensionSet, Flavor};
use pandocmd_syntax::PandocMarkdownParser;

fn analyze(text: &str) -> DocumentAnalysis {
    analyze_with(text, AnalyzeOptions::default())
}

fn analyze_with(text: &str, options: AnalyzeOptions) -> DocumentAnalysis {
    let mut parser = PandocMarkdownParser::new().unwrap();
    let document = parser.parse(text).unwrap();
    DocumentAnalysis::analyze(&document, &WorkspaceIndex::empty(), &options)
}

fn markdown_extensions() -> ExtensionSet {
    ExtensionSet::flavor_defaults(Flavor::Markdown)
}

fn codes(analysis: &DocumentAnalysis) -> Vec<&'static str> {
    analysis
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
}

fn disabled_for(analysis: &DocumentAnalysis, extension: Extension) -> usize {
    analysis
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.extension == Some(extension.name()))
        .count()
}

// ---------------------------------------------------------------- symbols

#[test]
fn extracts_writing_symbols_and_diagnostics() {
    let analysis =
        analyze("# Intro\n\nSee [the docs][docs] and [missing][nope].\n\n[^a]\n\n[docs]: https://example.com\n[^a]: Footnote\n");
    assert_eq!(analysis.headings[0].anchor.as_deref(), Some("intro"));
    assert_eq!(analysis.reference_definitions[0].label, "docs");
    assert_eq!(analysis.footnote_definitions[0].label, "a");
    assert!(codes(&analysis).contains(&"unresolved-reference"));
}

#[test]
fn extracts_fenced_divs_and_resolves_div_anchors() {
    let analysis = analyze("# Intro\n\n::: {#panel .note key=\"two words\"}\nContent.\n:::\n\n::: warning\nBody.\n:::\n\nSee [panel](#panel) and [intro](#intro).\n");
    assert_eq!(analysis.fenced_divs.len(), 2);
    assert_eq!(analysis.fenced_divs[0].id.as_deref(), Some("panel"));
    assert_eq!(analysis.fenced_divs[0].classes, vec!["note"]);
    assert_eq!(analysis.fenced_divs[0].attributes[0].key, "key");
    assert_eq!(
        analysis.fenced_divs[0].attributes[0].value.as_deref(),
        Some("two words")
    );
    assert_eq!(analysis.fenced_divs[1].classes, vec!["warning"]);
    assert!(!codes(&analysis).contains(&"unresolved-heading"));
}

#[test]
fn fenced_div_attributes_are_order_independent() {
    let attribute_sets = [
        r#"#prob-mii .problem"#,
        r#".problem #prob-mii"#,
        r#"#prob-mii .problem title="MII 5\" theorem""#,
        r#"#prob-mii title="MII 5\" theorem" .problem"#,
        r#".problem #prob-mii title="MII 5\" theorem""#,
        r#".problem title="MII 5\" theorem" #prob-mii"#,
        r#"title="MII 5\" theorem" #prob-mii .problem"#,
        r#"title="MII 5\" theorem" .problem #prob-mii"#,
    ];

    for attributes in attribute_sets {
        let text =
            format!("::: {{{attributes}}}\nA mixed-integer problem.\n:::\n\nSee [@prob-mii].\n");
        let analysis = analyze(&text);

        assert_eq!(analysis.fenced_divs.len(), 1, "attributes: {attributes}");
        let div = &analysis.fenced_divs[0];
        assert_eq!(
            div.id.as_deref(),
            Some("prob-mii"),
            "attributes: {attributes}"
        );
        assert_eq!(div.classes, ["problem"], "attributes: {attributes}");
        assert!(
            analysis.local_reference("prob-mii").is_some(),
            "attributes: {attributes}"
        );
        assert!(!codes(&analysis).contains(&"unresolved-citation"));
    }
}

#[test]
fn extracts_fenced_div_attributes_before_inline_caption() {
    let caption =
        "Cogirth-strength ratio bounds and resulting deterministic rank-$j$-reduction runtimes.";
    let analysis = analyze(&format!(
        "::: {{.table #tbl:applications}} {caption}\nsome table\n:::\n\nSee [@tbl:applications].\n"
    ));
    assert_eq!(analysis.fenced_divs.len(), 1);
    assert_eq!(
        analysis.fenced_divs[0].id.as_deref(),
        Some("tbl:applications")
    );
    assert_eq!(analysis.fenced_divs[0].classes, vec!["table"]);
    assert_eq!(analysis.fenced_divs[0].title(), Some(caption));
    assert!(analysis.local_reference("tbl:applications").is_some());
}

#[test]
fn diagnoses_fenced_div_structure() {
    let analysis = analyze("# Panel\n\n::: {#panel}\ncontent\n\n:::\n");
    assert!(codes(&analysis).contains(&"duplicate-anchor"));

    let analysis = analyze(":::\n");
    assert!(codes(&analysis).contains(&"unmatched-fenced-div-close"));

    let analysis = analyze(":::: {.note}\n:::\n");
    assert!(codes(&analysis).contains(&"short-fenced-div-close"));
    assert!(codes(&analysis).contains(&"unclosed-fenced-div"));
}

#[test]
fn finds_fenced_div_from_closing_fence() {
    let text = "::: Lemma\ncontent\n:::\n";
    let analysis = analyze(text);
    let closing_offset = text.rfind(":::").unwrap();
    assert!(matches!(
        analysis.symbol_at(closing_offset),
        Some(pandocmd_analysis::SymbolAtOffset::FencedDiv(div)) if div.classes == vec!["Lemma"]
    ));
}

#[test]
fn finds_citation_from_at_sigil() {
    let text = "See [@doe2024].\n";
    let analysis = analyze(text);
    let at_offset = text.find('@').unwrap();
    assert!(matches!(
        analysis.symbol_at(at_offset),
        Some(pandocmd_analysis::SymbolAtOffset::Citation(citation)) if citation.key == "doe2024"
    ));
}

#[test]
fn parses_citation_variants_from_the_manual() {
    // Blah blah [see @doe99, pp. 33-35; also @smith04, chap. 1].
    let analysis = analyze(
        "Blah blah [see @doe99, pp. 33-35; also @smith04, chap. 1].\n\nSmith says @smith04 [-p. 30].\n",
    );
    let keys: Vec<&str> = analysis.citations.iter().map(|c| c.key.as_str()).collect();
    assert!(keys.contains(&"doe99"));
    assert!(keys.contains(&"smith04"));
    assert_eq!(keys.iter().filter(|key| **key == "smith04").count(), 2);
}

#[test]
fn classifies_local_cross_references() {
    let analysis = analyze("# Intro {#sec-custom}\n\n::: {#thm-main .theorem title=\"Main theorem\"}\nContent.\n:::\n\n![Plot](plot.png){#plot}\n\n```{#lst-demo .rust}\nfn main() {}\n```\n\n$$ x = y $$ {#eq-main}\n\n[Term]{#span-term}\n\nSee [@plot] and [Plot](#plot).\n\n[^note]: Footnote\n");
    let references: std::collections::HashMap<_, _> = analysis
        .local_references
        .iter()
        .map(|reference| (reference.id.clone(), reference.detail.clone()))
        .collect();

    assert_eq!(
        references.get("sec-custom").map(String::as_str),
        Some("section")
    );
    assert_eq!(
        references.get("thm-main").map(String::as_str),
        Some("theorem: Main theorem")
    );
    assert_eq!(references.get("plot").map(String::as_str), Some("figure"));
    assert_eq!(
        references.get("lst-demo").map(String::as_str),
        Some("listing")
    );
    assert_eq!(
        references.get("eq-main").map(String::as_str),
        Some("equation")
    );
    assert_eq!(
        references.get("span-term").map(String::as_str),
        Some("span")
    );
    assert!(!references.contains_key("note"));
    assert_eq!(
        analysis.local_reference_ranges_for_id("plot").len(),
        3,
        "target id, [@plot], and (#plot) should all be indexed"
    );
}

#[test]
fn treats_pandoc_at_references_to_local_labels_as_resolved() {
    let mut workspace = WorkspaceIndex::empty();
    workspace.add_bibliography_text("@article{doe2024,\n title = {T}\n}\n");
    let mut parser = PandocMarkdownParser::new().unwrap();
    let document = parser
        .parse("# Intro {#sec-intro}\n\n::: {#panel .note}\nContent.\n:::\n\n```{#lst-demo .rust}\nfn main() {}\n```\n\n![Plot](plot.png){#fig-plot}\n\n[Term]{#span-term}\n\nSee [@sec-intro], [@panel], [@lst-demo], [@fig-plot], [@span-term], and [@missing].\n")
        .unwrap();
    let analysis = DocumentAnalysis::analyze(&document, &workspace, &AnalyzeOptions::default());
    assert_eq!(
        analysis
            .diagnostics
            .iter()
            .filter(|d| d.code == "unresolved-citation")
            .count(),
        1
    );
    assert!(analysis
        .diagnostics
        .iter()
        .any(|d| d.message == "unresolved citation `@missing`"));
}

#[test]
fn footnote_definitions_do_not_resolve_citations() {
    let mut workspace = WorkspaceIndex::empty();
    workspace.add_bibliography_text("@article{real,\n title = {T}\n}\n");
    let mut parser = PandocMarkdownParser::new().unwrap();
    let document = parser.parse("[^note]: Footnote\n\nSee [@note].\n").unwrap();
    let analysis = DocumentAnalysis::analyze(&document, &workspace, &AnalyzeOptions::default());
    assert!(analysis
        .diagnostics
        .iter()
        .any(|d| d.message == "unresolved footnote `note`"
            || d.message == "unresolved citation `@note`"));
}

#[test]
fn inline_notes_are_indexed() {
    let analysis = analyze("Here is an inline note.^[Inline notes are nice.]\n");
    assert_eq!(analysis.inline_notes.len(), 1);
    assert_eq!(analysis.inline_notes[0].content, "Inline notes are nice.");
    assert!(codes(&analysis).is_empty());
}

// ---------------------------------------------------- code & metadata masking

#[test]
fn code_blocks_hide_every_inline_construct() {
    let analysis = analyze(
        "```rust\nlet citation = \"[@key]\"; let footnote = \"[^a]\";\n::: {.note}\n~~gone~~\n$math$\n```\n",
    );
    assert!(analysis.citations.is_empty());
    assert!(analysis.footnote_references.is_empty());
    assert!(analysis.fenced_divs.is_empty());
    assert!(codes(&analysis).is_empty());
}

#[test]
fn tilde_code_blocks_are_also_opaque() {
    let analysis = analyze("~~~\n[@hidden]\n~~~\n");
    assert!(analysis.citations.is_empty());
}

#[test]
fn inline_code_spans_hide_citations_and_footnotes() {
    let analysis = analyze("Use `[@key]` and `[^a]` literally in code spans.\n");
    assert!(analysis.citations.is_empty());
    assert!(analysis.footnote_references.is_empty());
}

#[test]
fn yaml_metadata_block_is_opaque() {
    let analysis = analyze("---\ntitle: \"Note [@not-a-citation]\"\n---\n\nReal [@real].\n");
    let keys: Vec<&str> = analysis.citations.iter().map(|c| c.key.as_str()).collect();
    assert_eq!(keys, ["real"]);
}

#[test]
fn crlf_documents_are_handled() {
    let analysis = analyze("# Intro\r\n\r\nText [@key].\r\n");
    assert_eq!(analysis.headings[0].anchor.as_deref(), Some("intro"));
    assert_eq!(analysis.citations[0].key, "key");
}

// -------------------------------------------------------- extension gating

#[test]
fn disabled_citations_warn_and_skip_analysis() {
    let options =
        AnalyzeOptions::with_extensions(markdown_extensions().disable(Extension::Citations));
    let analysis = analyze_with("See [@doe2024] and @smith04.\n", options);

    assert!(analysis.citations.is_empty());
    assert_eq!(disabled_for(&analysis, Extension::Citations), 2);
    assert!(!codes(&analysis).contains(&"unresolved-citation"));
    assert_eq!(
        analysis.diagnostics[0].code, "extension-disabled",
        "diagnostic code should be extension-disabled"
    );
    assert!(analysis.diagnostics[0]
        .message
        .contains("citations` extension"));
}

#[test]
fn enabled_citations_do_not_warn() {
    let analysis = analyze("See [@doe2024].\n");
    assert!(analysis.diagnostics.is_empty());
}

#[test]
fn disabled_footnotes_warn() {
    let options =
        AnalyzeOptions::with_extensions(markdown_extensions().disable(Extension::Footnotes));
    let analysis = analyze_with("One.[^note]\n\n[^note]: Def.\n", options);

    assert!(analysis.footnote_references.is_empty());
    assert!(analysis.footnote_definitions.is_empty());
    assert_eq!(disabled_for(&analysis, Extension::Footnotes), 2);
    assert!(!codes(&analysis).contains(&"unresolved-footnote"));
}

#[test]
fn disabled_inline_notes_warn() {
    let options =
        AnalyzeOptions::with_extensions(markdown_extensions().disable(Extension::InlineNotes));
    let analysis = analyze_with("Note.^[hidden]\n", options);
    assert_eq!(disabled_for(&analysis, Extension::InlineNotes), 1);
}

#[test]
fn disabled_fenced_divs_warn() {
    let options =
        AnalyzeOptions::with_extensions(markdown_extensions().disable(Extension::FencedDivs));
    let analysis = analyze_with("::: {.warning}\nBe careful.\n:::\n", options);

    assert!(analysis.fenced_divs.is_empty());
    assert_eq!(disabled_for(&analysis, Extension::FencedDivs), 2);
}

#[test]
fn disabled_header_attributes_keep_literal_text() {
    let options =
        AnalyzeOptions::with_extensions(markdown_extensions().disable(Extension::HeaderAttributes));
    let analysis = analyze_with("# Heading {#custom}\n", options);

    // Verified against pandoc: the slug of "# Heading {#custom}" with
    // header_attributes off is "heading-custom".
    assert_eq!(
        analysis.headings[0].anchor.as_deref(),
        Some("heading-custom")
    );
    assert_eq!(
        analysis.headings[0].identifier_source,
        pandocmd_analysis::IdentifierSource::Auto
    );
    assert_eq!(disabled_for(&analysis, Extension::HeaderAttributes), 1);
}

#[test]
fn no_identifier_extensions_means_no_heading_anchor() {
    let options = AnalyzeOptions::with_extensions(
        markdown_extensions()
            .disable(Extension::AutoIdentifiers)
            .disable(Extension::GfmAutoIdentifiers),
    );
    let analysis = analyze_with("# Heading\n\nSee [Heading](#heading).\n", options);

    assert_eq!(analysis.headings[0].anchor, None);
    assert_eq!(
        analysis.headings[0].identifier_source,
        pandocmd_analysis::IdentifierSource::None
    );
    // The link target does not exist anymore.
    assert!(codes(&analysis).contains(&"unresolved-heading"));
}

#[test]
fn explicit_header_attributes_win_and_are_not_uniquified() {
    let analysis = analyze("# A {#same}\n\n# B {#same}\n");
    assert_eq!(analysis.headings[0].anchor.as_deref(), Some("same"));
    assert_eq!(analysis.headings[1].anchor.as_deref(), Some("same"));
    assert!(codes(&analysis).contains(&"duplicate-anchor"));
}

#[test]
fn automatic_identifiers_are_uniquified_like_pandoc() {
    let analysis = analyze("# Dup\n\n# Dup\n\n# Dup\n");
    let anchors: Vec<Option<&str>> = analysis
        .headings
        .iter()
        .map(|heading| heading.anchor.as_deref())
        .collect();
    assert_eq!(anchors, [Some("dup"), Some("dup-1"), Some("dup-2")]);
}

#[test]
fn gfm_preset_uses_github_identifiers() {
    let options = AnalyzeOptions::with_extensions(ExtensionSet::flavor_defaults(Flavor::Gfm));
    let analysis = analyze_with("# 1. Introduction\n\n# 42 answer\n", options);
    assert_eq!(
        analysis.headings[0].anchor.as_deref(),
        Some("1-introduction")
    );
    assert_eq!(analysis.headings[1].anchor.as_deref(), Some("42-answer"));
}

#[test]
fn ascii_identifiers_fold_accents_in_headings() {
    let options =
        AnalyzeOptions::with_extensions(markdown_extensions().enable(Extension::AsciiIdentifiers));
    let analysis = analyze_with("# Müller\n", options);
    assert_eq!(analysis.headings[0].anchor.as_deref(), Some("muller"));
}

#[test]
fn smart_off_keeps_dots_in_identifiers() {
    let options = AnalyzeOptions::with_extensions(markdown_extensions().disable(Extension::Smart));
    let analysis = analyze_with("# Hello ... world\n", options);
    assert_eq!(
        analysis.headings[0].anchor.as_deref(),
        Some("hello-...-world")
    );
}

#[test]
fn disabled_task_lists_warn() {
    let options =
        AnalyzeOptions::with_extensions(markdown_extensions().disable(Extension::TaskLists));
    let analysis = analyze_with("- [ ] todo\n- [x] done\n", options);
    assert_eq!(disabled_for(&analysis, Extension::TaskLists), 2);
}

#[test]
fn enabled_task_lists_do_not_warn() {
    let analysis = analyze("- [ ] todo\n- [x] done\n");
    assert!(analysis.diagnostics.is_empty());
}

#[test]
fn disabled_inline_styling_constructs_warn() {
    let options = AnalyzeOptions::with_extensions(
        markdown_extensions()
            .disable(Extension::Strikeout)
            .disable(Extension::Superscript)
            .disable(Extension::Subscript)
            .disable(Extension::Mark),
    );
    let analysis = analyze_with("~~gone~~ and H~2~O and 2^10^ bits ==marked==.\n", options);
    assert_eq!(disabled_for(&analysis, Extension::Strikeout), 1);
    assert_eq!(disabled_for(&analysis, Extension::Subscript), 1);
    assert_eq!(disabled_for(&analysis, Extension::Superscript), 1);
    assert_eq!(disabled_for(&analysis, Extension::Mark), 1);
}

#[test]
fn disabled_math_constructs_warn_but_currency_does_not() {
    let options = AnalyzeOptions::with_extensions(
        markdown_extensions()
            .disable(Extension::TexMathDollars)
            .disable(Extension::TexMathSingleBackslash)
            .disable(Extension::TexMathDoubleBackslash)
            .disable(Extension::TexMathGfm),
    );
    let analysis = analyze_with(
        "Inline $x^2$ math, display $$y = 1$$, backslash \\(z\\), and gfm $`q`$.\n\nCosts $5 and $6 dollars, times 12:30:45.\n",
        options,
    );
    assert_eq!(disabled_for(&analysis, Extension::TexMathDollars), 2);
    assert_eq!(
        disabled_for(&analysis, Extension::TexMathSingleBackslash),
        1
    );
    assert_eq!(disabled_for(&analysis, Extension::TexMathGfm), 1);
}

#[test]
fn disabled_tables_warn() {
    let options = AnalyzeOptions::with_extensions(
        markdown_extensions()
            .disable(Extension::PipeTables)
            .disable(Extension::GridTables),
    );
    let analysis = analyze_with(
        "| a | b |\n|---|---|\n| 1 | 2 |\n\n+-----+-----+\n| g   | t   |\n+-----+-----+\n",
        options,
    );
    assert_eq!(disabled_for(&analysis, Extension::PipeTables), 1);
    assert_eq!(disabled_for(&analysis, Extension::GridTables), 2);
}

#[test]
fn enabled_tables_do_not_warn() {
    let analysis = analyze("| a | b |\n|---|---|\n| 1 | 2 |\n");
    assert!(analysis.diagnostics.is_empty());
}

#[test]
fn thematic_breaks_are_not_pipe_tables() {
    let options =
        AnalyzeOptions::with_extensions(markdown_extensions().disable(Extension::PipeTables));
    let analysis = analyze_with("Above\n\n---\n\n- - -\n", options);
    assert_eq!(disabled_for(&analysis, Extension::PipeTables), 0);
}

#[test]
fn disabled_alerts_warn() {
    let options = AnalyzeOptions::with_extensions(markdown_extensions().disable(Extension::Alerts));
    let analysis = analyze_with("> [!NOTE]\n> Helpful hint.\n", options);
    assert_eq!(disabled_for(&analysis, Extension::Alerts), 1);
}

#[test]
fn disabled_example_lists_and_emoji_warn() {
    let options = AnalyzeOptions::with_extensions(
        markdown_extensions()
            .disable(Extension::ExampleLists)
            .disable(Extension::Emoji),
    );
    let analysis = analyze_with("(@good) First example.\n\nA :smile: emoji.\n", options);
    assert_eq!(disabled_for(&analysis, Extension::ExampleLists), 1);
    assert_eq!(disabled_for(&analysis, Extension::Emoji), 1);
}

#[test]
fn times_are_not_emoji() {
    let options = AnalyzeOptions::with_extensions(markdown_extensions().disable(Extension::Emoji));
    let analysis = analyze_with("The train leaves 12:30:45 sharp.\n", options);
    assert_eq!(disabled_for(&analysis, Extension::Emoji), 0);
}

#[test]
fn disabled_definition_lists_hint() {
    let options =
        AnalyzeOptions::with_extensions(markdown_extensions().disable(Extension::DefinitionLists));
    let analysis = analyze_with("Term\n: Definition\n", options);
    assert_eq!(disabled_for(&analysis, Extension::DefinitionLists), 1);
}

#[test]
fn disabled_wikilinks_hint() {
    let options = AnalyzeOptions::with_extensions(
        markdown_extensions().disable(Extension::WikilinksTitleAfterPipe),
    );
    let analysis = analyze_with("See [[Some Page]].\n", options);
    assert_eq!(
        disabled_for(&analysis, Extension::WikilinksTitleAfterPipe),
        1
    );
}

#[test]
fn disabled_raw_attributes_warn() {
    let options =
        AnalyzeOptions::with_extensions(markdown_extensions().disable(Extension::RawAttribute));
    let analysis = analyze_with(
        "```{=html}\n<b>bold</b>\n```\n\nAnd `<i>`{=html}.\n",
        options,
    );
    assert_eq!(disabled_for(&analysis, Extension::RawAttribute), 2);
}

#[test]
fn disabled_bracketed_spans_warn() {
    let options =
        AnalyzeOptions::with_extensions(markdown_extensions().disable(Extension::BracketedSpans));
    let analysis = analyze_with("[marked]{.kw}\n", options);
    assert_eq!(disabled_for(&analysis, Extension::BracketedSpans), 1);
}

#[test]
fn shortcut_reference_links_respect_the_extension() {
    let text = "See [docs].\n\n[docs]: https://example.com\n";

    let enabled = analyze(text);
    assert!(enabled
        .reference_links
        .iter()
        .any(|link| link.label == "docs"));

    let options = AnalyzeOptions::with_extensions(
        markdown_extensions().disable(Extension::ShortcutReferenceLinks),
    );
    let disabled = analyze_with(text, options);
    assert!(disabled.reference_links.is_empty());
}

#[test]
fn implicit_header_references_resolve_link_labels() {
    let text = "# Getting Started\n\nBack to [Getting Started][].\n";

    let enabled = analyze(text);
    assert!(!codes(&enabled).contains(&"unresolved-reference"));

    let options = AnalyzeOptions::with_extensions(
        markdown_extensions().disable(Extension::ImplicitHeaderReferences),
    );
    let disabled = analyze_with(text, options);
    assert!(codes(&disabled).contains(&"unresolved-reference"));
}

#[test]
fn unresolved_diagnostics_can_be_turned_off() {
    let options = AnalyzeOptions {
        unresolved_references: false,
        ..AnalyzeOptions::default()
    };
    let analysis = analyze_with("See [missing][nope].\n", options);
    assert!(!codes(&analysis).contains(&"unresolved-reference"));
}

#[test]
fn disabled_extension_diagnostics_can_be_turned_off() {
    let options = AnalyzeOptions {
        disabled_extensions: false,
        ..AnalyzeOptions::with_extensions(markdown_extensions().disable(Extension::Footnotes))
    };
    let analysis = analyze_with("One.[^note]\n", options);
    assert!(analysis.diagnostics.is_empty());
}

#[test]
fn space_in_atx_header_disabled_accepts_hashword_headings() {
    // Verified: pandoc -f markdown-space_in_atx_header parses "#Heading".
    let options =
        AnalyzeOptions::with_extensions(markdown_extensions().disable(Extension::SpaceInAtxHeader));
    let analysis = analyze_with("#Heading\n", options);
    assert_eq!(analysis.headings[0].title, "Heading");
}

#[test]
fn space_in_atx_header_enabled_rejects_hashword_headings() {
    let analysis = analyze("#Heading\n");
    assert!(analysis.headings.is_empty());
}

// --------------------------------------------------------------- links

#[test]
fn extracts_links_for_document_link_providers() {
    let analysis = analyze(
        "Visit [the docs](https://example.com) or <mailto:a@b.c> or ![img](pic.png \"Title\").\n\n[ref]: https://ref.example \"Ref\"\n",
    );
    let kinds: Vec<_> = analysis.links.iter().map(|link| link.kind).collect();
    assert_eq!(
        kinds,
        [
            pandocmd_analysis::LinkKind::Inline,
            pandocmd_analysis::LinkKind::Autolink,
            pandocmd_analysis::LinkKind::Image,
            pandocmd_analysis::LinkKind::Definition,
        ]
    );
    assert_eq!(analysis.links[0].target, "https://example.com");
    assert_eq!(analysis.links[0].label.as_deref(), Some("the docs"));
    assert_eq!(analysis.links[1].target, "mailto:a@b.c");
    assert_eq!(analysis.links[2].target, "pic.png");
    assert_eq!(analysis.links[3].target, "https://ref.example");
}

#[test]
fn heading_links_are_indexed() {
    let analysis = analyze("# Target\n\nSee [Target](#target).\n");
    assert_eq!(analysis.heading_links.len(), 1);
    assert_eq!(analysis.heading_links[0].anchor, "target");
    assert!(!codes(&analysis).contains(&"unresolved-heading"));
}

// -------------------------------------------------------- semantic tokens

#[test]
fn semantic_tokens_cover_core_constructs() {
    let analysis = analyze(
        "# Head\n\n::: {.note}\n\n```rust\nfn x() {}\n```\n\nMath $x$ and $$y$$.\n\nCite [@k].\n\nNote.^[inline]\n:::\n",
    );
    let token_names: Vec<&str> = analysis
        .semantic_tokens
        .iter()
        .map(|token| token.kind.name())
        .collect();
    for expected in [
        "heading",
        "fencedDiv",
        "codeFence",
        "math",
        "citation",
        "footnote",
    ] {
        assert!(
            token_names.contains(&expected),
            "missing token {expected} in {token_names:?}"
        );
    }
}

// ---------------------------------------------------------- bibliography

#[test]
fn bibliography_keys_resolve_citations() {
    let text = "See [@doe2024].\n";
    let mut parser = PandocMarkdownParser::new().unwrap();
    let document = parser.parse(text).unwrap();
    let mut workspace = WorkspaceIndex::empty();
    workspace.add_bibliography_text("@article{doe2024,\n title = {T}\n}\n");
    let analysis = DocumentAnalysis::analyze(&document, &workspace, &AnalyzeOptions::default());
    assert!(analysis.diagnostics.is_empty());
}

#[test]
fn slugifies_headings_compat_helper() {
    assert_eq!(
        pandocmd_analysis::slugify_heading("Hello, Pandoc Markdown!"),
        "hello-pandoc-markdown"
    );
}
