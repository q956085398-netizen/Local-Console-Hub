# state/

Frontend view state only (selection, tab visibility, transient UI state).

Per `docs/MVP_IMPLEMENTATION_SPEC.md` §3 the UI must **not** own process
truth — runtime state belongs to Session Core in the Rust backend. Do not add
a runtime session model here: runtime state arrives as `SessionRuntimeDto`
and lifecycle changes originate in Session Core.

- `backend-connection.ts` — **transport liveness, and only that**: is a host
  answering the typed ping? It is the first question the shell asks, and the
  one it keeps asking — a lost request is not a permanent answer, so
  `unavailable` re-asks on a bounded, documented backoff, a question nobody
  answers counts as no answer after `QUESTION_TIMEOUT_MS`, and the first reply
  stops the asking (`#29`). It holds no session state and no error text: a
  failed ping means "no host is answering", never that a session or a terminal
  failed.
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
- `view.ts` — UI-only vocabulary (workspace tabs).

## Where the sessions come from (T07 #8)

`src/app/useSessionRegistry.ts` reads Session Core when a backend is answering
(`list_session_configs` + `list_sessions`, then the `session-state-changed`
events) and falls back to `FIXTURE_SESSIONS` when none is — the browser
preview, and the moment before the first listing returns. The two sources fill
the *same* `SessionView`, so the shell renders one model, not two.

"Answering" is `backend-connection.ts`'s answer, not this hook's: a host that
was down at startup and replies later flips this state to `connected`, which is
the same transition the first listing already rides — so the live workspace
takes over without a restart, and the fixture workspace keeps its precedence
right up until then.

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
