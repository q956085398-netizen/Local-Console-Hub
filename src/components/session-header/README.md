# components/session-header/

The selected-session header: identity, type and lifecycle badges, purpose,
the close-impact callout, the compact metadata line and the action group
(T06 #7, docs/UI_STYLE_GUIDE.md §5).

Button availability derives from `availableActions` in
`src/state/derivations.ts` (lifecycle rules of MVP_IMPLEMENTATION_SPEC.md §5,
and "the config has a target to open" for the two open actions) — never UI
guesses. Start and Stop are mutually exclusive; the overflow menu holds the
low-frequency and destructive actions (directory, focus terminal, log policy,
copy path, force-kill the managed tree per D-007). The controls that act on the
working directory — `目录` plus the menu's `打开目录`, and the menu's `复制路径`
— are offered only where the config has a `cwd` to act on.

The header reports an action by its identifier, not by the label it rendered:
`onAction` hands over a `SessionAction` (`src/state/actions.ts`) and the *caller*
decides what it means (`src/app/App.tsx`) — a control says what it was asked to
do, and the words are presentation. With a backend connected, `start` / `stop` /
`restart` / `force-stop` are Session Core commands, and so are the two open
actions (`open_session_url`, `open_session_cwd` — the session's own configured
URL and working directory, resolved in Core, T08 #9); in the preview workspace
there is no run behind them, so they surface a notice instead of pretending to
act. The buttons themselves are identical in both modes — the rules they render
are the same rules either way. `复制路径` is the exception in both directions: it
needs no command (the path is already in the config this header renders,
`src/app/clipboard.ts`), so it copies in either mode and reports what the
clipboard did.

The status badge reads `isReady` (T08 #9): `Running` says the process is there,
`Ready` says the port it was configured with answers or its shell is attached.
It is derived from the snapshot each render, so a health reading arriving while
the window is open flips the badge without the view holding a copy.
