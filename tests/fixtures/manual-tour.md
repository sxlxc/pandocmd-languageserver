---
title: Pandoc Markdown Feature Tour
subtitle: Exercises the constructs documented in the Pandoc User's Guide
author:
  - Author One
  - Author Two
date: 2024-06-01
bibliography: tour.bib
lang: en
---

# Introduction {#sec:introduction}

Welcome to a tour of Pandoc's Markdown as documented in the Pandoc User's
Guide. This document doubles as a test fixture: the language server must
analyze it without producing any diagnostics, and `pandoc -f markdown` must
parse it without errors.

Non-paragraph text below a heading needs a blank line, since the
`blank_before_header` extension is enabled by default.

## Inline formatting {#sec:inline}

*Emphasis*, **strong emphasis**, ***both***, `inline code`, ~~strikeout~~,
superscript 2^10^, subscript H~2~O, escaped \*asterisks\*, and a hard line break\
right here.

Smart typography with the `smart` extension: "curly quotes", 'single
quotes', an em dash---like this---, an en dash -- like this, and an
ellipsis...

Unicode and identifiers work together: Über Müller and 郭德纲相声.

## Links and images {#sec:links}

An [inline link](https://pandoc.org "Pandoc"), an <https://autolink.example>,
an ![inline image](logo.png "Logo"), and a bare ![paragraph image](figure.png)
that becomes an implicit figure.

Reference links: full [reference][ref-full], collapsed [ref-collapsed][],
and shortcut [ref-shortcut]. Definitions appear at the bottom.

An internal heading link to [the introduction](#sec:introduction), plus
attribute forms: [a bracketed span]{#span-example .kw key="value"}, an
inline attribute on code `let x = 1;`{#code-example .rust}, and a link with
attributes [styled](https://example.com){.external}.

## Math {#sec:math}

Inline math $a^2 + b^2 = c^2$ and display math:

$$\int_0^\infty e^{-x^2}\,dx = \frac{\sqrt{\pi}}{2}$$ {#eq:gauss}

## Citations {#sec:citations}

Blah blah [see @doe2004, pp. 33-35; also @smith2020, chap. 1].

In-text citation: @doe2004 says blah, and a suppressed form [-@smith2020].

A citation with a local cross-reference target: see [@eq:gauss].

## Fenced code {#sec:code}

``` {#lst:demo .rust .numberLines startFrom="3"}
fn main() {
    println!("fenced code with attributes");
}
```

```python
print("plain fenced code")
```

~~~~
Tilde fences work too, even with [^not-a-footnote] and [@not-a-citation]
inside as literal text.
~~~~

## Fenced divs {#sec:divs}

::: {#panel .warning key="value"}
A fenced div with attributes.

::: {.nested}
Divs nest.
:::
:::

::: lemma
An unbraced class name works too, and the div may carry a caption.
:::

## Tables {#sec:tables}

Pipe table:

| Right | Left | Default | Center |
|------:|:-----|---------|:------:|
|    12 | 12   | 12      |   12   |
|   123 | 123  | 123     |  123   |
|     1 | 1    | 1       |    1   |

: Demo pipe table {#tbl:pipe}

Grid table:

+---------------+---------------+--------------------+
| Fruit         | Price         | Advantages         |
+===============+===============+====================+
| Bananas       | $1.34         | built-in wrapper   |
+---------------+---------------+--------------------+
| Oranges       | $2.10         | cures scurvy       |
+---------------+---------------+--------------------+

Simple table:

  Weak     Strong
  ------   ------
  1        2
  ------   ------

: Simple table demo

Multiline table:

-------------------------------------
 Centered   Default           Right
  Header    Aligned         Aligned
----------- ------- -----------------
   First    row                12.0
  Second    row                 5.0
----------- ------- -----------------

: Multiline table demo {#tbl:multiline}

## Lists {#sec:lists}

Task lists with the `task_lists` extension:

- [ ] unchecked item
- [x] checked item

Fancy lists with `fancy_lists` and `startnum`:

(2) two
(3) three

#. next automatically numbered
#. after that

Term lists with `definition_lists`:

Term 1
: Definition for term 1.

Term 2
: First definition.
: Second definition.

Example lists with `example_lists`:

(@)  A good example.
(@)  Another one.

(@good)  A labeled example referenced again below.

As (@good) illustrates, examples can be referenced.

Line blocks with `line_blocks`:

| The limerick packs laughs anatomical
| In space that is quite economical.

## Block quotes {#sec:quotes}

> Block quotes are indented.
>
> > They nest.


## Raw content {#sec:raw}

Raw inline HTML like <abbr title="HyperText Markup Language">HTML</abbr>
passes through with `raw_html`, and raw attributes with `raw_attribute`:

```{=html}
<p>Raw HTML block.</p>
```

`<b>inline raw</b>`{=html}

Footnotes
=========

Setext-style headings still work.[^longnote]

[^longnote]: Here is a long footnote with multiple paragraphs.

    Indented continuation.

    Second paragraph.

An inline note.^[Inline notes are shorter.]

Definitions
===========

[ref-full]: https://example.com/full "Full reference"
[ref-collapsed]: https://example.com/collapsed
[ref-shortcut]: https://example.com/shortcut

# Conclusion {#sec:conclusion}

That covers the tour. See [the math](#sec:math), [@tbl:pipe],
[@lst:demo], and [the introduction](#sec:introduction) again.
