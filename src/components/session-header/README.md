# components/session-header/

The selected-session header: identity, type and lifecycle badges, purpose,
the close-impact callout, the compact metadata line and the action group
(T06 #7, docs/UI_STYLE_GUIDE.md §5).

Button availability derives from `availableActions` in
`src/state/derivations.ts` (lifecycle rules of MVP_IMPLEMENTATION_SPEC.md §5,
and "the config has a target to open" for the two open actions) — never UI
guesses. Start and Stop are mutually exclusive. The Directory and Open Website
buttons act on configured targets. The header has no overflow menu.

A temporary terminal has a direct Close button: the registry awaits the backend's
stop before requesting removal, and retains the session if either operation
fails. Duplicate close clicks are disabled while that operation is pending.
A saved application has a direct Remove button that opens the existing confirmation.
Save Launch Configuration lives in Details for temporary terminals.

The header reports actions by their identifiers through `onAction`; App and the
registry perform them against Session Core. Preview mode displays a notice.
The status badge reads `isReady` (T08 #9): `Running` says the process is there,
`Ready` says the port it was configured with answers or its shell is attached.
It is derived from the snapshot each render, so a health reading arriving while
the window is open flips the badge without the view holding a copy.
