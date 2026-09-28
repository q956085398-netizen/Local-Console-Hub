# components/sidebar/

The compact grouped session list (T06 #7, docs/UI_STYLE_GUIDE.md §4).

Status dots, not decorative per-session icons. Rows render from session
config + runtime DTOs via the pure derivations in `src/state/derivations.ts`;
groups come from the fixture group list, and search filters by name, port,
purpose, id and type.
