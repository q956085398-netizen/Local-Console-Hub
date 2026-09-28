# state/

Frontend view state only (selection, tab visibility, transient UI state).

Per `docs/MVP_IMPLEMENTATION_SPEC.md` §3 the UI must **not** own process
truth — runtime state belongs to Session Core in the Rust backend. Do not add
a runtime session model here: runtime state arrives as `SessionRuntimeDto`
and lifecycle changes originate in Session Core.

- `derivations.ts` — pure view rules over the DTOs (status tones and labels,
  action availability, callout wording, metadata pairs, row chips, grouping,
  filtering). Unit-tested; components stay thin wrappers around these.
- `fixtures.ts` — the T06 fixture workspace. Every config/runtime/run value
  passes the landed DTO guards; T07–T10 replace it with live `list_sessions`
  payloads.
- `logs.ts` — the Logs tab's rules (T10): the state badge, "is this being
  logged?", which file an action points at, whether a run's log is still on
  disk (retention deletes files, not run records — `DECISIONS.md` D-022), and
  the retention wording. Also turns a fixture session into the payloads
  `get_log_info` / `get_run_history` answer with, so the tab renders live and
  preview data through one path.
- `view.ts` — UI-only vocabulary (workspace tabs).

## Fixture boundary (T06 #7)

`FixtureSession` is a **fixture container, not a contract**: components
receive display data as props and never import the fixture arrays (only
`App.tsx` does). The extras it carries beyond the landed DTOs are documented
per field on the type, and are not a second runtime model:

- `busy` / `ready` — spec §4's optional runtime flags. `src-tauri/src/session/
  state.rs` notes neither exists yet; T08 (#9) produces them. They supplement
  lifecycle state and never replace it.
- `lines` — the terminal stream T07 (#8) replaces with the PTY.
- `runs[].logFilePresent` — the entry-level field the real payload carries
  around a run record (`RunHistoryEntryDto`, D-022). A fixture has no entry
  wrapper, so the one run whose log was swept says so here.
- `group` / `dependsOn` — **no landed field and no owning ticket yet** (the
  session config schema has neither). They are recorded as `FixtureExtras`
  with this note rather than presented as contracts; a config-schema issue
  must add them before anything depends on them.

