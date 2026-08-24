//! Differential corpus tests: real-world Pandoc Markdown documents fetched
//! from the internet, analyzed both by this crate and by `pandoc` itself.
//!
//! The corpus lives in `tests/corpus/cache` (gitignored, auto-downloaded on
//! first run, see `tests/corpus/README.md`). Every file is parsed with
//! `pandoc -f <flavor> -t json` and the resulting AST is compared against
//! [`DocumentAnalysis`] for the facts both sides model:
//!
//! * heading levels and identifiers (auto and explicit) in document order,
//! * citation keys in document order,
//! * footnote/note counts (pandoc drops the original labels, so counts only),
//! * the sequence of link/image destinations (inline, angle autolinks, and
//!   reference links resolved the way pandoc resolves them, including
//!   `implicit_header_references`),
//! * fenced-div identifiers in document order.
//!
//! The test is skipped (with a note) when `pandoc` is not installed or the
//! corpus cannot be downloaded, mirroring the ground-truth tests in
//! `pandocmd-extensions`.
//!
//! Use `PANDOCMD_CORPUS=substr cargo test --test corpus -- --nocapture` to
//! run a single corpus file, and `PANDOCMD_CORPUS_FULL=1` to print both
//! complete sequences on divergence.

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use pandocmd_analysis::{normalize_label, AnalyzeOptions, DocumentAnalysis, WorkspaceIndex};
use pandocmd_extensions::{Extension, ExtensionSet, Flavor};
use pandocmd_syntax::PandocMarkdownParser;
use serde_json::Value;

struct CorpusFile {
    /// File name inside `tests/corpus/cache`.
    name: &'static str,
    /// Pinned raw URL (commit or tag, never a moving branch).
    url: &'static str,
    /// Pandoc flavor to parse with on both sides.
    flavor: Flavor,
}

const CORPUS: &[CorpusFile] = &[
    // Pandoc's own documentation and reader test suite (GPL-2.0+; fetched,
    // never committed). MANUAL.txt is the Pandoc User's Guide source and
    // exercises essentially every extension.
    CorpusFile {
        name: "pandoc-MANUAL.txt",
        url: "https://raw.githubusercontent.com/jgm/pandoc/3.10.2/MANUAL.txt",
        flavor: Flavor::Markdown,
    },
    CorpusFile {
        name: "pandoc-lua-filters.md",
        url: "https://raw.githubusercontent.com/jgm/pandoc/3.10.2/doc/lua-filters.md",
        flavor: Flavor::Markdown,
    },
    CorpusFile {
        name: "pandoc-getting-started.md",
        url: "https://raw.githubusercontent.com/jgm/pandoc/3.10.2/doc/getting-started.md",
        flavor: Flavor::Markdown,
    },
    CorpusFile {
        name: "pandoc-using-the-pandoc-api.md",
        url: "https://raw.githubusercontent.com/jgm/pandoc/3.10.2/doc/using-the-pandoc-api.md",
        flavor: Flavor::Markdown,
    },
    CorpusFile {
        name: "pandoc-markdown-reader-more.txt",
        url: "https://raw.githubusercontent.com/jgm/pandoc/3.10.2/test/markdown-reader-more.txt",
        flavor: Flavor::Markdown,
    },
    CorpusFile {
        name: "pandoc-tables.txt",
        url: "https://raw.githubusercontent.com/jgm/pandoc/3.10.2/test/tables.txt",
        flavor: Flavor::Markdown,
    },
    CorpusFile {
        name: "pandoc-pipe-tables.txt",
        url: "https://raw.githubusercontent.com/jgm/pandoc/3.10.2/test/pipe-tables.txt",
        flavor: Flavor::Markdown,
    },
    // Real-world third-party documents processed by pandoc in production:
    // quarto-web docs (fenced divs, cross-references, citations, math) and
    // the R Markdown Cookbook (long chapters with code chunks).
    CorpusFile {
        name: "quarto-cross-references.qmd",
        url: "https://raw.githubusercontent.com/quarto-dev/quarto-web/db905f61ca3b0b1bce3158e5353d9f22fff58a08/docs/authoring/cross-references.qmd",
        flavor: Flavor::Markdown,
    },
    CorpusFile {
        name: "quarto-citations.qmd",
        url: "https://raw.githubusercontent.com/quarto-dev/quarto-web/db905f61ca3b0b1bce3158e5353d9f22fff58a08/docs/authoring/citations.qmd",
        flavor: Flavor::Markdown,
    },
    CorpusFile {
        name: "quarto-html-basics.qmd",
        url: "https://raw.githubusercontent.com/quarto-dev/quarto-web/db905f61ca3b0b1bce3158e5353d9f22fff58a08/docs/output-formats/html-basics.qmd",
        flavor: Flavor::Markdown,
    },
    CorpusFile {
        name: "quarto-callouts.qmd",
        url: "https://raw.githubusercontent.com/quarto-dev/quarto-web/db905f61ca3b0b1bce3158e5353d9f22fff58a08/docs/authoring/callouts.qmd",
        flavor: Flavor::Markdown,
    },
    CorpusFile {
        name: "rmc-04-content.Rmd",
        url: "https://raw.githubusercontent.com/yihui/rmarkdown-cookbook/f1da9dbc5d819fe22a0d790cdd81e8dc85fb2c3c/04-content.Rmd",
        flavor: Flavor::Markdown,
    },
    CorpusFile {
        name: "rmc-10-tables.Rmd",
        url: "https://raw.githubusercontent.com/yihui/rmarkdown-cookbook/f1da9dbc5d819fe22a0d790cdd81e8dc85fb2c3c/10-tables.Rmd",
        flavor: Flavor::Markdown,
    },
    // A large GFM README (task lists, tables, autolinks) for the gfm flavor.
    CorpusFile {
        name: "gfm-ohmyzsh-readme.md",
        url: "https://raw.githubusercontent.com/ohmyzsh/ohmyzsh/830a5bcfd29fd577fdcd5f3b8e98cbaf973421fa/README.md",
        flavor: Flavor::Gfm,
    },
];

/// The facts extracted from one document, in a form both pandoc and this
/// crate can produce.
#[derive(Default, Debug)]
struct Facts {
    /// (level, identifier) in document order. Empty identifier for none.
    headers: Vec<(u8, String)>,
    /// Citation keys in document order.
    citations: Vec<String>,
    /// Footnote references plus inline notes.
    notes: usize,
    /// Link and image destinations in document order (reference
    /// definitions themselves excluded, like pandoc's AST).
    link_targets: Vec<String>,
    /// Fenced-div identifiers in document order (empty for anonymous).
    div_ids: Vec<String>,
}

#[test]
fn corpus_analysis_matches_pandoc() {
    if !pandoc_available() {
        eprintln!("skipping: pandoc not installed");
        return;
    }
    let Some(corpus) = ensure_corpus() else {
        eprintln!("skipping: corpus could not be downloaded and no cache exists");
        return;
    };
    assert!(!corpus.is_empty(), "corpus is empty");

    let filter = std::env::var("PANDOCMD_CORPUS").ok();
    let mut failures = Vec::new();
    let mut compared = 0;
    for path in corpus {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if let Some(substr) = &filter {
            if !name.contains(substr.as_str()) {
                continue;
            }
        }
        let file = match CorpusFile::by_name(&name) {
            Some(file) => file,
            None => continue,
        };
        let raw = match std::fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(error) => {
                failures.push(format!("{name}: unreadable: {error}"));
                continue;
            }
        };
        // Normalize line endings the way editors deliver content to the LSP.
        let text = raw.replace("\r\n", "\n");
        // R Markdown / Quarto chunk fences (```{r, ...} / ```{{r}}) are not
        // valid `fenced_code_attributes` syntax for pandoc, which then reads
        // them as inline-code paragraphs and turns `#`-comment lines inside
        // chunks into headings. Our language server treats them as code
        // fences (the useful behavior for Rmd/qmd authors), so the chunk
        // headers are normalized to plain fenced-code info strings before
        // pandoc sees the document.
        let pandoc_text = normalize_chunk_fences(&text);

        let compare_divs =
            ExtensionSet::flavor_defaults(file.flavor).contains(Extension::FencedDivs);
        let mut format = file.flavor.name().to_string();
        if compare_divs {
            // Our document model covers `:::` fenced divs only, so pandoc's
            // conversion of raw HTML `<div>` elements into Div blocks is
            // turned off to make the comparison exact.
            format.push_str("-native_divs");
        }
        let Some(pandoc) = pandoc_facts(&pandoc_text, &format) else {
            eprintln!("skipping {name}: pandoc could not parse it");
            continue;
        };
        let ours = match our_facts(&text, file.flavor) {
            Ok(facts) => facts,
            Err(error) => {
                failures.push(format!("{name}: our analysis failed: {error}"));
                continue;
            }
        };
        compared += 1;
        failures.extend(compare_facts(&name, &text, &ours, &pandoc, compare_divs));
    }
    assert!(
        compared > 0,
        "no corpus files were compared (filter={filter:?})"
    );
    assert!(
        failures.is_empty(),
        "{} divergence(s) from pandoc across {compared} files:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

impl CorpusFile {
    fn by_name(name: &str) -> Option<&'static CorpusFile> {
        CORPUS.iter().find(|file| file.name == name)
    }
}

// ---------------------------------------------------------------------------
// Corpus download
// ---------------------------------------------------------------------------

/// Rewrite ```` ```{r, eval=FALSE} ```` / ```` ```{{python}} ```` chunk fences
/// into plain ```` ```r ```` fences. Only bare-word chunk headers are
/// rewritten; pandoc attribute fences like ```` ```{#id .class k=v} ```` are
/// left alone (pandoc parses those as real code blocks).
fn normalize_chunk_fences(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_end();
        let indent_len = trimmed.len() - trimmed.trim_start_matches([' ', '\t']).len();
        let (indent, rest) = trimmed.split_at(indent_len);
        if let Some(info) = rest.strip_prefix("```") {
            let braces = info.trim().trim_start_matches('{').trim_end_matches('}');
            let first = braces.split([',', ' ']).next().unwrap_or("");
            let bare_word = !first.is_empty()
                && first
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-');
            if bare_word && !braces.starts_with('.') && !braces.starts_with('#') {
                out.push_str(indent);
                out.push_str("```");
                out.push_str(first);
                out.push('\n');
                continue;
            }
        }
        out.push_str(line);
    }
    out
}

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/corpus/cache")
}

/// Make sure every corpus file exists locally; download the missing ones.
/// Returns the list of available files (possibly a subset on partial
/// network failure), or `None` when nothing is available.
fn ensure_corpus() -> Option<Vec<PathBuf>> {
    let dir = corpus_dir();
    std::fs::create_dir_all(&dir).ok()?;
    for file in CORPUS {
        let path = dir.join(file.name);
        if path.exists() {
            continue;
        }
        eprintln!("corpus: downloading {}", file.name);
        let status = Command::new("curl")
            .arg("-fsSL")
            .arg("--max-time")
            .arg("60")
            .arg(file.url)
            .arg("-o")
            .arg(&path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if !status.map(|status| status.success()).unwrap_or(false) {
            eprintln!("corpus: failed to download {}", file.name);
            let _ = std::fs::remove_file(&path);
        }
    }
    let mut files: Vec<PathBuf> = CORPUS
        .iter()
        .map(|file| dir.join(file.name))
        .filter(|path| path.exists())
        .collect();
    files.sort();
    (!files.is_empty()).then_some(files)
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

// ---------------------------------------------------------------------------
// Pandoc side: walk the JSON AST
// ---------------------------------------------------------------------------

fn pandoc_facts(text: &str, flavor: &str) -> Option<Facts> {
    let mut child = Command::new("pandoc")
        .args(["--from", flavor, "--to", "json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    child.stdin.take().unwrap().write_all(text.as_bytes()).ok();
    let output = child.wait_with_output().ok()?;
    if !output.status.success() {
        return None;
    }
    let json: Value = serde_json::from_slice(&output.stdout).ok()?;
    let mut facts = Facts::default();
    // Metadata values are parsed as inlines by pandoc but not scanned by our
    // document model, so only the block list is compared.
    for block in json.get("blocks")?.as_array()? {
        walk_pandoc(block, &mut facts);
    }
    Some(facts)
}

fn walk_pandoc(value: &Value, facts: &mut Facts) {
    if let Some(items) = value.as_array() {
        for item in items {
            walk_pandoc(item, facts);
        }
        return;
    }
    let Some(object) = value.as_object() else {
        return;
    };
    let tag = object.get("t").and_then(Value::as_str).unwrap_or("");
    let constructor = object.get("c").and_then(Value::as_array);
    match tag {
        "Header" => {
            // c: [level, attr, inlines]
            if let Some(constructor) = constructor {
                let level = constructor.first().and_then(Value::as_u64).unwrap_or(0) as u8;
                let identifier = constructor
                    .get(1)
                    .and_then(Value::as_array)
                    .and_then(|attr| attr.first())
                    .and_then(Value::as_str)
                    .unwrap_or("");
                facts.headers.push((level, identifier.to_string()));
                if let Some(inlines) = constructor.get(2) {
                    walk_pandoc(inlines, facts);
                }
            }
        }
        "Cite" => {
            // c: [[citation...], inlines]
            if let Some(constructor) = constructor {
                if let Some(citations) = constructor.first().and_then(Value::as_array) {
                    for citation in citations {
                        if let Some(key) = citation.get("citationId").and_then(Value::as_str) {
                            facts.citations.push(key.to_string());
                        }
                    }
                }
                if let Some(inlines) = constructor.get(1) {
                    walk_pandoc(inlines, facts);
                }
            }
        }
        "Note" => {
            facts.notes += 1;
            if let Some(blocks) = constructor.and_then(|c| c.first()) {
                walk_pandoc(blocks, facts);
            }
        }
        "Link" | "Image" => {
            // c: [attr, inlines, [target, title]]
            if let Some(constructor) = constructor {
                if let Some(target) = constructor
                    .get(2)
                    .and_then(Value::as_array)
                    .and_then(|pair| pair.first())
                    .and_then(Value::as_str)
                {
                    facts.link_targets.push(target.to_string());
                }
                if let Some(inlines) = constructor.get(1) {
                    walk_pandoc(inlines, facts);
                }
            }
        }
        "Div" => {
            // c: [attr, blocks]
            if let Some(constructor) = constructor {
                let identifier = constructor
                    .first()
                    .and_then(Value::as_array)
                    .and_then(|attr| attr.first())
                    .and_then(Value::as_str)
                    .unwrap_or("");
                facts.div_ids.push(identifier.to_string());
                if let Some(blocks) = constructor.get(1) {
                    walk_pandoc(blocks, facts);
                }
            }
        }
        _ => {
            if let Some(constructor) = constructor {
                for child in constructor {
                    walk_pandoc(child, facts);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Our side: DocumentAnalysis
// ---------------------------------------------------------------------------

fn our_facts(text: &str, flavor: Flavor) -> anyhow_lite::Result<Facts> {
    let mut parser = PandocMarkdownParser::new().map_err(|error| error.to_string())?;
    let document = parser
        .parse(text.to_string())
        .map_err(|error| error.to_string())?;
    let options = AnalyzeOptions::with_extensions(ExtensionSet::flavor_defaults(flavor));
    let analysis = DocumentAnalysis::analyze(&document, &WorkspaceIndex::empty(), &options);

    let mut facts = Facts::default();
    for heading in &analysis.headings {
        facts
            .headers
            .push((heading.level, heading.anchor.clone().unwrap_or_default()));
    }
    for citation in &analysis.citations {
        facts.citations.push(citation.key.clone());
    }
    facts.notes = analysis.footnote_references.len() + analysis.inline_notes.len();

    // Reference-link resolution mirrors pandoc: definition target first,
    // then implicit header references (`[heading title]` -> `#identifier`).
    let definitions: HashMap<&str, &str> =
        analysis
            .reference_definitions
            .iter()
            .fold(HashMap::new(), |mut map, definition| {
                // Pandoc's reference table keeps the LAST definition of a label
                // (verified: duplicate labels resolve to the later target).
                map.insert(
                    definition.normalized_label.as_str(),
                    definition.target.as_str(),
                );
                map
            });
    let mut implicit_headers: HashMap<String, Option<String>> = HashMap::new();
    if options
        .extensions
        .contains(Extension::ImplicitHeaderReferences)
    {
        // Pandoc resolves `[heading title]` through its reference table,
        // where the FIRST definition of a label wins.
        for heading in &analysis.headings {
            implicit_headers
                .entry(normalize_label(&heading.title))
                .or_insert_with(|| heading.anchor.clone());
        }
    }
    let mut ours_links: Vec<(usize, String)> = Vec::new();
    for link in &analysis.reference_links {
        let resolved = definitions
            .get(link.normalized_label.as_str())
            .map(|target| (*target).to_string())
            .or_else(|| {
                implicit_headers
                    .get(&link.normalized_label)
                    .and_then(|anchor| anchor.clone().map(|anchor| format!("#{anchor}")))
            });
        if let Some(target) = resolved {
            ours_links.push((link.range.start, target));
        }
    }
    for link in &analysis.links {
        if link.kind == pandocmd_analysis::LinkKind::Definition {
            continue;
        }
        ours_links.push((link.range.start, link.target.clone()));
    }
    ours_links.sort_by_key(|(start, _)| *start);
    facts.link_targets = ours_links
        .into_iter()
        .map(|(_, target)| normalize_target_for_pandoc(&target))
        .collect();

    for div in &analysis.fenced_divs {
        facts.div_ids.push(div.id.clone().unwrap_or_default());
    }
    Ok(facts)
}

/// Bring our raw link targets into pandoc's AST form for comparison:
/// pandoc collapses internal whitespace runs in URLs to single spaces and
/// percent-encodes them, and decodes character entities.
fn normalize_target_for_pandoc(target: &str) -> String {
    let mut normalized = target.split_whitespace().collect::<Vec<_>>().join(" ");
    if !normalized.is_empty() {
        normalized = normalized.replace(' ', "%20");
    }
    if normalized.contains('&') {
        normalized = decode_entities(&normalized);
    }
    normalized
}

/// Decode the handful of named entities pandoc's test corpus uses plus
/// numeric decimal entities.
fn decode_entities(text: &str) -> String {
    let named = [
        ("&ouml;", "\u{f6}"),
        ("&uuml;", "\u{fc}"),
        ("&auml;", "\u{e4}"),
        ("&amp;", "&"),
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&quot;", "\""),
    ];
    let mut out = text.to_string();
    for (entity, replacement) in named {
        out = out.replace(entity, replacement);
    }
    out
}

// A tiny error alias so this test file does not need a new dev-dependency.
mod anyhow_lite {
    pub type Result<T> = std::result::Result<T, String>;
}

// ---------------------------------------------------------------------------
// Comparison and reporting
// ---------------------------------------------------------------------------

fn compare_facts(
    name: &str,
    text: &str,
    ours: &Facts,
    pandoc: &Facts,
    compare_divs: bool,
) -> Vec<String> {
    let mut failures = Vec::new();
    failures.extend(report_sequence(
        name,
        text,
        "heading (level, identifier)",
        &ours.headers,
        &pandoc.headers,
        |header| format!("({}, {:?})", header.0, header.1),
    ));
    // Citations and link destinations are compared as multisets: pandoc
    // visits grid/multiline table cells column-by-column per row while a
    // line-oriented scanner reads them line-by-line, so the order of keys
    // inside multi-line cells legitimately differs. The multiset must
    // still match exactly (same keys, same counts).
    failures.extend(report_multiset(
        name,
        "citation key",
        ours.citations.clone(),
        pandoc.citations.clone(),
        |key| format!("@{key}"),
    ));
    if ours.notes != pandoc.notes {
        failures.push(format!(
            "{name}: note count diverges from pandoc: ours {} vs pandoc {}",
            ours.notes, pandoc.notes
        ));
    }
    failures.extend(report_multiset(
        name,
        "link target",
        ours.link_targets.clone(),
        pandoc.link_targets.clone(),
        |target| format!("{target:?}"),
    ));
    if compare_divs {
        failures.extend(report_sequence(
            name,
            text,
            "div identifier",
            &ours.div_ids,
            &pandoc.div_ids,
            |id| format!("{id:?}"),
        ));
    }
    failures
}

/// Compare two document-order sequences and describe every divergence.
fn report_sequence<T: std::cmp::PartialEq + std::fmt::Debug>(
    name: &str,
    _text: &str,
    what: &str,
    ours: &[T],
    pandoc: &[T],
    render: impl Fn(&T) -> String,
) -> Vec<String> {
    if ours.len() == pandoc.len() && ours.iter().zip(pandoc).all(|(a, b)| a == b) {
        return Vec::new();
    }
    let mut failures = Vec::new();
    let mut first_divergence = None;
    for (index, (ours_item, pandoc_item)) in ours.iter().zip(pandoc).enumerate() {
        if ours_item != pandoc_item {
            first_divergence = Some(index);
            failures.push(format!(
                "{name}: {what} #{index} diverges: ours {} vs pandoc {}",
                render(ours_item),
                render(pandoc_item)
            ));
            break;
        }
    }
    if ours.len() != pandoc.len() && first_divergence.is_none() {
        let index = ours.len().min(pandoc.len());
        let extra = if ours.len() > pandoc.len() {
            format!("ours has extra: {}", render(&ours[index]))
        } else {
            format!("pandoc has extra: {}", render(&pandoc[index]))
        };
        failures.push(format!(
            "{name}: {what} count diverges at #{index} (ours {}, pandoc {}): {extra}",
            ours.len(),
            pandoc.len()
        ));
        first_divergence = Some(index);
    }
    if first_divergence.is_some() && ours.len() != pandoc.len() {
        failures.push(format!(
            "{name}: {what} counts differ overall: ours {} vs pandoc {}",
            ours.len(),
            pandoc.len()
        ));
    }
    if std::env::var("PANDOCMD_CORPUS_FULL").is_ok() {
        eprintln!("--- {name}: {what} ---");
        eprintln!("ours   ({}): {:?}", ours.len(), ours);
        eprintln!("pandoc ({}): {:?}", pandoc.len(), pandoc);
    }
    failures
}

/// Compare two multisets (sorted copies) and describe the differences.
fn report_multiset<T: Ord + Clone + std::fmt::Debug>(
    name: &str,
    what: &str,
    mut ours: Vec<T>,
    mut pandoc: Vec<T>,
    render: impl Fn(&T) -> String,
) -> Vec<String> {
    ours.sort();
    pandoc.sort();
    if ours == pandoc {
        return Vec::new();
    }
    let mut failures = Vec::new();
    let mut only_ours = Vec::new();
    let mut only_pandoc = Vec::new();
    let mut index_ours = 0;
    let mut index_pandoc = 0;
    while index_ours < ours.len() || index_pandoc < pandoc.len() {
        match (ours.get(index_ours), pandoc.get(index_pandoc)) {
            (Some(left), Some(right)) if left == right => {
                index_ours += 1;
                index_pandoc += 1;
            }
            (Some(left), Some(right)) => {
                if left < right {
                    only_ours.push(render(left));
                    index_ours += 1;
                } else {
                    only_pandoc.push(render(right));
                    index_pandoc += 1;
                }
            }
            (Some(left), None) => {
                only_ours.push(render(left));
                index_ours += 1;
            }
            (None, Some(right)) => {
                only_pandoc.push(render(right));
                index_pandoc += 1;
            }
            (None, None) => break,
        }
    }
    failures.push(format!(
        "{name}: {what} multiset diverges from pandoc (ours {}, pandoc {})",
        ours.len(),
        pandoc.len()
    ));
    if !only_ours.is_empty() {
        failures.push(format!(
            "  only ours:   {}",
            only_ours
                .iter()
                .take(12)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !only_pandoc.is_empty() {
        failures.push(format!(
            "  only pandoc: {}",
            only_pandoc
                .iter()
                .take(12)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if std::env::var("PANDOCMD_CORPUS_FULL").is_ok() {
        eprintln!("--- {name}: {what} ---");
        eprintln!("ours   ({}): {:?}", ours.len(), ours);
        eprintln!("pandoc ({}): {:?}", pandoc.len(), pandoc);
    }
    failures
}
