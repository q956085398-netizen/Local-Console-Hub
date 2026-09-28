# components/sidebar/

The compact grouped session list (T06 #7, docs/UI_STYLE_GUIDE.md §4).

Status dots, not decorative per-session icons. Rows render from session
config + runtime DTOs via the pure derivations in `src/state/derivations.ts`;
search filters by name, port, purpose, id and type.

Groups are a UI concern with no landed config field: the fixture workspace
uses the reference prototype's three groups (so the shell can be compared
against the images), while a live workspace renders its sessions under the
single `LIVE_GROUP` — named for where they came from rather than inventing a
classification the config does not have (`src/state/session-view.ts`).
