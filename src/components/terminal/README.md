# components/terminal/

Reserved for the xterm.js terminal host (T07 #8, docs/MVP_IMPLEMENTATION_SPEC.md §6).

xterm.js renders the terminal; it does not own the process or PTY. No
read-only/fake terminal path may be introduced here.
