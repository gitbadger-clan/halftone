// The PDF report's one template, built into the binary (pdf.rs, include_str!).
//
// It draws what the page draws: the same model, read here as JSON. No sentence of a
// report is written in this file; only column labels and layout. A value from the
// model is always a string placed as text, never markup, so a file name or a
// rationale cannot change the document's structure.
//
// One pigment, as on the page: everything is set in the theme's ink on its paper,
// and a verdict is told by the shape of its glyph. The light theme is for paper and
// filing (white page). The dark theme is for reading on a screen; it prints as a
// dark page.
//
// Every section is open: a PDF holds the whole report. Fonts are the two families
// built into the binary, so the same inspection gives the same bytes anywhere.

#let d = json("/report.json")
#let r = d.report
#let paper = rgb(d.theme.paper)
#let ink = rgb(d.theme.ink)
#let soft = color.mix((ink, 72%), (paper, 28%), space: rgb)
#let rule = color.mix((ink, 20%), (paper, 80%), space: rgb)
#let wash = color.mix((ink, 5%), (paper, 95%), space: rgb)

#set document(title: r.title)
#set page(
  paper: "a4",
  margin: (x: 15mm, top: 16mm, bottom: 20mm),
  fill: paper,
  footer: context {
    set text(size: 8pt, fill: soft)
    grid(
      columns: (1fr, auto),
      r.page_footer,
      [Page #counter(page).display() of #counter(page).final().first()],
    )
  },
)
#set text(
  font: "Libertinus Serif",
  size: 10pt,
  fill: ink,
  lang: "en",
  number-type: "lining",
  number-width: "tabular",
  hyphenate: false,
)
#set par(leading: 0.58em, spacing: 0.9em, justify: false)
#show raw: set text(font: "DejaVu Sans Mono", size: 0.9em)

#show heading.where(level: 1): it => block(above: 18pt, below: 8pt, sticky: true, text(
  size: 12.5pt,
  weight: "semibold",
  it.body,
))
#show heading.where(level: 2): it => block(above: 12pt, below: 6pt, sticky: true, text(
  size: 10.5pt,
  weight: "semibold",
  it.body,
))

// Words with literal values set apart, as the model gives them.
#let rich(spans) = {
  spans
    .map(s => if s.kind == "code" { raw(s.text) } else if s.kind == "break" { linebreak() } else {
      s.text
    })
    .join()
}

// A verdict's glyph, sized to the text beside it.
#let glyph(status, size: 0.78em) = box(
  baseline: 0.08em,
  inset: (right: 0.4em),
  image("/glyph/" + status + ".svg", height: size, alt: ""),
)

// Tables: hairlines between rows and nothing else; a row is never split over pages.
// Columns are parted by padding inside the cells, not by a gutter, so a hairline runs
// unbroken across the row.
#set table(stroke: none, inset: (left: 0pt, right: 4mm, y: 4.5pt))
#show table.cell: set block(breakable: false)
#let row-line(y) = if y > 0 { (top: 0.5pt + rule) } else { none }
#let label(body) = text(weight: "semibold", body)
#let head(body) = text(size: 8.5pt, fill: soft, weight: "semibold", body)

#let fields(rows, width: 38mm) = table(
  columns: (width, 1fr),
  stroke: (x, y) => row-line(y),
  ..rows.map(f => (label(f.label), rich(f.value))).flatten(),
)

// ---- masthead

#grid(
  columns: (auto, 1fr),
  column-gutter: 9pt,
  align: horizon,
  image("/mark.svg", height: 27pt, alt: "Halftone"),
  [
    #text(size: 17pt, weight: "semibold")[Halftone report] \
    #text(size: 9pt, fill: soft, r.checked)
  ],
)
#v(-2pt)
#line(length: 100%, stroke: 1.5pt + ink)

// ---- the file

= File
#fields(r.file)

// ---- what differs from a default check

#if r.notes.len() > 0 {
  block(
    width: 100%,
    above: 14pt,
    fill: wash,
    stroke: (left: 3pt + ink),
    inset: (left: 11pt, right: 9pt, y: 8pt),
    {
      label(r.notes_lead)
      list(tight: false, spacing: 0.65em, marker: [•], ..r.notes.map(n => [#n]))
    },
  )
}

// ---- the evidence, a group of rows per layer

= Evidence
#table(
  columns: (31mm, 50mm, 1fr),
  stroke: (x, y) => none,
  table.header(head[Status], head[Source], head[Rationale]),
  ..r
    .evidence
    .map(g => (
      (
        table.cell(colspan: 3, inset: (left: 0pt, top: 9pt, bottom: 4pt), stroke: (bottom: 0.75pt + ink), label(
          g.layer,
        )),
      )
        + g
          .rows
          .enumerate()
          .map(((i, e)) => {
            let quiet = e.status == "not_applicable"
            let colour = if quiet { soft } else { ink }
            let line = if i > 0 { (top: 0.5pt + rule) } else { none }
            (
              table.cell(stroke: line, text(fill: colour, weight: if quiet { "regular" } else {
                "semibold"
              })[#glyph(e.status)#e.word]),
              table.cell(stroke: line, text(fill: colour)[#raw(e.source) \ #text(
                  size: 8.5pt,
                  fill: soft,
                  e.version,
                )]),
              table.cell(stroke: line, text(fill: colour, e.rationale)),
            )
          })
          .flatten()
    ))
    .flatten(),
)

// Layers that were asked for and have no source: said under the rows, in full ink.
#if r.empty_layers != none {
  block(above: 6pt, stroke: (top: 0.75pt + ink), inset: (top: 6pt), width: 100%, r.empty_layers)
}

#block(above: 10pt, text(size: 8.5pt, fill: soft, {
  for item in r.legend {
    glyph(item.status, size: 0.72em)
    text(weight: "semibold", item.word)
    [: #item.text ]
  }
  r.legend_close
}))

// ---- what the file says about itself: unsigned fields, one table per source

#if r.declared != none {
  let dc = r.declared
  heading(level: 1, dc.heading)
  [#upper(dc.summary.first())#dc.summary.slice(1).]
  parbreak()
  text(fill: soft, dc.caveat)
  for t in dc.tables {
    heading(level: 2, t.title)
    let n = t.columns.len()
    table(
      columns: if n == 2 { (62mm, 1fr) } else { (58mm, 38mm, 1fr) },
      stroke: (x, y) => if y > 0 { (top: 0.5pt + rule) } else { none },
      table.header(..t.columns.map(c => head(c))),
      ..t.rows.map(row => row.map(cell => rich(cell))).flatten(),
    )
    for note in t.notes {
      par(note)
    }
  }
}

// ---- conformance measurements of the manifest, when there are any

#if r.conformance != none {
  let c = r.conformance
  heading(level: 1, c.heading)
  if c.state == "not_measured" {
    [Not measured (#c.reason).]
  } else {
    [#c.summary. #c.caveat]
    parbreak()
    text(fill: soft, c.about)
    table(
      columns: (66mm, 26mm, 1fr),
      stroke: (x, y) => none,
      table.header(head[Check], head[Result], head[Report text]),
      ..c
        .rubrics
        .map(rb => (
          (
            table.cell(colspan: 3, inset: (left: 0pt, top: 9pt, bottom: 4pt), stroke: (bottom: 0.75pt + ink), label(
              rb.heading,
            )),
          )
            + rb
              .checks
              .enumerate()
              .map(((i, k)) => {
                let line = if i > 0 { (top: 0.5pt + rule) } else { none }
                (
                  table.cell(stroke: line, raw(k.name)),
                  table.cell(stroke: line, k.result),
                  table.cell(stroke: line, k.text),
                )
              })
              .flatten()
        ))
        .flatten(),
    )
  }
}

// ---- the conditions of the check: build, options, trust lists

#heading(level: 1, r.conditions.heading)
#[#r.conditions.summary.]

#fields(r.conditions.rows)

#let trust = r.conditions.trust
#if trust.state == "not_recorded" {
  par(trust.sentence)
} else {
  heading(level: 2)[Trust lists]
  if trust.state == "none_loaded" {
    par(trust.sentence)
  } else {
    // One cell per list, so a list is never split over two pages. Version and hash
    // get lines of their own, so neither is broken and the hash can be copied whole.
    let widths = (46mm, 52mm, 1fr, 24mm)
    table(
      columns: (1fr,),
      inset: (x: 0pt, y: 4.5pt),
      stroke: (x, y) => none,
      table.header(grid(columns: widths, head[Used for], head[This copy], head[File], head[Certificates])),
      ..trust.rows.map(t => table.cell(stroke: (top: 0.5pt + rule), {
        grid(columns: widths, t.used_for, t.copy, t.file, t.certificates)
        v(-3pt)
        text(size: 9pt, fill: soft)[
          Version #t.version \
          SHA-256 #raw(t.sha256)
        ]
      })),
    )
  }
}

#text(size: 8.5pt, fill: soft, r.conditions.statement)

// ---- the fixed notice

#v(10pt)
#line(length: 100%, stroke: 1.5pt + ink)
#block(width: 82%, breakable: false, text(size: 8.5pt, r.notice))
