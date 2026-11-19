# Fonts of the PDF report

The PDF report (`src/pdf.rs`, the `pdf` feature) is set in two families. Their files
are compiled into the binary, so a report looks the same on every machine and the
same inspection gives the same bytes. A PDF embeds the glyphs it uses.

| File | Family, style | Licence | SHA-256 |
|---|---|---|---|
| `LibertinusSerif-Regular.otf` | Libertinus Serif, regular | SIL Open Font License 1.1 (`LICENSE-LibertinusSerif.txt`) | `fcf06307a77367394fcb0ccb241e59eea70dba3d732be309647611224679c733` |
| `LibertinusSerif-Semibold.otf` | Libertinus Serif, semibold | SIL Open Font License 1.1 (`LICENSE-LibertinusSerif.txt`) | `a4b3f28e85881db34695c1f005e4c79233a6caf3a2bd286c9b418c025fb99308` |
| `DejaVuSansMono.ttf` | DejaVu Sans Mono, regular | Bitstream Vera and Arev terms; DejaVu changes are public domain (`LICENSE-DejaVu.txt`) | `b4a6c3e4faab8773f4ff761d56451646409f29abedd68f05d38c2df667d3c582` |

The files and both licence texts are copied unchanged from the `typst-assets` crate,
version 0.15.1 (`files/fonts/` and `NOTICE`). They are kept here, and the crate's own
`fonts` feature is left off, for two reasons:

- That feature compiles seventeen font files into the binary, fourteen of which the
  report never uses.
- One of those fourteen, `NewCM10-Regular.otf`, is under the GNU GPL version 3 or
  later with a font exception and a distribution exception. Leaving the feature off
  keeps it out of a binary that is otherwise MIT or Apache-2.0.

These are the only weights the template asks for (`src/report.typ`). A weight or style
it does not find here is not replaced by a system font: the compiler is given these
three files and nothing else. Text in a script neither family covers (Chinese,
Japanese, Korean, Arabic, among others) is typeset as missing glyphs; the page
(`--report`) has no such limit, because the reader's own fonts draw it.

To change a font: replace the file, update the hash above, and run
`cargo test -p halftone-report --features pdf`.
