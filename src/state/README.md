# state/

Frontend view state only (selection, tab visibility, transient UI state).

Per `docs/MVP_IMPLEMENTATION_SPEC.md` §3 the UI must **not** own process
truth — runtime state belongs to Session Core in the Rust backend.

- `derivations.ts` — pure view rules over the DTOs (status tones and labels,
  action availability, callout wording, metadata pairs, row chips, grouping,
  filtering). Unit-tested; components stay thin wrappers around these.
- `fixtures.ts` — the T06 fixture workspace. Every config/runtime/run value
  passes the landed DTO guards; T07–T10 replace it with live `list_sessions`
  payloads. Fields the DTOs do not carry yet (`group`, `dependsOn`, `busy`,
  `ready`, `lines`) are declared on `FixtureSession` with the ticket that
  moves each into a contract.
- `view.ts` — UI-only vocabulary (workspace tabs).
