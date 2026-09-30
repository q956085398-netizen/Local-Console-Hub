# state/

Frontend view state only (selection, tab visibility, transient UI state).

Per `docs/MVP_IMPLEMENTATION_SPEC.md` §3 the UI must **not** own process
truth — runtime state belongs to Session Core in the Rust backend. Do not add
a runtime session model here: runtime state arrives as `SessionRuntimeDto`
and lifecycle changes originate in Session Core.

- `derivations.ts` — pure view rules over the DTOs (status tones and labels,
  action availability, callout wording, metadata pairs, row chips, grouping,
  filtering). Unit-tested; components stay thin wrappers around these.
- `session-view.ts` — `SessionView`, the shape a row and the workspace render:
  a session's validated configuration, its runtime snapshot, and the UI-only
  extras. It is what the workspace is made of, whichever source filled it.
- `fixtures.ts` — the T06 fixture workspace, rendered when no backend answers.
- `logs.ts` — the Logs tab's rules (T10): the state badge, "is this being
  logged?", which file an action points at, whether a run's log is still on
  disk (retention deletes files, not run records — `DECISIONS.md` D-022), and
  the retention wording. Also turns a session view into the payloads
  `get_log_info` / `get_run_history` answer with, so the tab renders live and
  preview data through one path.
- `terminal-stream.ts` — where a terminal view is in the stream it renders,
  and whether a live batch joins it or is already shown. Pure; unit-tested.
- `terminal-attach.ts` — the attachment protocol (subscribe, replay, input,
  resize) against an injected `TerminalBackend`, so it is testable without a
  DOM or a Tauri host.
- `backend-connection.ts` — is a host answering the typed ping? Transport
  liveness only: the three readings (`pending` / `connected` / `unavailable`),
  the shared bounded backoff that keeps `unavailable` from being permanent,
  and the stop-on-first-answer rule. The ping is injected, the same way
  `terminal-attach.ts` injects a backend. It is deliberately **not** a runtime
  model — see the note below.
- `retry-schedule.ts` — the shared capped retry cadence used by transport and
  session startup watchers.
- `window-controls.ts` — the window operations the merged title bar owns (#68):
  what each control means, which of them the OS refused, and whether the window
  is maximized. The window is injected, so the rules run in the node test
  environment; a browser preview has no window at all. This is not session
  truth either — `tauri.conf.json` and the tray's close handler decide what the
  window *is* (D-028).
- `session-registry.ts` — coordinates the initial configured-session snapshot
  with full `session-state-changed` events through an injected backend. It
  buffers a bounded set of latest events until configs arrive, resynchronizes
  if that bound is exceeded, retries failed reads on the shared capped
  cadence, and emits `SessionView[]`; it does not decide lifecycle state.
- `view.ts` — UI-only vocabulary (workspace tabs).

## Transport liveness is not session runtime truth

`backend-connection.ts` answers one question: is the Hub's own host answering
its typed ping? A host that stops answering says nothing about any session —
the sessions keep running whether or not the window can reach the backend —
and the module therefore holds no per-session state and never reports a
session or terminal failure. A failed ping is "no host is answering", which is
what puts the shell in the preview workspace; the runtime model stays where
this file's first paragraph puts it.

## Where the sessions come from (T07 #8)

`src/app/useSessionRegistry.ts` reads Session Core when a backend is answering
(`list_session_configs` + `list_sessions`, then the `session-state-changed`
events) and falls back to `FIXTURE_SESSIONS` only when none is. While a live
backend's first snapshot is loading or retrying, the shell renders no session
rows until it has validated config; it does not show fixture sessions as if
they belonged to that backend. The two populated sources fill the *same*
`SessionView`, so the shell renders one model, not two.

`SessionView`'s extras beyond the landed DTOs are documented per field on the
type and are not a second runtime model:

- `busy` — spec §4's optional runtime flag. `src-tauri/src/session/state.rs`
  notes it does not exist yet, and nothing produces it: the MVP has no source
  for "the application is working" (a port says whether a service is
  *reachable*, not whether it is free), and the adapter that could tell is
  outside T08's scope. It supplements lifecycle state and never replaces it.
- `ready` is deliberately **not** an extra on the view model (T08 #9): it is
  derived from the snapshot on every render (`derivations.isReady` — a service
  whose port answers, or a terminal whose shell is attached), so a health
  reading arriving mid-render cannot leave a stale copy behind.
- `group` — a UI concern with **no landed field and no owning ticket** (the
  config schema has none). Live sessions therefore render under one group,
  `LIVE_GROUP`, named for where they came from. A config-schema issue must add
  the field before anything depends on a classification.
- `dependsOn` — likewise unlanded; the details panel renders what it is given
  (the live source provides nothing).
- `lines` — the preview stream, read only when no backend is attached. With a
  backend, the PTY is the stream and this field is never rendered.
- `runs[].logFilePresent` — the entry-level field the real payload carries
  around a run record (`RunHistoryEntryDto`, D-022). The view model has no entry
  wrapper, so the one run whose log was swept says so here.
