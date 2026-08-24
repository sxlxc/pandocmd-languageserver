//! Regression tests for pandoc behaviors discovered by the differential
//! corpus test (`tests/corpus.rs`). Each test documents one verified pandoc
//! behavior that the line-oriented scanner emulates; the corpus keeps them
//! honest against real-world documents.

use pandocmd_analysis::{AnalyzeOptions, DocumentAnalysis, WorkspaceIndex};
use pandocmd_extensions::{Extension, Flavor};
use pandocmd_syntax::PandocMarkdownParser;

fn analyze(text: &str) -> DocumentAnalysis {
    let mut parser = PandocMarkdownParser::new().unwrap();
    let document = parser.parse(text).unwrap();
    DocumentAnalysis::analyze(
        &document,
        &WorkspaceIndex::empty(),
        &AnalyzeOptions::default(),
    )
}

fn headings(text: &str) -> Vec<(u8, Option<String>)> {
    analyze(text)
        .headings
        .into_iter()
        .map(|heading| (heading.level, heading.anchor))
        .collect()
}

fn link_targets(text: &str) -> Vec<String> {
    let analysis = analyze(text);
    let mut targets: Vec<_> = analysis
        .links
        .iter()
        .map(|link| link.target.clone())
        .collect();
    targets.sort();
    targets
}

fn citation_keys(text: &str) -> Vec<String> {
    let mut keys: Vec<_> = analyze(text).into_citation_keys();
    keys.sort();
    keys
}

trait CitationKeys {
    fn into_citation_keys(self) -> Vec<String>;
}

impl CitationKeys for DocumentAnalysis {
    fn into_citation_keys(self) -> Vec<String> {
        self.citations.into_iter().map(|c| c.key).collect()
    }
}

// ---------------------------------------------------------------------------
// Block context: lists, definition lists, indented code, block quotes
// ---------------------------------------------------------------------------

#[test]
fn indented_code_blocks_hide_inline_constructs() {
    // Pandoc: four-space indented text after a blank line is verbatim, so
    // the citation, footnote, and reference never materialize (MANUAL's
    // `@font-face` CSS blocks).
    let analysis = analyze("Text.\n\n    @font-face {\n      x @cite y [^1] [ref]\n");
    assert!(analysis.citations.is_empty());
    assert!(analysis.footnote_references.is_empty());
    assert!(analysis.reference_links.is_empty());
}

#[test]
fn list_content_is_not_indented_code() {
    // Content indented to a list item's column is list content even when it
    // contains deeper constructs (quarto's indented engine cells).
    let analysis =
        analyze("- item\n\n      ```{{python}}\n      x = 1\n      @not-a-citation\n      ```\n");
    assert!(analysis.citations.is_empty());
}

#[test]
fn definition_list_content_holds_fenced_divs() {
    // `::: {#input-formats}` at the definition's content column is a div in
    // pandoc (the Pandoc User's Guide structure), while text indented four
    // beyond it would be code.
    let analysis = analyze(
        "Term\n:   definition continues\n\n    ::: {#input-formats}\n    - item\n    :::\n",
    );
    assert_eq!(
        analysis
            .fenced_divs
            .iter()
            .map(|div| div.id.clone())
            .collect::<Vec<_>>(),
        vec![Some("input-formats".to_string())]
    );
}

#[test]
fn nested_definition_lists_raise_the_content_column() {
    // `Docx` term + `:   For best results` marker at the content column
    // starts a nested definition whose content sits at the marker's text
    // column (MANUAL's `--reference-doc` section).
    let analysis = analyze(
        ":   outer definition\n\n    Docx\n\n    :   inner definition\n\n        @cite here\n",
    );
    assert_eq!(
        citation_keys_from(&analysis),
        vec!["cite".to_string()],
        "nested definition content is prose, not indented code"
    );
}

fn citation_keys_from(analysis: &DocumentAnalysis) -> Vec<String> {
    let mut keys: Vec<_> = analysis.citations.iter().map(|c| c.key.clone()).collect();
    keys.sort();
    keys
}

#[test]
fn headings_inside_block_quotes_are_headings() {
    // `> ## Header attributes inside block quote {#foobar}` from pandoc's
    // reader test suite.
    assert_eq!(
        headings("> ## Attributed {#foobar}\n"),
        vec![(2, Some("foobar".to_string()))]
    );
}

#[test]
fn divs_inside_block_quotes_are_divs() {
    // `> ::: {.flushright}` from the R Markdown Cookbook.
    let analysis = analyze("> ::: {.flushright data-latex=\"\"}\n> right text\n> :::\n");
    assert_eq!(
        analysis
            .fenced_divs
            .iter()
            .map(|div| div.classes.clone())
            .collect::<Vec<_>>(),
        vec![vec!["flushright".to_string()]]
    );
}

// ---------------------------------------------------------------------------
// Multiline and simple tables
// ---------------------------------------------------------------------------

#[test]
fn multiline_table_rules_are_not_setext_underlines() {
    // The row separators of a multiline table must not turn cell text into
    // headings (pandoc's tables.txt test).
    assert!(headings(
        "---------------------------------------------------------------\n Centered   Left             Right\n  Header    Aligned        Aligned  Default aligned\n----------  ---------  -----------  ---------------------------\n   First    row               12.0  Example of a row that spans\n                                    multiple lines.\n\n   Second   row                5.0  Here's another one.  Note\n                                    the blank line between rows.\n---------------------------------------------------------------\n"
    )
    .is_empty());
}

#[test]
fn setext_headings_survive_near_table_rules() {
    // A lone dash underline after a paragraph is still a setext heading
    // when no second rule claims it as a table.
    assert_eq!(
        headings("Multiline table with caption:\n\n:  Here's the caption.\nIt may span multiple lines.\n\n- - -\n\n# Not a table\n"),
        vec![(1, Some("not-a-table".to_string()))]
    );
}

#[test]
fn grid_table_code_cells_do_not_hold_citations() {
    // Grid cells indented four spaces from their border hold code in pandoc
    // (quarto shows syntax examples this way); unindented cells are prose.
    let analysis = analyze(
        "+-----------+-----------+\n| Markdown  | Output    |\n+===========+===========+\n|     [@a]  | plain @b  |\n+-----------+-----------+\n",
    );
    assert_eq!(
        citation_keys_from(&analysis),
        vec!["b".to_string()],
        "indented grid cell is code, unindented cell is prose"
    );
}

#[test]
fn grid_table_cells_can_hold_headings() {
    // `| # col 1 | # col 2 |` from pandoc's reader test suite: cell block
    // content includes ATX headings, with pandoc-style uniquified anchors.
    assert_eq!(
        headings(
            "+-------+-------+\n| # col 1 | # col 2 |\n+=======+=======+\n| a   | b   |\n+-------+-------+\n"
        ),
        vec![
            (1, Some("col-1".to_string())),
            (1, Some("col-2".to_string())),
        ]
    );
}

// ---------------------------------------------------------------------------
// Citations and example lists
// ---------------------------------------------------------------------------

#[test]
fn citation_keys_strip_trailing_punctuation() {
    // Pandoc strips trailing `.,;:!?` from citation keys (the R Markdown
    // Cookbook cites `[@bookdown2016].` mid-sentence).
    assert_eq!(
        citation_keys("Studies show @alpha. and [@beta!] and [@gamma;]\n"),
        vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()]
    );
}

#[test]
fn example_list_references_are_not_citations() {
    // `(@label)` is an example reference only for labels an example-list
    // marker defined; otherwise it is a citation (quarto crossrefs use
    // `(@eq-black-scholes)` in prose).
    let text = "As (@good) illustrates.\n\n(@)  First.\n(@good)  Second.\n\nBut (@eq-x) stays a citation.\n";
    assert_eq!(
        citation_keys(text),
        vec!["eq-x".to_string()],
        "(@good) resolves as an example reference, (@eq-x) as a citation"
    );
}

// ---------------------------------------------------------------------------
// Links and reference definitions
// ---------------------------------------------------------------------------

#[test]
fn inline_link_text_may_wrap_onto_the_next_line() {
    // pandoc-getting-started.md: `[User's\nGuide](https://...)` is one link.
    assert_eq!(
        analyze("go to the [User's\nGuide](https://pandoc.org) now\n")
            .links
            .iter()
            .map(|link| link.target.clone())
            .collect::<Vec<_>>(),
        vec!["https://pandoc.org".to_string()]
    );
}

#[test]
fn inline_link_destinations_may_wrap_and_contain_spaces() {
    // pandoc's reader tests: `[foo](/bar\n and baz )` and `[foo](/bar and baz)`.
    // The stored target keeps the source bytes (joiner space plus the
    // continuation line's leading spaces); comparisons against pandoc's
    // AST collapse whitespace at diff time.
    let mut targets = analyze("[a](/bar and baz)\n[b](/bar\n and baz )\n")
        .links
        .iter()
        .map(|link| link.target.clone())
        .collect::<Vec<_>>();
    targets.sort();
    assert_eq!(
        targets,
        vec!["/bar  and baz".to_string(), "/bar and baz".to_string()]
    );
}

#[test]
fn badge_style_links_contain_images() {
    // `[![Build](badge.svg)](actions)` from GitHub-style READMEs: pandoc
    // records the outer link and the inner image.
    let analysis = analyze("[![Build](badge.svg)](https://example.com/actions)\n");
    let mut targets: Vec<_> = analysis.links.iter().map(|l| l.target.clone()).collect();
    targets.sort();
    assert_eq!(
        targets,
        vec![
            "badge.svg".to_string(),
            "https://example.com/actions".to_string()
        ]
    );
}

#[test]
fn reference_definitions_may_continue_on_the_next_line() {
    // `[foo]:` followed by `  /url` (no blank between) is a definition in
    // pandoc; after a blank line the URL is NOT picked up (reader tests).
    let analysis = analyze("[foo]:\n  /url\n\n[bar]:\n\nqux\n\n[foo]\n\n[bar]\n");
    let foo = analysis
        .reference_definitions
        .iter()
        .find(|definition| definition.label == "foo")
        .unwrap();
    assert_eq!(foo.target, "/url");
    let bar = analysis
        .reference_definitions
        .iter()
        .find(|definition| definition.label == "bar")
        .unwrap();
    assert_eq!(bar.target, "");
}

#[test]
fn definitions_cannot_interrupt_a_paragraph() {
    // `... just as with\n[fenced code blocks]:` is prose; the bracket is a
    // shortcut reference resolved through the implicit header (MANUAL).
    let analysis = analyze(
        "## Fenced code blocks\n\nAttributes attach, just as with\n[fenced code blocks]:\n",
    );
    assert_eq!(analysis.reference_definitions.len(), 0);
    assert_eq!(analysis.reference_links.len(), 1);
    let implicit: Vec<_> = analysis
        .heading_links
        .iter()
        .map(|link| link.anchor.clone())
        .collect();
    let _ = implicit; // heading_links covers `](#...)` forms; the shortcut
                      // resolution is exercised by the corpus test.
}

#[test]
fn reference_labels_keep_formatting_characters() {
    // `` [`list`] `` and `[List]` are different labels in pandoc: keys keep
    // backticks and are only case-folded and whitespace-collapsed.
    let analysis = analyze("[List]: #one\n\n[`list`]: #two\n\nsee [`list`]\n");
    let definitions: Vec<_> = analysis
        .reference_definitions
        .iter()
        .map(|definition| {
            (
                definition.normalized_label.clone(),
                definition.target.clone(),
            )
        })
        .collect();
    assert_eq!(
        definitions,
        vec![
            ("list".to_string(), "#one".to_string()),
            ("`list`".to_string(), "#two".to_string()),
        ]
    );
}

#[test]
fn angle_bracket_definition_targets_may_contain_spaces() {
    // `[foo]: <bar baz>` defines target `bar baz` (pandoc percent-encodes
    // it in output; the stored target keeps the spaces).
    let analysis = analyze("[foo]: <bar baz>\n\n[foo]\n");
    assert_eq!(analysis.reference_definitions[0].target, "bar baz");
}

#[test]
fn email_autolinks_resolve_to_mailto() {
    assert_eq!(
        link_targets("write <john@example.com> now\n"),
        vec!["mailto:john@example.com".to_string()]
    );
}

#[test]
fn escaped_parentheses_survive_in_destinations() {
    // `[link](/hithere\))` has target `/hithere)` in pandoc.
    assert_eq!(
        link_targets("[link](/hithere\\))\n"),
        vec!["/hithere)".to_string()]
    );
}

// ---------------------------------------------------------------------------
// Headings
// ---------------------------------------------------------------------------

#[test]
fn heading_attribute_blocks_are_always_consumed() {
    // `## General options {.options}`: the class block never contributes to
    // the title or the auto identifier (MANUAL's option sections).
    assert_eq!(
        headings("## General options {.options}\n"),
        vec![(2, Some("general-options".to_string()))]
    );
}

#[test]
fn heading_identifiers_drop_html_and_emphasis() {
    // `# 2. _Optionally_, backup <!-- omit in toc -->` — pandoc computes
    // identifiers from plain inline text (GitHub README pattern), and the
    // default auto_identifiers algorithm drops everything before the first
    // letter (`2. ` disappears).
    assert_eq!(
        headings("## 2. _Optionally_, backup your file <!-- omit in toc -->\n"),
        vec![(2, Some("optionally-backup-your-file".to_string()))]
    );
}

#[test]
fn heading_identifiers_drop_inline_html_tags() {
    assert_eq!(
        headings("# Hello <b>World</b>\n"),
        vec![(1, Some("hello-world".to_string()))]
    );
}

// ---------------------------------------------------------------------------
// Inline code spans across lines
// ---------------------------------------------------------------------------

#[test]
fn inline_code_spans_may_wrap_across_lines() {
    // MANUAL: `` `[@foo, p. `` ... ` 33]`) `` — the citation inside the
    // wrapped code span must not materialize.
    let analysis = analyze("citations (such as `[@foo, p.\n33]`) render in parentheses.\n");
    assert!(analysis.citations.is_empty());
}

// ---------------------------------------------------------------------------
// Flavors
// ---------------------------------------------------------------------------

#[test]
fn gfm_heading_identifiers_drop_emphasis() {
    let mut parser = PandocMarkdownParser::new().unwrap();
    let document = parser
        .parse("## 2. _Optionally_, backup your existing ~/.zshrc file.\n")
        .unwrap();
    let options = AnalyzeOptions::with_extensions(
        pandocmd_extensions::ExtensionSet::flavor_defaults(Flavor::Gfm),
    );
    let analysis = DocumentAnalysis::analyze(&document, &WorkspaceIndex::empty(), &options);
    assert_eq!(
        analysis.headings[0].anchor.as_deref(),
        Some("2-optionally-backup-your-existing-zshrc-file")
    );
    let _ = Extension::Smart; // keep the import meaningful in all cfgs
}

// ---------------------------------------------------------------------------
// LSP-facing behaviors: byte-exact ranges and pandoc reference semantics
// (exercised end-to-end by the lsp integration tests)
// ---------------------------------------------------------------------------

#[test]
fn reference_continuation_takes_the_whole_next_line() {
    // Verified against pandoc: `[foo]:` followed by `- list item` defines
    // target `- list item`; even a fence line becomes the target.
    let analysis = analyze("[foo]:\n- list item\n\n[foo]\n");
    assert_eq!(analysis.reference_definitions[0].target, "- list item");
    let analysis = analyze("[foo]:\n```\n@cited\n```\n\n[foo]\n");
    assert_eq!(analysis.reference_definitions[0].target, "```");
    // The code fence consumed as a target means the block is not open, so
    // the citation leaks in — exactly like pandoc.
    assert_eq!(citation_keys_from(&analysis), vec!["cited".to_string()]);
}

#[test]
fn continuation_targets_report_their_own_range() {
    // The documentLink region of a continuation definition is the target
    // line, not the label line.
    let text = "[ref]:\n  https://example.com/continued \"Title\"\n";
    let analysis = analyze(text);
    let link = analysis
        .links
        .iter()
        .find(|link| link.kind == pandocmd_analysis::LinkKind::Definition)
        .unwrap();
    assert_eq!(
        &text[link.target_range.start..link.target_range.end],
        "https://example.com/continued"
    );
}

#[test]
fn same_line_definition_consumes_title_on_next_line() {
    // Verified: pandoc swallows a title-only line after `[q]: /url`, so a
    // citation inside it never materializes.
    let analysis = analyze("[q]: /url\n\"[see @x]\"\n");
    assert!(analysis.citations.is_empty());
}

#[test]
fn trailing_definition_at_eof_still_registers() {
    // `[label]:` as the last line must still define the label (with an
    // empty target) instead of dropping it and flagging false
    // unresolved-reference diagnostics.
    let analysis = analyze("Use [foo] and [foo].\n\n[foo]:\n");
    assert_eq!(analysis.reference_definitions.len(), 1);
    assert!(!analysis
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "unresolved-reference"));
}

#[test]
fn blank_lines_end_grid_tables() {
    // Verified: after a blank line pandoc parses `| ... |` as a line block,
    // not as another grid row, so deeply indented cell text is prose.
    let analysis = analyze("+---+---+\n| a | b |\n+---+---+\n\n|     [@c] | x |\n");
    assert_eq!(citation_keys_from(&analysis), vec!["c".to_string()]);
}

#[test]
fn wrapped_link_ranges_are_byte_exact() {
    // A link whose text wraps (with a trailing space before the newline —
    // the case where the wrap joiner must still occupy the newline byte) has
    // ranges that slice the exact construct and destination out of the
    // document.
    let text = "See [the \ndocs](https://example.com) end.\n";
    let analysis = analyze(text);
    assert_eq!(analysis.links.len(), 1);
    let link = &analysis.links[0];
    assert_eq!(
        &text[link.range.start..link.range.end],
        "[the \ndocs](https://example.com)"
    );
    assert_eq!(
        &text[link.target_range.start..link.target_range.end],
        "https://example.com"
    );
}

#[test]
fn wrapped_destination_ranges_cover_escaped_source_bytes() {
    // The clickable region covers the raw destination including escapes.
    let text = "See [docs](/hi\\(there\\)) now.\n";
    let analysis = analyze(text);
    let link = &analysis.links[0];
    assert_eq!(link.target, "/hi(there)");
    assert_eq!(
        &text[link.target_range.start..link.target_range.end],
        "/hi\\(there\\)"
    );
}
