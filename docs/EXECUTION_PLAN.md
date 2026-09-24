# Execution Plan / Tickets

> This document mirrors the intended GitHub issue structure for the Windows-first MVP.  
> **Hard dependency means the blocked ticket must not be considered merge-ready until the dependency is complete.**

Related spec: [MVP_IMPLEMENTATION_SPEC.md](./MVP_IMPLEMENTATION_SPEC.md)

## 1. Dependency graph

~~~text
T00 Bootstrap
 ├─> T01 Config + schema
 ├─> T02 PTY/ConPTY
 ├─> T03 Process supervisor
 └─> T06 V1 UI shell

T01 + T02 + T03
 └─> T04 Session runtime/state machine

T04
 ├─> T05 Logging core
 ├─> T07 Interactive terminal integration   (also needs T02 + T06)
 ├─> T08 Service actions/status            (also needs T03 + T06)
 └─> T09 Tray lifecycle                    (also needs T06)

T05 + T06
 └─> T10 Logs view + run history

T07 + T08 + T09 + T10
 └─> T11 End-to-end MVP verification

T11
 └─> T12 Packaging + v0.1.0 RC
~~~

Parallel work is encouraged where the graph allows it.

## 2. Blocker policy

### Hard blocked

A ticket is **hard blocked** when it depends on an unfinished interface or runtime behavior that would otherwise cause duplicated or throwaway implementation.

Examples:

- terminal integration before PTY contract is proven;
- run-history UI before logging/run-record schema exists;
- packaging before end-to-end behavior passes.

### Soft blocked

A ticket is **soft blocked** when work can proceed behind an interface or fixture, but final merge/acceptance requires another ticket.

Examples:

- UI shell can use fixture session data before Session Core exists;
- tray wording/layout can be built before all tray backend actions exist.

### Agent rule

When blocked:

1. do not invent a second competing subsystem;
2. do not bypass the dependency with permanent mocks;
3. leave a concise blocker note on the issue;
4. continue only with portions explicitly marked parallel-safe.

## 3. Contract-first rule

A dependency ticket should land its stable interface before dependents integrate.

Examples:

- T01 lands config DTO/schema;
- T02 lands PTY trait/API and test harness;
- T03 lands process supervisor API;
- T04 lands session events/commands;
- T05 lands logging/run-record API.

Dependents must use those contracts rather than duplicating types.

## 4. Ticket list

### T00 — Bootstrap desktop shell and repository architecture

**Priority:** P0  
**Blocked by:** none  
**Blocks:** T01, T02, T03, T06

Deliver:

- Tauri 2 + React/TypeScript/Vite project;
- Rust/backend and frontend module boundaries;
- formatter/linter/test commands;
- basic CI build check;
- app identity and V1 icon wiring;
- tray can be stubbed, but no business behavior yet.

Acceptance:

- clean checkout builds on Windows;
- dev mode launches one window;
- frontend can invoke one typed backend ping command;
- repository structure matches the architecture spec.

### T01 — Config schema and session model

**Priority:** P0  
**Blocked by:** T00  
**Blocks:** T04, T05, T08

Deliver:

- YAML loading;
- Service/Terminal session schemas;
- logging config schema;
- per-session validation;
- default app-data path handling;
- fixture config for development.

Acceptance:

- duplicate ids fail clearly;
- one bad session does not erase valid sessions;
- parsed model can round-trip through DTOs where required;
- no process is launched in this ticket.

### T02 — Windows PTY/ConPTY validation and abstraction

**Priority:** P0 / release blocker  
**Blocked by:** T00  
**Blocks:** T04, T07

Deliver:

- PTY backend abstraction;
- Windows implementation;
- output stream;
- input;
- resize;
- Ctrl+C test;
- Unicode test;
- high-output smoke test.

Acceptance:

- real PowerShell session is interactive;
- hiding/switching frontend does not terminate PTY;
- no read-only textbox substitute;
- failure modes are surfaced structurally.

### T03 — Process supervisor and safe stop/kill-tree

**Priority:** P0 / safety blocker  
**Blocked by:** T00  
**Blocks:** T04, T08

Deliver:

- managed-process handle;
- PID tracking;
- descendant/process-tree strategy;
- graceful stop;
- force stop;
- exit notification;
- timeout behavior.

Acceptance:

- managed service can stop cleanly;
- force stop removes its managed tree;
- unrelated same-name process survives;
- repeated stop/restart calls are race-safe.

### T04 — Session runtime, state machine, and event model

**Priority:** P0  
**Blocked by:** T01, T02, T03  
**Blocks:** T05, T07, T08, T09

Deliver:

- Session Core;
- lifecycle state machine;
- run id creation;
- command orchestration;
- typed frontend events;
- summary state.

Acceptance:

- invalid transitions rejected;
- unexpected exit updates state;
- restart waits for old process exit;
- main-window visibility does not affect runtime state.

### T05 — Terminal buffer and selective logging core

**Priority:** P1  
**Blocked by:** T01, T04  
**Blocks:** T10

Deliver:

- bounded terminal buffer;
- log modes off/always/manual/on_error/auto;
- sources none/captured/external;
- run metadata;
- deterministic log paths;
- on_error pre-failure buffer;
- retention primitives.

Acceptance:

- default interactive terminal creates no file;
- stdin is not persisted;
- external log is not duplicated;
- run-specific captured log can be located from metadata.

### T06 — V1 UI shell

**Priority:** P1  
**Blocked by:** T00  
**Soft blocked by:** runtime integration  
**Blocks:** T07, T08, T09, T10

Normative references:

- docs/UI_STYLE_GUIDE.md
- assets/ui/ui-v1-preview.png

Deliver:

- left compact session list;
- status dots;
- selected-session header;
- Open / Restart / Stop / More actions;
- Terminal / Logs tabs;
- terminal host area;
- minimal status bar;
- responsive desktop sizing.

Acceptance:

- no permanent right-side dashboard;
- no decorative per-session icons;
- no duplicate Logs navigation;
- terminal remains dominant;
- fixture data may be used before runtime integration.

### T07 — Interactive terminal integration

**Priority:** P1 / release blocker  
**Blocked by:** T02, T04, T06  
**Blocks:** T11

Deliver:

- xterm.js integration;
- PTY output batching;
- terminal input;
- resize propagation;
- multi-session switching;
- reconnect view to already-running session after hide/show.

Acceptance:

- PowerShell passes MVP terminal test matrix;
- Ctrl+C works;
- switching does not destroy PTY;
- large output does not freeze UI.

### T08 — Service session actions, URL/port status, and health hooks

**Priority:** P1  
**Blocked by:** T01, T03, T04, T06  
**Blocks:** T11

Deliver:

- Start/Stop/Restart;
- Open URL;
- Open working directory;
- configured port display;
- basic process/port health signal;
- clear error reporting.

Acceptance:

- button state derives from Session Core;
- restart never duplicates a process;
- port/URL are not treated as lifecycle truth;
- actions affect only the selected managed session.

### T09 — Tray lifecycle and quick controls

**Priority:** P1  
**Blocked by:** T04, T06  
**Blocks:** T11

Deliver:

- X hides main window;
- tray icon;
- summary;
- running-session list;
- Show Window;
- Restart Failed;
- Stop All;
- Exit confirmation.

Acceptance:

- sessions continue while hidden;
- summary updates with runtime;
- Exit never silently kills active sessions;
- taskbar/tray icon matches V1.

### T10 — Logs view, run history, and retention UI

**Priority:** P1  
**Blocked by:** T05, T06  
**Blocks:** T11

Deliver:

- Logs tab;
- current source/mode;
- run history;
- open file/open folder/copy path;
- cleanup action;
- clear distinction between buffer/captured/external logs.

Acceptance:

- user can answer "is this being logged?";
- user can locate a persisted run;
- logging-off terminal does not create a fake empty log entry;
- no redundant global Logs section is introduced.

### T11 — End-to-end MVP verification and regression suite

**Priority:** P1 / release gate  
**Blocked by:** T07, T08, T09, T10  
**Blocks:** T12

Deliver:

- automated tests where practical;
- repeatable manual checklist;
- fixture/demo sessions;
- multi-session stress smoke test;
- fixes needed to satisfy the spec.

Acceptance:

- full MVP_IMPLEMENTATION_SPEC.md matrix passes;
- no P0 safety or PTY blocker remains;
- logging defaults verified;
- UI compared with V1 reference.

### T12 — Windows packaging and v0.1.0 release candidate

**Priority:** P1  
**Blocked by:** T11  
**Blocks:** public v0.1.0

Deliver:

- Windows installer/bundle;
- icon resources;
- clean install/uninstall verification;
- app-data path verification;
- version metadata;
- release notes;
- known limitations.

Acceptance:

- fresh install launches;
- user data/logs are outside install directory;
- upgrade does not silently erase config/log history;
- release artifact passes MVP smoke test.

## 5. Recommended parallel lanes

After T00:

### Lane A — Runtime

T01 -> T04 -> T05

### Lane B — Terminal/process

T02 + T03 -> T04 -> T07/T08

### Lane C — UI

T06 -> T07/T08/T09/T10

Lane C must not invent backend state models. It should use fixtures shaped exactly like the spec until runtime contracts land.

## 6. Merge order recommendation

~~~text
T00
T01 / T02 / T03 / T06  (parallel)
T04
T05 / T07 / T08 / T09 (parallel where practical)
T10
T11
T12
~~~

T06 may merge early as a fixture-driven shell if it contains no fake runtime logic.

## 7. Definition of Ready for an agent ticket

Before assigning a ticket to another AI, verify:

- hard blockers are complete;
- referenced spec files are available;
- expected public interfaces are named;
- acceptance criteria are testable;
- out-of-scope behavior is explicit;
- no unresolved design question is hidden inside the task.

If one item is false, resolve the blocker before asking the agent to guess.
