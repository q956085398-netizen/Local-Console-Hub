# state/

Frontend view state only (selection, tab visibility, transient UI state).

Per `docs/MVP_IMPLEMENTATION_SPEC.md` §3 the UI must **not** own process
truth — runtime state belongs to Session Core in the Rust backend. This
directory is reserved for T06+; do not add runtime session models here.
