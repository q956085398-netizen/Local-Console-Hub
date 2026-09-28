# components/terminal/

The dominant terminal/output host (T06 #7 rendered the shell; T07 #8 wires
the live surface per MVP_IMPLEMENTATION_SPEC.md §6).

xterm.js renders the terminal; it does not own the process or PTY. Until T07
lands, `TerminalHost` renders the fixture preview lines read-only — the caret
is presentation of fixture state, there is no input path, and no fake command
execution ships. A stopped session shows the start overlay, not a log box.
