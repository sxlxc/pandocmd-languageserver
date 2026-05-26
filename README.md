# pandocmd-languageserver

`pandocmd-languageserver` is a Rust language server for Pandoc Markdown.

The server uses `tree-sitter-pandoc-markdown` for syntax parsing and adds a
document model for common writing workflows: headings, links, reference
definitions, footnotes, citations, fenced divs, hovers, completion, navigation, and
diagnostics.

The generated grammar source is vendored under `vendor/tree-sitter-pandoc-markdown`
because the upstream repository currently has a submodule URL that Cargo cannot
resolve when used directly as a git dependency.

## Build

```bash
cargo build
```

## Release

GitHub release assets are built by `.github/workflows/release.yml` when a
version tag is pushed.

```bash
git tag -a v0.1.0 -m "v0.1.0"
git push origin v0.1.0
```

The workflow publishes these assets:

- `pandocmd-lsp-aarch64-apple-darwin.tar.gz`
- `pandocmd-lsp-x86_64-apple-darwin.tar.gz`
- `pandocmd-lsp-aarch64-unknown-linux-gnu.tar.gz`
- `pandocmd-lsp-x86_64-unknown-linux-gnu.tar.gz`
- `pandocmd-lsp-aarch64-pc-windows-msvc.zip`
- `pandocmd-lsp-x86_64-pc-windows-msvc.zip`
- `SHA256SUMS`

Update the crate versions before tagging a release.
To rebuild assets for an existing tag, run the `Release` workflow manually and
enter the tag name.

## Run the language server

```bash
cargo run -p pandocmd-lsp
```

The server speaks LSP over stdio.

## Debug with the CLI

```bash
cargo run -p pandocmd-cli -- parse path/to/file.md
cargo run -p pandocmd-cli -- symbols path/to/file.md
cargo run -p pandocmd-cli -- diagnose path/to/file.md
```

## Architecture

- `pandocmd-syntax`: tree-sitter parser setup, line indexing, syntax errors.
- `pandocmd-analysis`: headings, references, footnotes, citations, diagnostics.
- `pandocmd-pandoc`: optional Pandoc CLI validation helpers.
- `pandocmd-lsp`: stdio LSP server.
- `pandocmd-cli`: debugging commands for parser and analysis output.
