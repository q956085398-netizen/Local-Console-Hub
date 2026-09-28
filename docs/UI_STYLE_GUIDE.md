# Local Console Hub UI 规范（V2）

> V2 replaces the previous V1 preview as the normative UI direction.
> Implementation should preserve the product semantics in this document rather than copy preview-only mock behavior.

## 1. Normative references

- `assets/ui/ui-v2-service.png` — service-session state and overall desktop hierarchy.
- `assets/ui/ui-v2-terminal.png` — interactive terminal state.
- `docs/MVP_IMPLEMENTATION_SPEC.md` — runtime and safety contracts.
- `docs/LOGGING.md` — logging semantics.

## 2. Design goals

V2 is a compact Windows-first control surface for local services and terminals. The UI should make four questions answerable at a glance:

1. What sessions are managed?
2. What is the selected session doing now?
3. What happens if I stop it?
4. Where are its terminal and persisted logs?

The terminal remains the dominant work area. Status, metadata and safety context are visible without turning the app into a monitoring dashboard.

## 3. Main window hierarchy

The desktop window uses four persistent layers:

- compact title bar with app identity, global run summary and window controls;
- left managed-session sidebar;
- selected-session workspace;
- minimal bottom status strip.

Do not add a permanent right-side dashboard.

## 4. Managed-session sidebar

The sidebar is grouped by workload or purpose and contains:

- group heading;
- status dot;
- session name;
- compact secondary metadata such as port/type/logging mode;
- optional runtime duration or stopped state;
- search by name, port or purpose;
- one clear add-session entry.

Session rows use status dots, not decorative app logos. The selected row uses a restrained highlight.

## 5. Selected-session header

The workspace header contains:

- session name;
- session type;
- lifecycle/readiness state;
- one-line purpose;
- Start/Stop, Restart and context-appropriate Open/Directory actions;
- overflow menu for low-frequency and destructive actions.

For a running session, show a concise **close impact** callout. It must explain the practical consequence of stopping this session, not just repeat its state.

Below the callout, expose a compact metadata line when values exist: PID, port, uptime, cwd and effective logging mode/source.

## 6. Workspace tabs

The normative tabs are:

- **Terminal** — primary interactive surface;
- **Logs** — persistence policy and run history;
- **Details** — identity, close impact, dependencies and technical metadata.

Details is allowed because it moves low-frequency information out of the terminal without creating a permanent side panel. It carries the *low-frequency* fields only (identity, close impact, dependencies, exit/PTY/buffer detail): values already live in the header metadata line — PID, port, uptime, cwd, effective log policy — are not repeated here (§13 forbids excessive duplication).

## 7. Terminal

The terminal must be a real PTY view, not a read-only log box.

- interactive sessions use ConPTY/PTY input and resize semantics;
- service sessions may expose attached stdin when supported;
- connection state is visible but visually quiet;
- terminal scrollback is bounded;
- keyboard semantics such as Ctrl+C are preserved;
- preview-only fake command execution must not ship.

The UI may state when stdin is not persisted so users can distinguish terminal interaction from logging.

## 8. Logs

The Logs tab shows the effective logging policy before showing history.

It must distinguish:

- memory-only terminal buffer;
- captured Hub-owned log;
- external application-owned log.

Run history must not fabricate empty disk-log records when logging is off. Actions may include open log, open containing folder, copy path and retention cleanup, subject to the logging spec.

## 9. Tray behavior

Closing or minimizing the main window may hide it to the system tray while managed sessions keep running.

The tray surface is intentionally compact:

- overall running/busy summary;
- running or failed sessions;
- Show Window;
- Restart Failed;
- Stop All;
- Exit.

The tray is not a miniature copy of the full app.

## 10. Visual language

- dark near-black surfaces with restrained contrast;
- thin borders and subtle elevation;
- interaction and focus use the neutral light-gray (`#c5ccd6`), never a color — **color is reserved for lifecycle truth** (T06 measured this from the approved V2 source; recorded in DECISIONS.md D-017);
- green = healthy/running, amber = busy/warning, red = error/destructive, gray = stopped/inactive;
- monospace text for terminal and compact technical metadata;
- concise Chinese/English labels are acceptable, but implementation should be internally consistent;
- avoid decorative gradients, oversized cards, charts and large iconography.

## 11. Responsive behavior

Desktop is the release target. At narrower widths the session list may move into a drawer, but the same information hierarchy must be preserved. Terminal space has priority over secondary metadata.

## 12. Implementation boundary

The Grok design workspace is a prototype/reference source, not the application architecture. Do not copy its auth, database, deployment, preview simulation or fixture-runtime subsystems into the product.

Reuse the V2 information hierarchy and interaction decisions while implementing them against the stable DTOs and runtime contracts defined by the MVP spec.

## 13. Release acceptance

Before the MVP release gate passes:

- service and terminal states must visually match the V2 hierarchy;
- close-impact information must be present for running managed sessions;
- Terminal / Logs / Details must not duplicate the same information excessively;
- no old V1 preview dependency may remain;
- tray and main-window behavior must preserve live sessions;
- terminal interaction and logging semantics must be real, not preview simulations.
