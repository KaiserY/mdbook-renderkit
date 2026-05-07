#set page(paper: "a4", margin: 24mm)
#set text(size: 11pt, lang: "zh")
#set heading(numbering: "1.1")

#show link: underline
#show raw.where(block: true): block.with(
  width: 100%,
  fill: luma(245),
  inset: 8pt,
  radius: 3pt,
)

#align(center, text(18pt, weight: "bold")[
  MDBOOK_RENDERKIT_TITLE
])

#pagebreak()
#outline(depth: 3)
#pagebreak()

/**** MDBOOK_RENDERKIT_CONTENT ****/
