# components/title-bar/

The window's only title bar (T06 #7, #68; `docs/UI_STYLE_GUIDE.md` §3, D-028).

Left: the Hub icon (`assets/brand/hub-mark.svg`, rendered from the same file the
Windows icon set is rasterized from — one drawing, two sizes) and the
application name, plus the dev-only `UI 预览` pill. Right: the global run
summary and the window controls.

## It is the window's title bar, not a picture of one

`src-tauri/tauri.conf.json` opens the main window undecorated, so this bar is
where drag, double-click-maximize and the window buttons live:

- `data-tauri-drag-region="deep"` on the `<header>` — Tauri's `drag.js` walks up
  from the click target, so clicks on the mark, the name and the summary start a
  drag, while the `<button>`s (which `drag.js` refuses to treat as drag regions)
  do not. Double-clicking the region is Windows' maximize toggle.
- `WindowControls` — minimize, maximize/restore and close. Close is the same
  gesture the system's X was: the app's window-event handler hides the window to
  the tray and every managed session keeps running (D-006).
- The four `core:window:` permissions these need are granted in
  `src-tauri/capabilities/default.json`; `cargo test` fails if one goes missing.
- Resizing is *not* here: it comes from the native hit-test border Tauri attaches
  to an undecorated resizable window.

`WindowControls.tsx` is markup and glyphs only. What a control means, and how the
maximize glyph stays a reading of the window rather than a local toggle, is
`src/app/window-controls.ts` (tested in the node environment) reached through
`src/app/tauriWindowHost.ts` + `src/app/useWindowControls.ts`. In a browser
preview there is no window, so the host is `null` and the controls render inert —
they keep their place in the bar because that is the real layout.
