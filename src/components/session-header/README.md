# components/session-header/

The selected-session header: identity, type and lifecycle badges, purpose,
the close-impact callout, the compact metadata line and the action group
(T06 #7, docs/UI_STYLE_GUIDE.md §5).

Button availability derives from `availableActions` in
`src/state/derivations.ts` (lifecycle rules of MVP_IMPLEMENTATION_SPEC.md §5)
— never UI guesses. Start and Stop are mutually exclusive; the overflow menu
holds the low-frequency and destructive actions (directory, focus terminal,
log policy, copy path, force-kill the managed tree per D-007).

The header reports an action by its identifier, not by the label it rendered:
`onAction` hands over a `SessionAction` (`src/state/actions.ts`) and the *caller*
decides what it means (`src/app/App.tsx`) — a control says what it was asked to
do, and the words are presentation. With a backend connected, `start` / `stop` /
`restart` / `force-stop` are Session Core commands; in the preview workspace
there is no run behind them, so they surface a notice instead of pretending to
act. The buttons themselves are identical in both modes — the lifecycle rules
they render are the same rules either way.
