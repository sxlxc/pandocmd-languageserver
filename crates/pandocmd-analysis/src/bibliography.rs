//! Bibliography indexing: BibTeX `.bib` parsing and workspace scanning.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use ignore::WalkBuilder;
use pandocmd_extensions::{Extension, ExtensionSet};
use pandocmd_syntax::TextRange;
use regex::Regex;

static BIB_ENTRY_START_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)@\w+\s*[\{\(]\s*([^,\s]+)\s*,").unwrap());
static BIB_YEAR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b([12][0-9]{3})[a-z]?\b").unwrap());
static BIB_AUTHOR_AND_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\s+and\s+").unwrap());

/// One entry parsed from a BibTeX bibliography.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BibliographyEntry {
    pub key: String,
    pub authors: Option<String>,
    pub title: Option<String>,
    pub year: Option<String>,
    pub source: Option<BibliographySource>,
}

impl BibliographyEntry {
    /// Short "Author Year" summary used for completion detail.
    pub fn completion_detail(&self) -> Option<String> {
        match (&self.authors, &self.year) {
            (Some(authors), Some(year)) => Some(format!("{authors} {year}")),
            (Some(authors), None) => Some(authors.clone()),
            (None, Some(year)) => Some(year.clone()),
            (None, None) => None,
        }
    }
}

/// Location of an entry inside a bibliography file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BibliographySource {
    pub path: PathBuf,
    pub range: TextRange,
    pub key_range: TextRange,
}

/// Workspace-wide bibliography index.
///
/// Indexes BibTeX files referenced from YAML metadata (`bibliography:`
/// fields) and/or every `.bib` file under the workspace root.
#[derive(Debug, Clone, Default)]
pub struct WorkspaceIndex {
    root: Option<PathBuf>,
    citations: HashMap<String, BibliographyEntry>,
    duplicate_citation_keys: HashSet<String>,
}

impl WorkspaceIndex {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn from_root(root: &Path) -> Self {
        Self {
            root: Some(root.to_path_buf()),
            citations: HashMap::new(),
            duplicate_citation_keys: HashSet::new(),
        }
    }

    /// Build the index for a specific document, reading the `bibliography`
    /// paths declared in its YAML metadata.
    ///
    /// When the `yaml_metadata_block` extension is disabled the metadata
    /// block is not parsed, matching Pandoc.
    pub fn for_document(&self, document_path: Option<&Path>, text: &str) -> Self {
        self.for_document_with_extensions(
            document_path,
            text,
            ExtensionSet::flavor_defaults(pandocmd_extensions::Flavor::Markdown),
        )
    }

    /// Like [`WorkspaceIndex::for_document`] with an explicit extension set.
    pub fn for_document_with_extensions(
        &self,
        document_path: Option<&Path>,
        text: &str,
        extensions: ExtensionSet,
    ) -> Self {
        let bibliography_paths = if extensions.contains(Extension::YamlMetadataBlock) {
            bibliography_paths_from_metadata(text)
        } else {
            Vec::new()
        };
        let mut index = Self {
            root: self.root.clone(),
            citations: HashMap::new(),
            duplicate_citation_keys: HashSet::new(),
        };

        for bibliography_path in bibliography_paths {
            for resolved_path in self.resolve_bibliography_path(document_path, &bibliography_path) {
                index.add_bibliography_file(&resolved_path);
            }
        }

        index
    }

    pub fn add_bibliography_text(&mut self, text: &str) {
        self.add_bibliography_text_with_source(text, None);
    }

    pub fn add_bibliography_file(&mut self, path: &Path) {
        if !is_bibliography_file(path) {
            return;
        }
        if let Ok(text) = std::fs::read_to_string(path) {
            self.add_bibliography_text_with_source(&text, Some(path.to_path_buf()));
        }
    }

    /// Index every bibliography file under `root` (respecting ignore files).
    pub fn add_bibliography_files_from_root(&mut self, root: &Path) {
        let walker = WalkBuilder::new(root)
            .standard_filters(true)
            .max_filesize(Some(2_000_000))
            .build();

        for entry in walker.flatten() {
            let path = entry.path();
            if path.is_file() {
                self.add_bibliography_file(path);
            }
        }
    }

    fn add_bibliography_text_with_source(&mut self, text: &str, source_path: Option<PathBuf>) {
        for capture in BIB_ENTRY_START_RE.captures_iter(text) {
            let Some(entry) = parse_bibliography_entry(text, &capture, source_path.as_deref())
            else {
                continue;
            };
            if self.citations.contains_key(&entry.key) {
                self.duplicate_citation_keys.insert(entry.key.clone());
            } else {
                self.citations.insert(entry.key.clone(), entry);
            }
        }
    }

    pub fn has_citation_keys(&self) -> bool {
        !self.citations.is_empty()
    }

    pub fn contains_citation_key(&self, key: &str) -> bool {
        self.citations.contains_key(key)
    }

    pub fn citation_entry(&self, key: &str) -> Option<&BibliographyEntry> {
        self.citations.get(key)
    }

    pub fn citation_entries(&self) -> impl Iterator<Item = &BibliographyEntry> {
        self.citations.values()
    }

    pub fn citation_keys(&self) -> impl Iterator<Item = &str> {
        self.citations.keys().map(String::as_str)
    }

    pub fn has_duplicate_citation_key(&self, key: &str) -> bool {
        self.duplicate_citation_keys.contains(key)
    }

    fn resolve_bibliography_path(
        &self,
        document_path: Option<&Path>,
        bibliography_path: &str,
    ) -> Vec<PathBuf> {
        let path = Path::new(bibliography_path);
        if path.is_absolute() {
            return path
                .exists()
                .then(|| path.to_path_buf())
                .into_iter()
                .collect();
        }

        let mut paths = Vec::new();
        if let Some(document_dir) = document_path.and_then(Path::parent) {
            paths.push(document_dir.join(path));
        }
        if let Some(root) = &self.root {
            paths.push(root.join(path));
        }

        let mut seen = HashSet::new();
        paths
            .into_iter()
            .filter(|path| path.exists())
            .filter(|path| seen.insert(path.clone()))
            .collect()
    }
}

fn parse_bibliography_entry(
    text: &str,
    capture: &regex::Captures<'_>,
    source_path: Option<&Path>,
) -> Option<BibliographyEntry> {
    let key = capture.get(1)?.as_str().trim().to_string();
    let key_match = capture.get(1)?;
    let whole = capture.get(0)?;
    let matched = text.get(whole.start()..whole.end())?;
    let (open_relative, open_delimiter) = matched
        .char_indices()
        .find(|(_, ch)| matches!(ch, '{' | '('))?;
    let open_offset = whole.start() + open_relative;
    let entry_end = find_bib_entry_end(text, open_offset, open_delimiter).unwrap_or(text.len());
    let body_end = if entry_end == text.len() {
        entry_end
    } else {
        entry_end.saturating_sub(1)
    };
    let body = text.get(whole.end()..body_end)?;
    let fields = parse_bib_fields(body);
    let authors = fields
        .get("author")
        .or_else(|| fields.get("editor"))
        .and_then(|value| bib_author_summary(value));
    let title = fields
        .get("title")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let year = fields
        .get("year")
        .or_else(|| fields.get("date"))
        .and_then(|value| bib_year(value));

    Some(BibliographyEntry {
        key,
        authors,
        title,
        year,
        source: source_path.map(|path| BibliographySource {
            path: path.to_path_buf(),
            range: TextRange::new(whole.start(), entry_end),
            key_range: TextRange::new(key_match.start(), key_match.end()),
        }),
    })
}

fn find_bib_entry_end(text: &str, open_offset: usize, open_delimiter: char) -> Option<usize> {
    let close_delimiter = if open_delimiter == '{' { '}' } else { ')' };
    let mut depth = 0usize;
    let mut escaped = false;

    for (relative, ch) in text.get(open_offset..)?.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == open_delimiter {
            depth += 1;
        } else if ch == close_delimiter {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(open_offset + relative + ch.len_utf8());
            }
        }
    }

    None
}

fn parse_bib_fields(body: &str) -> HashMap<String, String> {
    let mut fields = HashMap::new();
    let mut cursor = 0;

    while cursor < body.len() {
        let Some(field_start) = next_bib_identifier_start(body, cursor) else {
            break;
        };
        let Some((key, after_key)) = parse_bib_identifier(body, field_start) else {
            break;
        };
        let mut value_start = skip_whitespace(body, after_key);
        if !body
            .get(value_start..)
            .is_some_and(|rest| rest.starts_with('='))
        {
            cursor = next_char_offset(body, field_start).unwrap_or(body.len());
            continue;
        }
        value_start += 1;
        value_start = skip_whitespace(body, value_start);

        let Some((value, after_value)) = parse_bib_field_value(body, value_start) else {
            break;
        };
        fields.insert(key.to_ascii_lowercase(), clean_bib_value(value));
        cursor = after_value;
    }

    fields
}

fn next_bib_identifier_start(input: &str, start: usize) -> Option<usize> {
    input
        .get(start..)?
        .char_indices()
        .find_map(|(index, ch)| ch.is_ascii_alphabetic().then_some(start + index))
}

fn parse_bib_identifier(input: &str, start: usize) -> Option<(&str, usize)> {
    let mut end = start;
    for (relative, ch) in input.get(start..)?.char_indices() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-') {
            end = start + relative + ch.len_utf8();
        } else {
            break;
        }
    }

    (end > start).then(|| (&input[start..end], end))
}

fn parse_bib_field_value(input: &str, start: usize) -> Option<(&str, usize)> {
    let first = input.get(start..)?.chars().next()?;
    match first {
        '{' => {
            let end = find_balanced_bib_value_end(input, start, '{', '}')?;
            Some((&input[start + 1..end - 1], end))
        }
        '"' => {
            let end = find_quoted_bib_value_end(input, start)?;
            Some((&input[start + 1..end - 1], end))
        }
        _ => {
            let end = input
                .get(start..)?
                .char_indices()
                .find_map(|(relative, ch)| (ch == ',').then_some(start + relative))
                .unwrap_or(input.len());
            Some((input[start..end].trim(), end))
        }
    }
}

fn find_balanced_bib_value_end(
    input: &str,
    open_offset: usize,
    open_delimiter: char,
    close_delimiter: char,
) -> Option<usize> {
    let mut depth = 0usize;
    let mut escaped = false;

    for (relative, ch) in input.get(open_offset..)?.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == open_delimiter {
            depth += 1;
        } else if ch == close_delimiter {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(open_offset + relative + ch.len_utf8());
            }
        }
    }

    None
}

fn find_quoted_bib_value_end(input: &str, quote_offset: usize) -> Option<usize> {
    let mut escaped = false;
    for (relative, ch) in input.get(quote_offset + 1..)?.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            return Some(quote_offset + 1 + relative + ch.len_utf8());
        }
    }
    None
}

fn skip_whitespace(input: &str, start: usize) -> usize {
    let mut cursor = start;
    while let Some(ch) = input.get(cursor..).and_then(|rest| rest.chars().next()) {
        if !ch.is_whitespace() {
            break;
        }
        cursor += ch.len_utf8();
    }
    cursor
}

fn next_char_offset(input: &str, offset: usize) -> Option<usize> {
    let ch = input.get(offset..)?.chars().next()?;
    Some(offset + ch.len_utf8())
}

fn clean_bib_value(value: &str) -> String {
    value
        .replace(['{', '}'], "")
        .replace(['~', '\n', '\r', '\t'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn bib_author_summary(value: &str) -> Option<String> {
    let authors = BIB_AUTHOR_AND_RE
        .split(value)
        .filter_map(bib_author_family_name)
        .collect::<Vec<_>>();

    match authors.as_slice() {
        [] => None,
        [author] => Some(author.clone()),
        [first, second] => Some(format!("{first} and {second}")),
        [first, ..] => Some(format!("{first} et al.")),
    }
}

fn bib_author_family_name(author: &str) -> Option<String> {
    let author = clean_bib_value(author);
    let author = author.trim().trim_matches([',', ';']);
    if author.is_empty() {
        return None;
    }

    let family = author
        .split_once(',')
        .map(|(family, _)| family.trim())
        .or_else(|| author.split_whitespace().last())?;
    let family = family.trim().trim_matches([',', ';', '.']);
    (!family.is_empty()).then(|| family.to_string())
}

fn bib_year(value: &str) -> Option<String> {
    BIB_YEAR_RE
        .captures(value)
        .and_then(|captures| captures.get(1))
        .map(|year| year.as_str().to_string())
}

/// Extract `bibliography:` values from a YAML metadata block at the start of
/// a document. Handles scalar, list (`- item`), and flow (`[a, b]`) forms.
fn bibliography_paths_from_metadata(text: &str) -> Vec<String> {
    let Some(metadata) = yaml_metadata_block(text) else {
        return Vec::new();
    };

    let lines = metadata.lines().collect::<Vec<_>>();
    let mut paths = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') || !trimmed.starts_with("bibliography:") {
            index += 1;
            continue;
        }

        let indent = line.len() - trimmed.len();
        let value = trimmed["bibliography:".len()..].trim();
        if !value.is_empty() {
            push_bibliography_values(value, &mut paths);
            index += 1;
            continue;
        }

        index += 1;
        while index < lines.len() {
            let child = lines[index];
            let child_trimmed = child.trim_start();
            let child_indent = child.len() - child_trimmed.len();
            if child_trimmed.is_empty() || child_trimmed.starts_with('#') {
                index += 1;
                continue;
            }
            if child_indent <= indent {
                break;
            }
            if let Some(value) = child_trimmed.strip_prefix('-') {
                push_bibliography_values(value.trim(), &mut paths);
            }
            index += 1;
        }
    }

    paths
}

/// The raw YAML metadata block text of a document, if it starts with one.
pub fn yaml_metadata_block(text: &str) -> Option<&str> {
    let first_line_end = text.find('\n')?;
    let first_line = text[..first_line_end].trim_end_matches('\r');
    if first_line != "---" {
        return None;
    }

    let mut content_start = first_line_end + 1;
    for line in text[content_start..].split_inclusive('\n') {
        let line_end = content_start + line.len();
        let line_without_newline = line.trim_end_matches(['\r', '\n']);
        if matches!(line_without_newline.trim(), "---" | "...") {
            return Some(&text[first_line_end + 1..content_start]);
        }
        content_start = line_end;
    }

    None
}

fn push_bibliography_values(value: &str, paths: &mut Vec<String>) {
    let value = value.trim();
    if value.is_empty() {
        return;
    }

    if value.starts_with('[') && value.ends_with(']') {
        for item in value[1..value.len().saturating_sub(1)].split(',') {
            push_bibliography_values(item, paths);
        }
        return;
    }

    let value = value
        .split('#')
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches(['"', '\'']);
    if !value.is_empty() {
        paths.push(value.to_string());
    }
}

fn is_bibliography_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("bib") | Some("bibtex")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_bibliography_keys() {
        let mut workspace = WorkspaceIndex::empty();
        workspace.add_bibliography_text(
            "@article{doe2024,\n author = {Jane Doe and John Smith},\n year = {2024},\n title = {T}\n}\n@book{roe2023,\n editor = {Richard Roe},\n date = {2023-05-01}\n}",
        );
        assert!(workspace.contains_citation_key("doe2024"));
        assert_eq!(
            workspace
                .citation_entry("doe2024")
                .and_then(|entry| entry.title.as_deref()),
            Some("T")
        );
        assert_eq!(
            workspace
                .citation_entry("doe2024")
                .and_then(BibliographyEntry::completion_detail)
                .as_deref(),
            Some("Doe and Smith 2024")
        );
        assert_eq!(
            workspace
                .citation_entry("roe2023")
                .and_then(BibliographyEntry::completion_detail)
                .as_deref(),
            Some("Roe 2023")
        );
    }

    #[test]
    fn reads_bibliography_from_yaml_metadata() {
        let root =
            std::env::temp_dir().join(format!("pandocmd-bib-metadata-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("refs.bib"),
            "@article{listed,\n title = {Listed}\n}\n@book{dup,\n title = {One}\n}\n@book{dup,\n title = {Two}\n}\n",
        )
        .unwrap();
        std::fs::write(
            root.join("unlisted.bib"),
            "@article{unlisted,\n title = {Unlisted}\n}\n",
        )
        .unwrap();

        let text = "---\nbibliography:\n  - refs.bib\n---\n\nSee [@listed].\n";
        let workspace =
            WorkspaceIndex::from_root(&root).for_document(Some(&root.join("main.md")), text);

        assert!(workspace.contains_citation_key("listed"));
        assert!(!workspace.contains_citation_key("unlisted"));
        assert!(workspace.has_duplicate_citation_key("dup"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn yaml_metadata_disabled_skips_bibliography() {
        let root = std::env::temp_dir().join(format!("pandocmd-bib-noyaml-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("refs.bib"),
            "@article{listed,\n title = {Listed}\n}\n",
        )
        .unwrap();

        let text = "---\nbibliography: refs.bib\n---\n\nSee [@listed].\n";
        let extensions = ExtensionSet::flavor_defaults(pandocmd_extensions::Flavor::Markdown)
            .disable(Extension::YamlMetadataBlock);
        let workspace = WorkspaceIndex::from_root(&root).for_document_with_extensions(
            Some(&root.join("main.md")),
            text,
            extensions,
        );

        assert!(!workspace.contains_citation_key("listed"));

        let _ = std::fs::remove_dir_all(root);
    }
}
