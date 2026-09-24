# MVP Implementation Spec

> **Status:** implementation baseline  
> **Target:** Local Console Hub v0.1.0 / Windows-first MVP  
> **Related:** [PRODUCT_SPEC.md](./PRODUCT_SPEC.md), [LOGGING.md](./LOGGING.md), [DEVELOPMENT.md](./DEVELOPMENT.md), [DECISIONS.md](./DECISIONS.md), [UI_STYLE_GUIDE.md](./UI_STYLE_GUIDE.md)

This document is the implementation contract for contributors and coding agents. It freezes the important boundaries, observable behavior, interfaces and acceptance criteria so work can be split without incompatible implementations.

## 1. MVP outcome

The MVP is complete when a Windows user can:

1. configure local services and interactive shells;
2. start, stop, restart and switch between them from one desktop window;
3. interact with a real terminal session, including stdin and Ctrl+C;
4. hide the main window to the tray without stopping managed sessions;
5. see service name, state and optional port/URL without dashboard clutter;
6. use selective logging without every terminal automatically producing files;
7. locate any persisted log by session and run;
8. stop one managed session without killing unrelated processes.

The approved V1 UI references are:

- assets/ui/ui-v1-preview.png
- docs/UI_STYLE_GUIDE.md

## 2. Technical baseline

Unless an explicit design decision changes this spec:

- Desktop shell: **Tauri 2**
- Frontend: **React + TypeScript + Vite**
- Terminal renderer: **xterm.js**
- Backend: **Rust**
- Async runtime: Tokio where needed
- Serialization: Serde
- Configuration: human-readable YAML
- PTY: backend-owned abstraction; Windows implementation may begin with portable-pty, but must satisfy ConPTY behavior in the acceptance tests

If portable-pty cannot satisfy the Windows requirements, replace only the PTY backend with direct Windows ConPTY integration. Do not change the frontend/session contracts to work around it.

Windows data locations should follow OS app-data conventions:

~~~text
%APPDATA%\LocalConsoleHub\config.yaml
%LOCALAPPDATA%\LocalConsoleHub\logs\...
%LOCALAPPDATA%\LocalConsoleHub\metadata\...
%LOCALAPPDATA%\LocalConsoleHub\cache\...
~~~

Runtime logs must not be written beside the executable by default.

## 3. Module boundaries

The exact filenames may vary, but responsibility boundaries must remain equivalent.

~~~text
src/
├─ app/
├─ components/
│  ├─ sidebar/
│  ├─ session-header/
│  └─ terminal/
├─ state/                  # frontend view state only
└─ types/

src-tauri/src/
├─ config/                 # schema, validation, defaults
├─ session/                # Session Core + state machine
├─ pty/                    # PTY abstraction and Windows backend
├─ process/                # process supervision / stop / kill tree
├─ logging/                # buffer, captured logs, external logs
├─ health/                 # port/HTTP checks
├─ tray/                   # native tray lifecycle
├─ ipc/                    # Tauri commands/events/DTO mapping
└─ app/
~~~

Boundary rules:

- UI does not own process truth.
- PTY code does not decide logging policy.
- Logging code does not decide session lifecycle.
- Tray actions call the same Session Core APIs as the main window.
- Session Core is the single source of lifecycle truth.

## 4. Core data contracts

### SessionConfig

Conceptual example:

~~~yaml
id: sillytavern
name: SillyTavern
type: service
cwd: D:/Tools/SillyTavern
command: node server.js
url: http://127.0.0.1:8000
port: 8000
purpose: 聊天前端
close_impact: 可停止；网页会失联
logging:
  mode: auto
  source: captured
~~~

Required fields:

- id: stable, unique, filesystem-safe identifier
- name
- type: service or terminal

Optional service fields:

- cwd
- command
- url
- port
- purpose
- close_impact

Optional terminal fields:

- shell
- cwd
- initial command

Logging fields follow LOGGING.md.

### SessionStatus

MVP lifecycle states:

~~~text
Stopped
Starting
Running
Stopping
Exited
Error
~~~

Optional runtime flags may supplement lifecycle state:

~~~text
busy: bool | unknown
ready: bool | unknown
~~~

Busy/ready must not replace lifecycle state.

### SessionRuntime

At minimum:

- session id
- lifecycle status
- PID when applicable
- run id
- start time
- exit code when known
- PTY attachment state
- effective logging mode
- terminal buffer reference
- last structured error

### RunRecord

Each managed start creates one run record containing:

- run id
- session id
- started/ended timestamps
- exit code
- PID
- logging source/mode
- persisted log path when one exists

## 5. State machine

Normal transitions:

~~~text
Stopped -> Starting -> Running
Running -> Stopping -> Exited/Stopped
Running -> Exited
Starting -> Error
Running -> Error
Stopping -> Error
Exited -> Starting
Error -> Starting
~~~

Rules:

1. Start while Starting/Running is rejected or a clear no-op.
2. Restart cannot launch a replacement until the previous managed process is confirmed stopped.
3. State changes originate in Session Core, not UI assumptions.
4. Unexpected exit updates state while the UI is hidden.
5. Exit code and error context remain available in the run record.

## 6. PTY contract

Interactive terminal support is a **release blocker**.

The PTY layer must support:

- spawn shell/command in configured working directory;
- bidirectional byte stream;
- resize;
- Unicode input/output;
- ANSI output;
- Ctrl+C behavior expected by common Windows CLI applications;
- terminal session remaining alive while hidden or while another session is selected.

xterm.js renders the terminal; it does not own the process or PTY.

Conceptual backend operations:

~~~text
spawn(session_id, cols, rows)
write(session_id, bytes)
resize(session_id, cols, rows)
close(session_id)
subscribe_output(session_id)
~~~

Output transport must be bounded/backpressured so a noisy process cannot grow memory without limit.

## 7. Process supervision contract

The process layer must:

- know exactly which process belongs to a managed session;
- track enough descendant/process-tree information for safe shutdown;
- distinguish graceful stop from force kill;
- never kill unrelated processes based only on executable name.

Default stop path:

~~~text
request graceful stop
        ↓
wait configured timeout
        ↓
confirm process exit
        ↓
if still alive -> explicit force-kill path
~~~

For terminal sessions, Ctrl+C is not the same action as closing the session.

## 8. Logging contract

LOGGING.md is normative.

Keep separate:

1. **Terminal Buffer** — bounded in-memory scrollback
2. **Hub Captured Log** — optional persistent stdout/stderr
3. **External Log** — application-owned file referenced by Hub

Defaults:

- interactive terminal: no persistent log
- stdin: never persisted by default
- application with its own log: reference it rather than duplicate it
- service without external logs: auto may resolve to on_error
- UI must reveal whether persistence is active and where it writes

For on_error, retain a bounded pre-error buffer so an abnormal exit can preserve useful context.

## 9. IPC contract

Keep the frontend/backend contract explicit.

MVP command equivalents:

~~~text
list_sessions
get_session
start_session
stop_session
force_stop_session
restart_session
terminal_write
terminal_resize
open_session_url
open_session_cwd
get_run_history
get_log_info
~~~

Do not introduce one generic "execute arbitrary backend action" command.

MVP event equivalents:

~~~text
session-state-changed
terminal-output
run-record-updated
app-summary-changed
~~~

Every session event includes the session id.

Terminal output should be batched enough that high-volume output does not create an expensive UI event per line.

## 10. UI contract

UI_STYLE_GUIDE.md is normative.

The main window must preserve:

- compact left session list;
- status dots instead of decorative per-session icons;
- selected session name + optional port;
- compact Open / Restart / Stop / More actions;
- Terminal / Logs as the primary content switch;
- terminal as the dominant region;
- interactive input behavior;
- low-density status bar.

Do not reintroduce:

- permanent right-side information dashboard;
- redundant global Logs navigation;
- repeated state/port/logging information;
- decorative app icons in the session list.

For stopped services, Start replaces the relevant running action state.

For terminal sessions, PTY-native interaction is required. A fake command textbox cannot replace real terminal input if it breaks full-screen/interactive CLI applications.

## 11. Tray contract

Clicking the main window close button hides the window to tray by default.

Tray behavior must provide the functional equivalent of:

- app title/icon;
- summary such as "3 running (1 busy)";
- compact running-session list;
- Show Window;
- Restart Failed;
- Stop All;
- Exit.

If managed sessions are active, Exit must not silently destroy them.

MVP acceptable behavior:

- confirm "Stop managed sessions and exit" or Cancel.

Detach-and-leave-running is optional and must not be faked before technically safe.

## 12. Health/status checks

MVP needs a small abstraction only:

- process alive;
- TCP port open;
- optional HTTP GET.

Checks must be low-frequency, cancellable with session lifecycle, non-blocking to UI, and separate from lifecycle truth.

## 13. Configuration validation

Validate before launch.

Reject with actionable messages:

- duplicate session id;
- unsupported type;
- missing command/shell;
- invalid required working directory;
- invalid port;
- malformed URL;
- incompatible logging settings.

One bad session must not hide unrelated valid sessions. Report per-session config errors.

## 14. Performance requirements

- hidden-to-tray idle CPU should remain effectively near idle;
- no high-frequency global process scanning;
- terminal scrollback must be bounded;
- log writes should be buffered;
- invisible terminals should not continuously re-render;
- one high-output session must not freeze the whole UI.

## 15. Security/privacy requirements

Must not:

- record stdin by default;
- dump full environment variables into logs/metadata;
- upload logs automatically;
- scan/capture every console process;
- kill by executable-name pattern;
- silently delete external application logs.

## 16. MVP test matrix

### Interactive terminal

- PowerShell starts
- commands can be typed
- Unicode works
- Ctrl+C interrupts a long-running command
- resize works
- switching sessions keeps PTY alive
- tray hide/restore keeps PTY alive

### Service

- long-running service starts
- stdout/stderr visible
- configured port/URL visible
- restart waits for previous process to exit
- graceful stop works
- force stop affects only the managed tree

### Logging

- terminal mode off creates no persistent log
- captured always creates a run-specific log
- on_error preserves pre-error context
- external log references the original file without duplicate capture

### Multi-session

- at least three concurrent sessions
- stopping one leaves others alive
- noisy output in one does not make another unusable

### Tray

- X hides window
- sessions continue
- Show Window restores
- summary reflects runtime changes
- Exit never silently destroys running work

## 17. Definition of Done for v0.1.0

v0.1.0 reaches release-candidate status only when:

- all P0/P1 tickets in EXECUTION_PLAN.md are complete;
- hard dependencies are resolved;
- the Windows MVP test matrix passes;
- no known issue can terminate unrelated processes;
- PTY interaction is real, not read-only emulation;
- logging defaults conform to LOGGING.md;
- UI preserves the approved V1 information hierarchy;
- tray behavior is implemented;
- a fresh user can configure a SillyTavern/ComfyUI-like service and a PowerShell terminal without modifying source code.

## 18. Change control

An implementation agent must not silently change:

- tech stack;
- config semantics;
- PTY ownership;
- logging defaults;
- session state model;
- process stop semantics;
- V1 UI information hierarchy.

If a blocker requires such a change:

1. stop dependent work;
2. document the blocker on the relevant ticket;
3. propose the smallest change;
4. update DECISIONS.md and this spec if accepted;
5. then resume dependent work.
