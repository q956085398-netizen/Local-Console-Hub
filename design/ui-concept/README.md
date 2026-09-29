# UI concept archive — "Ledger" (superseded)

Archived design exploration, kept for history only. **Nothing here is normative
and nothing here is implemented.** The approved UI direction is V2 (dark), per
`docs/UI_STYLE_GUIDE.md` and `docs/DESIGN_SPEC_EXTRACTED.md`.

## What this is

`prototype.html` is a self-contained, build-free concept prototype for an
alternative V2 direction called 「台账 / Ledger」: a warm paper-and-ink light
theme, sourced from `docs/PRODUCT_SPEC.md`, `docs/DECISIONS.md`,
`docs/LOGGING.md` and `docs/UI_STYLE_GUIDE.md` at the time it was written.

The direction was **not** adopted. The shipped V2 surface is dark, and its
tokens are transcribed from `E:/Grok-UI-Design/LocalConsoleHub` into
`src/index.css`. No document or source file in this repository references the
Ledger palette.

## Contents

| File | What it is |
| --- | --- |
| `prototype.html` | The prototype: tokens, layout, state machine and event delegation in one file. Open it directly in a browser; it needs no build and makes no network requests (web fonts aside — it falls back to system fonts offline). |
| `_v.js` | A one-shot verifier for the prototype. Run `node _v.js` from this directory: it checks encoding, tag closure, `id` references, JS syntax, and drives `render` plus the event delegation and state machine inside a fake DOM. |

A third scratch file, `_g.js` (a two-line snippet that dumped a fixed line
range of `prototype.html`), was dropped when this was archived.
