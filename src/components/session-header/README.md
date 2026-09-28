# components/session-header/

The selected-session header: identity, type and lifecycle badges, purpose,
the close-impact callout, the compact metadata line and the action group
(T06 #7, docs/UI_STYLE_GUIDE.md §5).

Button availability derives from `availableActions` in
`src/state/derivations.ts` (lifecycle rules of MVP_IMPLEMENTATION_SPEC.md §5)
— never UI guesses. Start and Stop are mutually exclusive; the overflow menu
holds the low-frequency and destructive actions (directory, focus terminal,
log policy, copy path, force-kill the managed tree per D-007).
