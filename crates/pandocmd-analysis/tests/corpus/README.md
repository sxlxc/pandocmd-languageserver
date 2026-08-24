# Differential test corpus

`tests/corpus.rs` compares this crate's `DocumentAnalysis` against `pandoc`
itself on real-world Pandoc Markdown documents. The corpus files are
**downloaded on demand** into `cache/` (gitignored) and are never committed:

- The pandoc sources are GPL-2.0+ and this repository is MIT-licensed.
- Documents stay frozen at a pinned upstream commit, so a corpus file always
  exercises the exact bytes recorded in the URL.

## Sources

All URLs are pinned to a tag or commit hash, never a moving branch.

| File | Source | License |
|---|---|---|
| `pandoc-MANUAL.txt` | [pandoc 3.10.2 `MANUAL.txt`](https://github.com/jgm/pandoc/blob/3.10.2/MANUAL.txt) — the Pandoc User's Guide source; exercises nearly every markdown extension | GPL-2.0+ |
| `pandoc-lua-filters.md` | pandoc 3.10.2 `doc/lua-filters.md` | GPL-2.0+ |
| `pandoc-getting-started.md` | pandoc 3.10.2 `doc/getting-started.md` | GPL-2.0+ |
| `pandoc-using-the-pandoc-api.md` | pandoc 3.10.2 `doc/using-the-pandoc-api.md` | GPL-2.0+ |
| `pandoc-markdown-reader-more.txt` | pandoc 3.10.2 `test/markdown-reader-more.txt` — exotic reader cases | GPL-2.0+ |
| `pandoc-tables.txt` | pandoc 3.10.2 `test/tables.txt` — grid/multiline tables with inline markup | GPL-2.0+ |
| `pandoc-pipe-tables.txt` | pandoc 3.10.2 `test/pipe-tables.txt` | GPL-2.0+ |
| `quarto-cross-references.qmd` | [quarto-web](https://github.com/quarto-dev/quarto-web) @ `db905f61` — fenced divs, cross-references, math | GPL-3.0 |
| `quarto-citations.qmd` | quarto-web @ `db905f61` — citations, footnotes | GPL-3.0 |
| `quarto-html-basics.qmd` | quarto-web @ `db905f61` | GPL-3.0 |
| `quarto-callouts.qmd` | quarto-web @ `db905f61` — nested callout divs | GPL-3.0 |
| `rmc-04-content.Rmd` | [rmarkdown-cookbook](https://github.com/yihui/rmarkdown-cookbook) @ `f1da9dbc` — long chapter, code chunks, citations | CC BY-NC-ND 4.0 |
| `rmc-10-tables.Rmd` | rmarkdown-cookbook @ `f1da9dbc` — tables chapter | CC BY-NC-ND 4.0 |
| `gfm-ohmyzsh-readme.md` | [ohmyzsh](https://github.com/ohmyzsh/ohmyzsh) @ `830a5bcf` README — large GFM document (tables, task lists, autolinks) | MIT |

## Running

```sh
cargo test -p pandocmd-analysis --test corpus -- --nocapture
```

Requirements: `pandoc` 3.x on `PATH` and `curl` for the first download
(files are cached afterwards). The test skips with a note when either is
missing.

Run a single file by substring of its name:

```sh
PANDOCMD_CORPUS=MANUAL cargo test -p pandocmd-analysis --test corpus -- --nocapture
```

Print both complete sequences for every diverging category:

```sh
PANDOCMD_CORPUS_FULL=1 cargo test -p pandocmd-analysis --test corpus -- --nocapture
```

## What is compared

For every file, the test parses with `pandoc -f <flavor> -t json` and with
`DocumentAnalysis` (same flavor defaults), then compares:

- heading levels + identifiers in document order (explicit and auto,
  including pandoc's uniquification),
- citation keys and link/image destinations as multisets (pandoc walks
  grid/multiline table cells column-by-column while a line-oriented scanner
  reads line-by-line, so order inside multi-line cells legitimately differs;
  keys, targets, and counts must still match exactly),
- note count (footnote references + inline notes),
- link resolution exactly like pandoc: definition lookup (last definition
  of a label wins), then `implicit_header_references` (first heading of a
  title wins),
- fenced-div identifiers in document order (pandoc runs with
  `-native_divs` so `Div` elements correspond 1:1 to `:::` fences; gfm has
  no fenced divs and skips this comparison).

Metadata blocks are compared on neither side (pandoc parses metadata values
as inlines, our document model does not scan them). Our raw link targets are
normalized to pandoc's AST form before comparison (whitespace runs collapse
and percent-encode, character entities decode).

## What the corpus development found and fixed

Building this harness surfaced real divergences between the scanner and
pandoc, all now covered by regression tests in
`tests/corpus_regressions.rs`:

- indented code blocks (with list/definition-list content-column awareness,
  including nested definition lists) were not recognized at all —
  citations, notes, and links leaked out of verbatim examples;
- headings and fenced divs inside block quotes were invisible;
- fenced divs and code fences indented to list-item content columns were
  invisible;
- multiline/simple table row separators were misread as setext heading
  underlines;
- grid-table cells received no block treatment (code cells leaked
  citations; `# heading` cells were skipped);
- inline link text and destinations may wrap onto the next line, may
  contain nested images (`[![badge](…)](…)`), spaces, escaped parentheses,
  and one lazy inner bracket (`[… [x](url)`);
- reference definitions may continue on the following line, may use
  angle-bracket targets with spaces, cannot interrupt a paragraph, and
  duplicate labels resolve to the last definition;
- reference labels keep formatting characters (`` [`x`] `` ≠ `[x]`) and are
  only case-folded and whitespace-collapsed;
- citation keys drop trailing punctuation, and `(@label)` is a citation
  unless an example-list marker defined the label;
- inline code spans may wrap across lines;
- heading titles drop HTML comments, HTML tags, and emphasis markers before
  identifier computation, and trailing attribute blocks (`{.options}`) are
  always consumed;
- email autolinks resolve to `mailto:`;
- `[label]:` takes the *entire* next non-blank line as its destination
  (verified: `- list item` and even a fence line become the target), a
  title-only line after a same-line definition is consumed, a trailing
  definition at end of document still registers, and blank lines end grid
  tables.

The LSP layer was hardened in the same pass: wrapped-construct ranges are
byte-exact (the wrap joiner occupies exactly the newline bytes, so hover,
definition, and documentLink positions are correct even for CRLF documents),
semantic tokens never cross line boundaries (invalid per the LSP
specification), and continuation definitions report the target line — not
the label line — as their clickable region.

R Markdown and Quarto chunk fences (```` ```{r, …} ```` / ```` ```{{r}} ````)
are rewritten to plain fences before pandoc sees them: they are invalid
`fenced_code_attributes` syntax, so plain pandoc misreads them as inline
code and turns `#`-comments inside chunks into headings. The language
server keeps treating them as code fences, which is the useful behavior for
Rmd/qmd authors.
