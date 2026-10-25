# Mermaid diagrams

`mp` renders a ` ```mermaid ` block as a character drawing when its diagram type has a
section on this page; any other type renders as a plain code block. A drawing is built
from characters, so it only approximates what a browser shows. Each section below names
the syntax that is supported and how the drawing differs from a browser. Differences
that apply to every type are in [README.md](../README.md), "Mermaid diagrams".

## Flowchart (`flowchart`, `graph`)

Nodes render as boxes joined by box-drawing lines. Shapes are made of box-drawing glyphs
and lines run horizontally and vertically only. Colors set with `style`, `classDef`,
`class`, `:::` and `linkStyle` are applied when color output is on.

### Supported syntax

The syntax follows the [Mermaid flowchart documentation](https://mermaid.js.org/syntax/flowchart.html):

- **Header and direction.** `flowchart`, `graph` or `flowchart-elk`, followed by `TB`,
  `TD`, `BT`, `LR`, `RL` or `BR`, or by `v`, `^`, `>` or `<`; no direction means
  top-down. `BR` is drawn top-down, as Mermaid draws it. Statements end at a line break
  or `;` outside labels and link text.
- **Nodes.** A bare id, or an id with a bracketed label: `[ ]`, `( )`, `{ }`, `([ ])`,
  `[[ ]]`, `[( )]`, `(( ))`, `((( )))`, `{{ }}`, `> ]`, `[/ /]`, `[\ \]`, `[/ \]`,
  `[\ /]` and `(- -)`. `id@{ shape: …, label: … }` takes every shape name and alias
  Mermaid lists; the undocumented `state`, `choice`, `note` and `composite` draw as a
  rounded box, a diamond and plain boxes. A label holding `( ) [ ] { } |` or `"` must be
  quoted (`A["f(x)"]`), as in Mermaid; an unquoted one is a syntax error. The labels of
  `[/ /]`, `[\ \]`, `[/ \]` and `[\ /]` may hold `|` and `"` unquoted, as in Mermaid,
  but not a `"` right after a `/` or `\` (`A[/a/"b/]`), which is a syntax error. An
  empty label (`A[]`, `A[""]`) is a syntax error too, as in Mermaid, while `A[ ]` is
  blank. An id
  may hold `:` and `,` (`ns:a`, `A,B`), as in Mermaid, while `:::` after an id
  attaches a class.
- **Labels.** Quoted strings, which may span lines, each line a row, and so may
  unquoted labels in brackets; markdown strings (``"`…`"``) with bold and italic, where
  a backslash escapes the ASCII punctuation character after it (`\*` is a plain `*`);
  `<br>` line breaks, and in labels other than markdown strings `\n` as well; entity
  codes such as `#quot;`, `#35;` and HTML names. A label may go on in plain text after a
  leading quoted string (`A["hi" there]`), which then decides whether the whole label is
  a markdown string; a second quoted string after it is a syntax error, as in Mermaid.
- **Links.** `-->`, `---`, `-.->`, `-.-`, `==>`, `===`, `~~~`, with `o` and `x` ends and
  `<`, `o` or `x` at the start for two-headed links (`<-->`, `o--o`, `x--x`); a start
  character that does not match the end is part of the line, and makes an `==` link a
  normal one, as in Mermaid (`A <--- B` is `A ---- B`, `A <=== B` is `A ---- B`);
  longer links (`--->`, up to 10 layers, as in Mermaid); labels as `-->|text|` or
  `-- text -->`, but not both on one link, where the text of `-- text -->` may span
  lines, each line a row; chains (`A --> B --> C`); `&` groups
  (`A & B --> C`), which need a blank on both sides of `&`, as in Mermaid; edge ids
  (`A e1@--> B`). The text of `-- text -->` may hold `( ) [ ] { } |`, but a `"` only as
  the start of a leading quoted string (`A -- "hi" there --> B`), as in Mermaid;
  otherwise it is a syntax error. A `-- text -->` link without text (`A -- --> B`) is a
  syntax error, as in Mermaid, and so is one whose start does not match its end
  (`A o-- text --> B`; see below).
- **Subgraphs.** `subgraph id [title]` … `end`, nested, or `subgraph title` without
  brackets, whose title may start with a quoted string and may not hold
  `( ) [ ] { } | < > , @ ~`, a link (`-->`, `---`, `~~~`, `-.-`, `==>`, …), the start of
  one (`--`, `==`, `-.`) or a `"` at the start of a word, as in Mermaid; a `"` inside a word (`subgraph a"b`) is text;
  links to and from a subgraph's frame; `direction` inside a subgraph, ignored when a
  link joins something inside the subgraph to something outside it, as in Mermaid; a
  subgraph without
  `direction` and without links to the outside is laid out `LR` inside a `TB` or `TD`
  chart or a subgraph laid out `TB`, by default or by `direction TB`, and `TB` inside
  any other, a `direction TD` subgraph included, as Mermaid does by default
  (`rankdir === 'TB' ? 'LR' : 'TB'`,
  `flowchart.inheritDir: false`); links entering a subgraph drawn in its own direction
  keep clear of its title, and its frame widens to make room for them;
  `id@{ view: collapsed }` after the subgraph's `end`
  draws it as one box. A second block with an id already used adds its members to the
  first, which keeps its title, direction and place, and a node or subgraph listed in
  the blocks of two subgraphs belongs to the one whose block ends first, as in Mermaid.
  A block whose unbracketed title holds a blank (`subgraph My Group`) has no id, as in
  Mermaid, so such blocks are never merged and cannot be linked to or styled.
  A `subgraph` with neither id nor title is a syntax error; Mermaid fails to render it.
- **Frontmatter.** A `---` block on the first line; its `title` is drawn above the
  diagram, on one row with each run of blanks and line breaks drawn as one space, as a
  browser draws it. The value may follow the key on its line or start on a later one,
  comment lines before it passed over, and continue as a block scalar (`title: |`,
  `title: >`), as indented lines, or, when quoted, up to its closing quote. A list is
  drawn as its items joined by commas and a mapping as `[object Object]`, as Mermaid
  draws what JavaScript writes for them. A block that is one JSON-like flow mapping
  (`{ title: … }`) is read as that mapping. A top-level key written twice is a syntax
  error, as in Mermaid. So, as in Mermaid, are text left after the title's value, a
  line of the title indented where it does not fit, a line holding no key after a key
  or before the first one (unless the whole block is one value, such as a quoted string
  or a flow collection), a line indented with a tab, text left after a quoted item or
  nested collection inside a flow collection, and a quote or bracket that nothing
  closes in any key's value.
- **Styling.** `style`, `classDef` (including `default` and `node`), `class`, `:::` and
  `linkStyle` (indexes, lists and `default`). A property list may hold only the
  characters of Mermaid's style tokens: letters, digits, blanks and
  ``! " # $ % & ' * + , - . / : = ? \ _ ` ``; any other, such as the `(` of
  `fill:rgb(255,0,0)`, is a syntax error, as in Mermaid. So are a `"`, `&` or `*`
  starting a word, a word that is just `v`, and `--`, `-.`, `->` or `==` anywhere
  (`font-family:"Arial"`, `stroke-dasharray:5--5`), while `fill:violet` and `fill:a"b`
  are accepted, as in Mermaid.
- **Other statements.** `%%` comment lines, `%%{init: …}%%` directives,
  `accTitle` / `accDescr`, `click` with a callback (`call f()`, bare `f`) or a link
  (`href "url"`, `"url"`), as in Mermaid; a `click` with neither, or a `call` without
  an argument list, is a syntax error, as in Mermaid.

### Differences from a browser

These are read but have no effect:

- `click` statements, `linkStyle … interpolate …` curve names, and the `curve`,
  `animate` and `animation` keys of a link's `e1@{ … }` data: a terminal drawing is
  static and its lines run along rows and columns.
- `%%{init: …}%%` directives and the frontmatter's `config`: themes, curves and HTML
  label settings apply to the browser's drawing only.
- Style properties other than `stroke`, `color`, `fill`, `stroke-width`,
  `stroke-dasharray`, `font-weight` and `font-style`, and `!important`, which is
  dropped so that declarations apply in order.

These are drawn differently:

- A `stroke-width` of 3px or more draws heavy lines and borders, a narrower one light
  ones; any `stroke-dasharray` but `none` or zero lengths draws dotted ones. Color,
  bold, italic, heavy and dotted are all shown.
- `stroke:none`, `stroke:transparent` and `color:transparent` reset the border, line or
  text to the default color instead of hiding it; `fill:none` and `fill:transparent`
  remove the fill. A hex color with zero alpha (`#f000`, `#ff000000`) counts as
  `transparent`; any other alpha is dropped.
- On a shape open on the right, such as `brace`, `text` or `datastore`, `fill` colors
  only the label's characters.
- FontAwesome tokens such as `fa:fa-car` are dropped from labels; a label made only of
  icons shows their names.
- A subgraph title with line breaks is drawn on one row.
- A frontmatter `title` that js-yaml reads as a number or a boolean, such as `1.0`,
  `0x10` or `True`, is drawn as written; Mermaid draws what JavaScript writes for the
  value: `1`, `16`, `true`.
- Markdown strings are not wrapped automatically; they break only at written line
  breaks and `<br>`.
- `@{ label: "…" }` values and the frontmatter's `title` keep YAML escapes other than
  `\"` and `\\` as written. In a label, `\n` still breaks the row, as a decoded line
  break does, except in a markdown string, where it stays as written.
- An unknown entity code such as `#nosuch;` is shown as written.
- A drawing too wide for the terminal first tightens its spacing, inside a subgraph laid
  out in a direction of its own as well as around it; a browser shrinks the picture
  instead.
- A link to or from a subgraph counts, when cycles are broken, as one link between each
  pair of members and the other end, and only the pairs that close a cycle are laid out
  reversed. When some pairs of a link are reversed and others are not, the link is
  drawn in the direction of the pairs that are not, meeting the subgraph's frame on the
  side facing its other end; for a link between two subgraphs, it meets whichever frame's
  layers span the other end. A browser instead attaches such a link to one member of the
  subgraph and reverses or keeps that one link.
- Two links share a cell only where they cross, drawn as `┼`, as a browser's curves
  cross without merging. When two sources' links swap cells, each entering its target
  by the cell the other leaves its source by, their lines share the cells between their
  turns and are drawn joined, as `┤` and `├` (`┴` and `┬` in a horizontal layout): a
  character grid cannot draw one line through another without a junction.
- In a chart with subgraphs, nodes are put in layers by the same rule as without them:
  the links together span the fewest layers, a link to or from a subgraph counting as
  one link per member. A browser also ranks the hidden nodes and links it adds to
  border each subgraph, so the layers of nodes inside and around a subgraph may differ.
- The search for the layers that the links together span the fewest of stops after
  10,000 changes of its spanning tree, where a browser searches on, so such a chart may
  span more layers than in a browser. This is a safety bound; no chart that reaches it
  is known.
- A link between a subgraph and a node or subgraph inside it (`A --> s` where `A` is
  inside `s`, in either direction) is not drawn, as in a browser; a browser still keeps
  room in the subgraph for the hidden link, and here the subgraph is drawn as if the
  link were not written.

These are syntax errors here:

- A `-- text -->` link whose start marker does not match its end (`A o-- text --> B`);
  Mermaid stores it as an invalid link, and a browser's drawing of it is undefined.

These render as the plain code block:

- `~~~|text|`: an invisible link has no line to carry the text.
- A node with `icon` or `img` in its `@{ … }` data, and the shapes `anchor` and `icon`.
- A drawing still too wide for the terminal at the tightest spacing, or larger than
  1,000,000 cells.
- Subgraphs laid out in a direction of their own, given by `direction` or crosswise,
  nested more than 32 deep.
- `A -->|text| s` or `s -->|text| A`, where `A` is a node or subgraph inside `s`: the
  link is not drawn, so the text has no line to ride, as with `~~~|text|`.
- A link between two subgraphs whose member pairs disagree on their direction (see
  above), when neither subgraph's layers span the link's other end: one line cannot
  meet both frames.
- Charts whose subgraph frames still change size after being measured again once per
  subgraph while links are fitted to their borders, which would otherwise make two
  links share a cell. This is a safety bound; no chart that reaches it is known.

A chart with more than 500 links is reported as a syntax error, as Mermaid reports it.
A chart longer than Mermaid's `maxTextSize` of 50,000 characters is drawn as the one box
"Maximum text size in diagram exceeded", as Mermaid draws it. The length is counted as
Mermaid counts it: in UTF-16 units, so that a character such as `😀` counts twice,
and without the frontmatter, the `%%{…}%%` directives and the `%%` comment lines.
