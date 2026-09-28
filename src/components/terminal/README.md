# components/terminal/

The dominant terminal host (T06 #7 built the shell; T07 #8 wired the live
surface, `MVP_IMPLEMENTATION_SPEC.md` §6, `UI_STYLE_GUIDE.md` §7).

xterm.js renders the terminal; it does not own the process or PTY. This
component owns neither either: it renders one session's stream, forwards
keystrokes and reports its own size. The session — and the shell on the other
end of it — lives in the backend, which is why selecting another session,
switching tabs or hiding the window cannot disturb a running terminal.

With a backend, the body is an xterm surface fed by
`src/app/useTerminalStream.ts` over `src/state/terminal-attach.ts`:

- attaching replays the retained scrollback and then renders live batches
  (the protocol is in that module; the rule that decides what a batch means is
  in `src/state/terminal-stream.ts`);
- typing goes back through `terminal_write`, Ctrl+C included — the byte
  `0x03` travels as input, which interrupts the running command and does **not**
  close the session (spec §7);
- the view's size is forwarded once it has settled, because a keystroke racing
  a ConPTY resize can be dropped (the quirk noted on #3).

Without a backend (the browser preview) the body is the fixture preview
stream, read-only: the caret is presentation of fixture state and there is no
input path. No fake command execution ships in either mode. A stopped session
shows the start overlay, not a log box — and with a backend, the last run's
scrollback stays visible underneath it.
