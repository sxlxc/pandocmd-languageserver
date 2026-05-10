use std::cmp::min;

use thiserror::Error;
use tree_sitter::{InputEdit, Parser, Point, Tree};

#[derive(Debug, Error)]
pub enum SyntaxError {
    #[error("failed to load tree-sitter Pandoc Markdown grammar: {0}")]
    Grammar(#[from] tree_sitter::LanguageError),
    #[error("tree-sitter parser returned no tree")]
    EmptyParse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextPosition {
    pub line: u32,
    pub character: u32,
}

impl TextPosition {
    pub const fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextRange {
    pub start: usize,
    pub end: usize,
}

impl TextRange {
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub fn contains(self, offset: usize) -> bool {
        self.start <= offset && offset <= self.end
    }

    pub fn is_empty(self) -> bool {
        self.start == self.end
    }
}

#[derive(Debug, Clone)]
pub struct LineIndex {
    line_starts: Vec<usize>,
    len: usize,
}

impl LineIndex {
    pub fn new(text: &str) -> Self {
        let mut line_starts = vec![0];
        for (idx, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(idx + 1);
            }
        }

        Self {
            line_starts,
            len: text.len(),
        }
    }

    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    pub fn line_start(&self, line: usize) -> Option<usize> {
        self.line_starts.get(line).copied()
    }

    pub fn line_end(&self, text: &str, line: usize) -> Option<usize> {
        let start = *self.line_starts.get(line)?;
        let next = self.line_starts.get(line + 1).copied().unwrap_or(self.len);
        let mut end = next;
        if end > start && text.as_bytes().get(end - 1) == Some(&b'\n') {
            end -= 1;
        }
        if end > start && text.as_bytes().get(end - 1) == Some(&b'\r') {
            end -= 1;
        }
        Some(end)
    }

    pub fn position_to_offset(&self, text: &str, position: TextPosition) -> usize {
        let line = min(
            position.line as usize,
            self.line_starts.len().saturating_sub(1),
        );
        let start = self.line_starts[line];
        let end = self.line_end(text, line).unwrap_or(self.len);
        let line_text = &text[start..end];
        let target = position.character as usize;
        let mut utf16_units = 0;

        for (idx, ch) in line_text.char_indices() {
            if utf16_units >= target {
                return start + idx;
            }
            utf16_units += ch.len_utf16();
            if utf16_units > target {
                return start + idx;
            }
        }

        end
    }

    pub fn offset_to_position(&self, text: &str, offset: usize) -> TextPosition {
        let offset = min(offset, self.len);
        let line = match self.line_starts.binary_search(&offset) {
            Ok(line) => line,
            Err(next) => next.saturating_sub(1),
        };
        let start = self.line_starts[line];
        let end = min(offset, self.line_end(text, line).unwrap_or(self.len));
        let character = text[start..end].chars().map(char::len_utf16).sum::<usize>() as u32;

        TextPosition::new(line as u32, character)
    }

    pub fn range_to_positions(&self, text: &str, range: TextRange) -> (TextPosition, TextPosition) {
        (
            self.offset_to_position(text, range.start),
            self.offset_to_position(text, range.end),
        )
    }
}

#[derive(Debug, Clone)]
pub struct SyntaxDiagnostic {
    pub range: TextRange,
    pub message: String,
}

pub struct ParsedDocument {
    text: String,
    tree: Tree,
    line_index: LineIndex,
}

impl ParsedDocument {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    pub fn line_index(&self) -> &LineIndex {
        &self.line_index
    }

    pub fn has_error(&self) -> bool {
        self.tree.root_node().has_error()
    }

    pub fn syntax_diagnostics(&self) -> Vec<SyntaxDiagnostic> {
        let mut diagnostics = Vec::new();
        collect_syntax_errors(self.tree.root_node(), &mut diagnostics);
        diagnostics
    }
}

pub struct PandocMarkdownParser {
    block: Parser,
}

impl PandocMarkdownParser {
    pub fn new() -> Result<Self, SyntaxError> {
        let mut block = Parser::new();
        let language: tree_sitter::Language = tree_sitter_pandoc_markdown::LANGUAGE.into();
        block.set_language(&language)?;
        Ok(Self { block })
    }

    pub fn parse(&mut self, text: impl Into<String>) -> Result<ParsedDocument, SyntaxError> {
        let text = text.into();
        let tree = self
            .block
            .parse(text.as_bytes(), None)
            .ok_or(SyntaxError::EmptyParse)?;
        let line_index = LineIndex::new(&text);
        Ok(ParsedDocument {
            text,
            tree,
            line_index,
        })
    }

    pub fn reparse(
        &mut self,
        text: impl Into<String>,
        old_tree: Option<&Tree>,
    ) -> Result<ParsedDocument, SyntaxError> {
        let text = text.into();
        let tree = self
            .block
            .parse(text.as_bytes(), old_tree)
            .ok_or(SyntaxError::EmptyParse)?;
        let line_index = LineIndex::new(&text);
        Ok(ParsedDocument {
            text,
            tree,
            line_index,
        })
    }
}

pub fn input_edit_for_replacement(
    old_text: &str,
    range: TextRange,
    replacement: &str,
) -> InputEdit {
    let old_index = LineIndex::new(old_text);
    let start_position = old_index.offset_to_position(old_text, range.start);
    let old_end_position = old_index.offset_to_position(old_text, range.end);

    let mut new_line = start_position.line as usize;
    let mut new_character = start_position.character as usize;
    for ch in replacement.chars() {
        if ch == '\n' {
            new_line += 1;
            new_character = 0;
        } else {
            new_character += ch.len_utf16();
        }
    }

    InputEdit {
        start_byte: range.start,
        old_end_byte: range.end,
        new_end_byte: range.start + replacement.len(),
        start_position: Point {
            row: start_position.line as usize,
            column: start_position.character as usize,
        },
        old_end_position: Point {
            row: old_end_position.line as usize,
            column: old_end_position.character as usize,
        },
        new_end_position: Point {
            row: new_line,
            column: new_character,
        },
    }
}

fn collect_syntax_errors(node: tree_sitter::Node<'_>, diagnostics: &mut Vec<SyntaxDiagnostic>) {
    if node.is_error() || node.is_missing() {
        diagnostics.push(SyntaxDiagnostic {
            range: TextRange::new(node.start_byte(), node.end_byte()),
            message: format!("syntax error near {}", node.kind()),
        });
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.has_error() || child.is_error() || child.is_missing() {
            collect_syntax_errors(child, diagnostics);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_index_round_trips_utf16_positions() {
        let text = "a\né𝌆x\nz";
        let index = LineIndex::new(text);

        let offset = index.position_to_offset(text, TextPosition::new(1, 3));
        assert_eq!(&text[offset..], "x\nz");
        assert_eq!(
            index.offset_to_position(text, offset),
            TextPosition::new(1, 3)
        );
    }

    #[test]
    fn parses_pandoc_markdown() {
        let mut parser = PandocMarkdownParser::new().unwrap();
        let document = parser.parse("# Title\n\nA paragraph.\n").unwrap();
        assert_eq!(document.line_index().line_count(), 4);
        assert!(!document.tree().root_node().to_sexp().is_empty());
    }
}
