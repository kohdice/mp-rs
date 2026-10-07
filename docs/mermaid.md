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
