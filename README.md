# pandocmd-languageserver

`pandocmd-languageserver` is a Rust language server for
[Pandoc Markdown](https://pandoc.org/MANUAL.html#pandocs-markdown), built to
rust-analyzer standards: full navigation, rich hover, completion, rename,
code actions, semantic tokens, document links, and extension-aware
diagnostics — with the complete Pandoc extension model (all 77 extensions of
pandoc 3.x) available as editor settings.

The server uses a vendored `tree-sitter-pandoc-markdown` grammar for syntax
parsing and adds a line-oriented document model for the writing features
that matter in Pandoc Markdown: headings, links, reference definitions,
footnotes, citations, fenced divs, cross-references, tables, math, and
metadata blocks.

## Feature overview

| LSP feature | What it covers |
|---|---|
| `documentSymbol` | Headings (with anchor details) and fenced divs |
| `definition` | Reference links, footnote references, heading links, `@`-references, bibliography keys (jumps into `.bib` files) |
| `references` | Labels, footnotes, anchors, citation keys |
| `documentHighlight` | Fenced div open/close fences |
| `hover` | Headings, divs, links, footnotes, citations (author/title/year), local cross-references |
| `completion` | Citation keys (`[@`), document anchors (`](#`), footnote labels (`[^`), reference labels (`][`) |
| `rename` / `prepareRename` | Reference labels, footnote labels, explicit heading ids, div ids, span ids, citation keys (renaming auto-generated anchors is rejected with an explanation) |
| `codeAction` | Create missing footnote/reference definitions; enable disabled extensions (via `pandocmd.enableExtension` command) |
| `foldingRange` | Sections, fenced divs, code fences, metadata blocks |
| `documentLink` | Inline links, images, autolinks, reference definitions (relative targets resolved) |
| `semanticTokens` | Headings, fenced divs, code fences, citations, footnotes, math, links |
| diagnostics | Parser errors, duplicates, unresolved references/footnotes/anchors/citations, and `extension-disabled` warnings when a construct is used while its Pandoc extension is off |

## Configuration

The server accepts settings via `initializationOptions`,
`workspace/didChangeConfiguration` (section `pandoc`), and
`workspace/configuration` pulls. All fields are optional; unknown fields are
rejected so typos surface quickly.

```json5
{
  // Which Pandoc extensions are enabled. All 77 pandoc 3.x extensions are
  // supported (see `cargo run -p pandocmd-cli -- extensions`).
  "extensions": {
    // Base flavor preset: "markdown" (default), "gfm", "commonmark",
    // "commonmark_x", "markdown_strict", "markdown_mmd", "markdown_phpextra".
    "flavor": "markdown",
    // Enable/disable individual extensions by name, e.g. "emoji", "smart",
    // "citations". `disabled` wins over `enabled`.
    "enabled": ["emoji"],
    "disabled": ["smart"],
    // ...or a full pandoc format spec, which overrides the fields above:
    // "format": "markdown+citations-smart"
  },
  "diagnostics": {
    "unresolvedReferences": true,   // unresolved labels/footnotes/anchors/citations
    "disabledExtensions": true,     // warn when a used construct's extension is off
    "pandoc": "off"                 // "off" | "onSave": external pandoc validation
  },
  "completion": {
    "citations": true,      // [@key completion from .bib files and local ids
    "anchors": true,        // ](#anchor completion
    "referenceLabels": true // ][label and [^label completion
  }
}
```

### Zed integration

The server binary speaks LSP over stdio. In a Zed extension's
`extension.toml` / Zed settings, point the language server at the
`pandocmd-lsp` binary and pass settings through `initializationOptions`:

```json
{
  "lsp": {
    "pandoc-markdown": {
      "settings": {
        "extensions": { "flavor": "markdown", "disabled": ["smart"] },
        "diagnostics": { "pandoc": "onSave" }
      }
    }
  }
}
```

The server declares one client-side command,
`pandocmd.enableExtension` (argument: `{ "extension": "name" }`), surfaced by
the "Enable the `x` extension" code action. Extensions can implement it by
updating the user's language settings and sending
`workspace/didChangeConfiguration`.

### Extension names

Run `cargo run -p pandocmd-cli -- extensions` to print every extension with
its default for each flavor and a one-line description. Defaults are
verified against `pandoc --list-extensions=...` by the test suite.

## Build and run

```bash
cargo build
cargo run -p pandocmd-lsp          # language server over stdio
```

## Debug with the CLI

```bash
cargo run -p pandocmd-cli -- parse tests/fixtures/manual-tour.md
cargo run -p pandocmd-cli -- symbols tests/fixtures/manual-tour.md
cargo run -p pandocmd-cli -- diagnose tests/fixtures/manual-tour.md
cargo run -p pandocmd-cli -- extensions
# flavor/extension overrides for all subcommands:
cargo run -p pandocmd-cli -- diagnose --flavor gfm +task_lists file.md
```

## Architecture

- `pandocmd-extensions`: the full Pandoc extension model — names, per-flavor
  defaults (markdown, gfm, commonmark, commonmark_x, markdown_strict,
  markdown_mmd, markdown_phpextra), format-spec parsing, serde config.
- `pandocmd-syntax`: tree-sitter parser setup, line indexing, syntax errors.
- `pandocmd-analysis`: extension-aware document model — identifiers (pandoc/
  gfm/ascii slug algorithms verified against pandoc), references, footnotes,
  citations, fenced divs, links, semantic tokens, diagnostics, bibliography
  (BibTeX) indexing.
- `pandocmd-pandoc`: optional external pandoc validation.
- `pandocmd-lsp`: the LSP server (`pandocmd_lsp` library + thin binary).
- `pandocmd-cli`: debugging commands for parser and analysis output.

## Testing

```bash
cargo test --workspace
```

The suite includes unit tests, a fixture tour of the Pandoc User's Guide
(`tests/fixtures/manual-tour.md`), in-memory LSP integration tests (the
same `Connection::memory()` harness rust-analyzer uses), and ground-truth
tests that cross-check defaults and heading identifiers against an
installed `pandoc` (auto-skipped when pandoc is absent).

## Release

GitHub release assets are built by `.github/workflows/release.yml` when a
version tag is pushed.

```bash
git tag -a v0.3.0 -m "v0.3.0"
git push origin v0.3.0
```

Update the crate versions before tagging a release. To rebuild assets for an
existing tag, run the `Release` workflow manually and enter the tag name.
