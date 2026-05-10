# Pandoc Markdown Language Server

  ## Summary

  Build a Rust-based LSP server for Pandoc Markdown, optimized first for editor writing assistance. The workspace is
  currently empty, so start with a clean Cargo workspace and treat tree-sitter-pandoc-markdown as the syntax
  foundation.

  Use Rust because it fits tree-sitter, native binaries, editor distribution, and fast incremental analysis. Use
  Pandoc itself only where tree-sitter cannot provide reliable semantic validation.

  Relevant references checked:

  - Pandoc supports enhanced Markdown features like metadata blocks, footnotes, citations, math, and tables:
    https://github.com/jgm/pandoc
  - Existing general tree-sitter Markdown grammars are CommonMark/GFM-oriented and warn against depending on them for
    full correctness: https://github.com/tree-sitter-grammars/tree-sitter-markdown

  ## Key Changes

  - Create a Cargo workspace with:
      - pandocmd-lsp: LSP binary over stdio.
      - pandocmd-syntax: tree-sitter integration, incremental parsing, syntax tree wrappers.
      - pandocmd-analysis: document model, heading/reference/citation/footnote extraction, diagnostics.
      - pandocmd-pandoc: optional Pandoc CLI integration for deeper validation.
      - pandocmd-cli: debugging commands like parse, symbols, diagnose.
  - Use modern Rust LSP tooling:
      - lsp-server + lsp-types for protocol handling.
      - tree-sitter for incremental parsing.
      - ropey plus a line-index helper for UTF-8/UTF-16 position mapping.
      - serde, tracing, tracing-subscriber, anyhow/thiserror.
      - insta for snapshot tests.
  - Vendor or pin https://github.com/sxlxc/tree-sitter-pandoc-markdown by commit.
      - Prefer its Rust binding if present.
      - Otherwise compile the generated C parser through a small build.rs using cc.
      - Keep the grammar isolated behind pandocmd-syntax so it can be updated without touching LSP logic.

  ## MVP Behavior

  - Implement core LSP capabilities:
      - initialize, shutdown, textDocument/didOpen, didChange, didClose.
      - Incremental parse updates on document changes.
      - textDocument/documentSymbol for headings and structured blocks.
      - textDocument/definition for reference links, footnotes, and heading anchors.
      - textDocument/references for reference links, footnotes, citations, and heading links.
      - textDocument/hover for resolved links, footnotes, citation keys, and headings.
      - textDocument/completion for local heading anchors, reference labels, footnotes, and citation keys.
      - Diagnostics for parser errors, duplicate definitions, unresolved references, unresolved footnotes, and
        malformed heading anchors.
  - Add optional Pandoc validation:
      - Detect pandoc on PATH.
      - Run it asynchronously/debounced, never blocking interactive syntax features.
      - Surface Pandoc parse/conversion errors as diagnostics when available.
      - Make this configurable and disabled gracefully if Pandoc is missing.
  - Support project indexing:
      - Scan Markdown files under the workspace using .gitignore rules.
      - Maintain a lightweight index of headings, reference definitions, footnotes, and bibliography files.
      - Re-index changed/opened files incrementally.

  ## Test Plan

  - Unit tests:
      - Position mapping with UTF-8, UTF-16, CRLF, and multiline edits.
      - Incremental parse correctness after insert/delete edits.
      - Anchor generation and duplicate heading handling.
      - Reference, footnote, citation, and link extraction.
  - Snapshot tests:
      - Syntax-to-document-model output for representative Pandoc Markdown fixtures.
      - Diagnostics for malformed or unresolved constructs.
      - Document symbols and completions.
  - Integration tests:
      - LSP initialize/open/change/request flows using JSON-RPC fixtures.
      - Workspace indexing across multiple files.
      - Pandoc validation behavior with Pandoc present and absent.

  ## Assumptions

  - First target is a standalone editor language server, not a VS Code extension.
  - Rust is the implementation stack.
  - MVP prioritizes writing assistance over build/preview workflows.
  - Formatting is not in the first MVP because Pandoc Markdown formatting can rewrite documents too aggressively; add
    it later as an explicit opt-in feature.
  - VS Code, Neovim, Helix, and Zed should all be able to use the server through normal LSP configuration once the
    binary exists.
