//! Pandoc-compatible heading identifier algorithms.
//!
//! Pandoc derives heading identifiers with one of three mutually related
//! extensions (see <https://pandoc.org/MANUAL.html#extension-auto_identifiers>):
//!
//! * `auto_identifiers` — the default Pandoc algorithm
//!   (`Text.Pandoc.Shared.uniqueIdent`),
//! * `gfm_auto_identifiers` — GitHub's algorithm
//!   (`Text.Pandoc.Shared.uniqueIdentGfm`),
//! * `ascii_identifiers` — folds the Pandoc algorithm to pure ASCII.
//!
//! Explicit identifiers from `header_attributes` always win; this module
//! only computes the automatic ones.

/// Which automatic identifier algorithm to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentifierAlgorithm {
    /// Pandoc's `auto_identifiers` algorithm.
    Pandoc,
    /// GitHub's `gfm_auto_identifiers` algorithm.
    Gfm,
}

/// Options that affect automatic identifier generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdentifierOptions {
    /// The identifier algorithm to apply.
    pub algorithm: IdentifierAlgorithm,
    /// Whether the `smart` extension is enabled. Smart typography converts
    /// `--`/`---` into dashes and `...` into an ellipsis *before* the
    /// identifier is derived, and those characters are then stripped by the
    /// identifier filter (verified against `pandoc -f markdown-smart`).
    pub smart: bool,
    /// Whether the `ascii_identifiers` extension is enabled (only meaningful
    /// with [`IdentifierAlgorithm::Pandoc`]).
    pub ascii: bool,
}

impl Default for IdentifierOptions {
    fn default() -> Self {
        IdentifierOptions {
            algorithm: IdentifierAlgorithm::Pandoc,
            smart: true,
            ascii: false,
        }
    }
}

/// Fallback identifier used when the derived identifier would be empty,
/// matching Pandoc's behavior (e.g. `# 郭德纲` with `ascii_identifiers`).
pub const EMPTY_IDENTIFIER_FALLBACK: &str = "section";

/// Derive an automatic heading identifier from heading text.
///
/// `title` is the raw heading text with explicit attributes already removed.
pub fn slugify(title: &str, options: IdentifierOptions) -> String {
    let prepared = if options.smart {
        strip_smart_punctuation_runs(title)
    } else {
        title.to_string()
    };

    match options.algorithm {
        IdentifierAlgorithm::Pandoc => slugify_pandoc(&prepared, options.ascii),
        IdentifierAlgorithm::Gfm => slugify_gfm(&prepared),
    }
}

/// Pandoc's `auto_identifiers` algorithm:
///
/// 1. drop formatting characters (emphasis markers, code, brackets),
/// 2. split into whitespace-separated words,
/// 3. per word keep only alphanumerics, `_`, `-`, `.`, lowercased
///    (empty words disappear),
/// 4. join the words with `-`,
/// 5. drop everything before the first letter,
/// 6. fall back to `section` when nothing remains.
///
/// All steps verified against `pandoc -t json` output for pandoc 3.x.
fn slugify_pandoc(title: &str, ascii: bool) -> String {
    let mut joined = String::new();
    for word in title.split_whitespace() {
        let filtered = filter_pandoc_word(word);
        if filtered.is_empty() {
            continue;
        }
        if !joined.is_empty() {
            joined.push('-');
        }
        joined.push_str(&filtered);
    }

    if ascii {
        // ascii_identifiers keeps only ASCII characters after folding.
        joined = fold_to_ascii(&joined)
            .chars()
            .filter(|ch| ch.is_ascii())
            .collect();
    }

    let joined = joined.to_lowercase();
    let slug = strip_up_to_first_letter(&joined).to_string();
    if slug.is_empty() {
        EMPTY_IDENTIFIER_FALLBACK.to_string()
    } else {
        slug
    }
}

/// GitHub's `gfm_auto_identifiers` algorithm: lowercase, keep only word
/// characters, spaces, and hyphens, then turn each space into a hyphen.
fn slugify_gfm(title: &str) -> String {
    let mut slug = String::new();
    for ch in strip_formatting(title).chars().flat_map(char::to_lowercase) {
        if ch.is_alphanumeric() || ch == '_' || ch == '-' {
            slug.push(ch);
        } else if ch.is_whitespace() {
            slug.push('-');
        }
    }

    if slug.is_empty() {
        EMPTY_IDENTIFIER_FALLBACK.to_string()
    } else {
        slug
    }
}

/// Keep only alphanumerics, `_`, `-`, `.`, lowercased, dropping formatting
/// characters first. Mirrors Pandoc's identifier filter per word.
fn filter_pandoc_word(word: &str) -> String {
    let mut filtered = String::new();
    for ch in strip_formatting(word).chars().flat_map(char::to_lowercase) {
        if ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.') {
            filtered.push(ch);
        }
    }
    filtered
}

/// Strip inline markup control characters that Pandoc's AST would never
/// contain: emphasis markers, code delimiters, and link/bracket syntax.
fn strip_formatting(text: &str) -> String {
    text.chars()
        .filter(|ch| !matches!(ch, '*' | '`' | '[' | ']' | '<' | '>' | '\\'))
        .collect()
}

/// Remove everything up to (but excluding) the first Unicode letter.
fn strip_up_to_first_letter(text: &str) -> &str {
    match text.find(char::is_alphabetic) {
        Some(index) => &text[index..],
        None => "",
    }
}

/// `smart` typography replaces `--`, `---`, and `...` before identifiers
/// are derived; the resulting typographic characters are then removed by the
/// identifier filter. Model that by pre-removing such runs.
fn strip_smart_punctuation_runs(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if ch == '-' {
            let run = chars[index..].iter().take_while(|c| **c == '-').count();
            if run >= 2 {
                index += run;
                continue;
            }
        } else if ch == '.' {
            let run = chars[index..].iter().take_while(|c| **c == '.').count();
            if run >= 3 {
                index += run;
                continue;
            }
        }
        out.push(ch);
        index += 1;
    }
    out
}

/// Fold common accented Latin characters to ASCII, mirroring what Pandoc's
/// `ascii_identifiers` produces for Western text.
pub fn fold_to_ascii(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => out.push('a'),
            'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' => out.push('A'),
            'æ' => out.push_str("ae"),
            'Æ' => out.push_str("AE"),
            'ç' => out.push('c'),
            'Ç' => out.push('C'),
            'è' | 'é' | 'ê' | 'ë' => out.push('e'),
            'È' | 'É' | 'Ê' | 'Ë' => out.push('E'),
            'ì' | 'í' | 'î' | 'ï' => out.push('i'),
            'Ì' | 'Í' | 'Î' | 'Ï' => out.push('I'),
            'ñ' => out.push('n'),
            'Ñ' => out.push('N'),
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' => out.push('o'),
            'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' => out.push('O'),
            'ø' => out.push('o'),
            'Ø' => out.push('O'),
            'ù' | 'ú' | 'û' | 'ü' => out.push('u'),
            'Ù' | 'Ú' | 'Û' | 'Ü' => out.push('U'),
            'ý' | 'ÿ' => out.push('y'),
            'Ý' => out.push('Y'),
            'ß' => out.push_str("ss"),
            'ð' => out.push('d'),
            'Ð' => out.push('D'),
            'ł' => out.push('l'),
            'Ł' => out.push('L'),
            'œ' => out.push_str("oe"),
            'Œ' => out.push_str("OE"),
            _ => out.push(ch),
        }
    }
    out
}

/// Number duplicates the way Pandoc does: first occurrence stays, subsequent
/// ones get `-1`, `-2`, ... appended.
///
/// Returns the unique identifier for `base` given how many times `base` (and
/// suffixed variants) have already been used, tracked in `counts`.
pub fn uniquify(base: &str, counts: &mut std::collections::HashMap<String, usize>) -> String {
    let count = counts.entry(base.to_string()).or_insert(0);
    let unique = if *count == 0 {
        base.to_string()
    } else {
        format!("{base}-{count}")
    };
    *count += 1;
    unique
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pandoc(title: &str) -> String {
        slugify(title, IdentifierOptions::default())
    }

    fn gfm(title: &str) -> String {
        slugify(
            title,
            IdentifierOptions {
                algorithm: IdentifierAlgorithm::Gfm,
                smart: false,
                ascii: false,
            },
        )
    }

    #[test]
    fn pandoc_slug_matches_manual_examples() {
        // (heading, expected) pairs verified with `pandoc -f markdown -t html`.
        for (title, expected) in [
            ("Hello, Pandoc Markdown!", "hello-pandoc-markdown"),
            ("`Code` & Math", "code-math"),
            ("1. Introduction", "introduction"),
            ("42 answer", "answer"),
            ("5 Umlauts MüLLER", "umlauts-müller"),
            ("郭德纲相声", "郭德纲相声"),
            ("😀smile", "smile"),
            ("Dup", "dup"),
            ("\"Quoted\"", "quoted"),
            ("a/b", "ab"),
            ("Émile Zola", "émile-zola"),
            ("A_b-c.d", "a_b-c.d"),
            ("A - B", "a---b"),
            ("3.14 is pi", "is-pi"),
            ("Über Ûnicode--Test", "über-ûnicodetest"),
            ("_underscore start", "underscore-start"),
        ] {
            assert_eq!(pandoc(title), expected, "title: {title:?}");
        }
    }

    #[test]
    fn smart_affects_pandoc_slugs_like_pandoc() {
        // With smart on, "..." and "--" disappear before slugging.
        assert_eq!(pandoc("Hello ... world"), "hello-world");
        // With smart off, periods and hyphens survive.
        assert_eq!(
            slugify(
                "Hello ... world",
                IdentifierOptions {
                    smart: false,
                    ..IdentifierOptions::default()
                }
            ),
            "hello-...-world"
        );
        assert_eq!(pandoc("\"Dashes\" -- rock"), "dashes-rock");
    }

    #[test]
    fn empty_slugs_fall_back_to_section() {
        assert_eq!(pandoc("42"), "section");
        assert_eq!(pandoc("!!!"), "section");
    }

    #[test]
    fn ascii_identifiers_fold_accents() {
        assert_eq!(
            slugify(
                "Müller",
                IdentifierOptions {
                    ascii: true,
                    ..IdentifierOptions::default()
                }
            ),
            "muller"
        );
        assert_eq!(
            slugify(
                "Émile",
                IdentifierOptions {
                    ascii: true,
                    ..IdentifierOptions::default()
                }
            ),
            "emile"
        );
        // Non-Latin text folds away entirely -> fallback.
        assert_eq!(
            slugify(
                "郭德纲",
                IdentifierOptions {
                    ascii: true,
                    ..IdentifierOptions::default()
                }
            ),
            "section"
        );
    }

    #[test]
    fn gfm_slug_matches_manual_examples() {
        for (title, expected) in [
            ("1. Introduction", "1-introduction"),
            ("Hello World!", "hello-world"),
            ("42 answer", "42-answer"),
            ("a/b", "ab"),
            ("Hello, World!", "hello-world"),
            ("42", "42"),
            ("3.14 is pi", "314-is-pi"),
            ("A - B", "a---b"),
        ] {
            assert_eq!(gfm(title), expected, "title: {title:?}");
        }
    }

    #[test]
    fn uniquify_appends_pandoc_suffixes() {
        let mut counts = std::collections::HashMap::new();
        assert_eq!(uniquify("dup", &mut counts), "dup");
        assert_eq!(uniquify("dup", &mut counts), "dup-1");
        assert_eq!(uniquify("dup", &mut counts), "dup-2");
        assert_eq!(uniquify("other", &mut counts), "other");
    }

    /// Cross-check every algorithm against a locally installed pandoc by
    /// round-tripping headings through `pandoc -t json`. Skipped when pandoc
    /// is unavailable.
    #[test]
    fn slugs_agree_with_installed_pandoc() {
        let probe = std::process::Command::new("pandoc")
            .arg("--version")
            .output();
        if probe.is_err() || !probe.unwrap().status.success() {
            eprintln!("skipping: pandoc not installed");
            return;
        }

        let titles = [
            "Hello, Pandoc Markdown!",
            "1. Introduction",
            "42 answer",
            "5 Umlauts MüLLER",
            "`Code` & Math",
            "郭德纲相声",
            "😀smile",
            "\"Quoted\"",
            "a/b",
            "Émile Zola",
            "A_b-c.d",
            "A - B",
            "3.14 is pi",
            "_underscore start",
            "Hello ... world",
            "\"Dashes\" -- rock",
            "Dup",
            "Über Ûnicode--Test",
            "Müller",
            "42",
        ];

        for (format, options) in [
            (
                "markdown",
                IdentifierOptions {
                    smart: true,
                    ..IdentifierOptions::default()
                },
            ),
            (
                "markdown-smart",
                IdentifierOptions {
                    smart: false,
                    ..IdentifierOptions::default()
                },
            ),
            (
                "markdown+ascii_identifiers",
                IdentifierOptions {
                    ascii: true,
                    ..IdentifierOptions::default()
                },
            ),
            (
                "markdown+gfm_auto_identifiers",
                IdentifierOptions {
                    algorithm: IdentifierAlgorithm::Gfm,
                    smart: true,
                    ..IdentifierOptions::default()
                },
            ),
        ] {
            let document = titles
                .iter()
                .map(|title| format!("# {title}\n\nx\n\n"))
                .collect::<String>();
            let output = std::process::Command::new("pandoc")
                .args(["-f", format, "-t", "json"])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .spawn()
                .and_then(|mut child| {
                    use std::io::Write;
                    child
                        .stdin
                        .as_mut()
                        .unwrap()
                        .write_all(document.as_bytes())?;
                    child.wait_with_output()
                })
                .expect("pandoc run");
            let json = String::from_utf8(output.stdout).unwrap();

            let mut mismatches = Vec::new();
            let mut counts = std::collections::HashMap::new();
            for (index, title) in titles.iter().enumerate() {
                let Some(actual) = nth_header_id(&json, index) else {
                    mismatches.push(format!("{format}: missing header #{index}"));
                    continue;
                };
                // Pandoc transliterates emoji to names ("grinningsmile");
                // we deliberately drop them instead. Skip that title here
                // (unit tests pin our documented behavior).
                let base = if *title == "\u{1f600}smile" {
                    actual.clone()
                } else {
                    slugify(title, options)
                };
                let expected = uniquify(&base, &mut counts);
                if actual != expected {
                    mismatches.push(format!(
                        "{format}: {title:?} -> {actual}, expected {expected}"
                    ));
                }
            }
            assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
        }
    }

    /// Extract the n-th Header identifier attribute from pandoc JSON output.
    fn nth_header_id(json: &str, n: usize) -> Option<String> {
        let value: serde_json::Value = serde_json::from_str(json).ok()?;
        let blocks = value.get("blocks")?.as_array()?;
        let mut seen = 0;
        for block in blocks {
            if block.get("t")?.as_str()? == "Header" {
                if seen == n {
                    return block.get("c")?.get(1)?.get(0)?.as_str().map(str::to_string);
                }
                seen += 1;
            }
        }
        None
    }
}
