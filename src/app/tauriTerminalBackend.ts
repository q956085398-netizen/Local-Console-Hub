import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { TerminalBackend } from "../state/terminal-attach";

/**
 * The terminal backend as the desktop app reaches it.
 *
 * The only place that knows both sides: `openTerminalSession` speaks the
 * protocol (`src/state/terminal-attach.ts`) and `src-tauri/src/ipc/terminal.rs`
 * answers it. Keeping the adapter this small is the point — everything worth
 * deciding about an attachment is on one side or the other of it.
 *
 * `subscribe` has to return its unlisten synchronously while Tauri's `listen`
 * resolves asynchronously, so the two orderings are both handled: a view that
 * detaches before the listener is registered unsubscribes the moment it
 * arrives.
 */
export const tauriTerminalBackend: TerminalBackend = {
  attach: (sessionId) => invoke("attach_terminal", { sessionId }),

  write: async (sessionId, data) => {
    await invoke("terminal_write", { sessionId, data });
  },

  resize: async (sessionId, cols, rows) => {
    await invoke("terminal_resize", { sessionId, cols, rows });
  },

  subscribe: (_sessionId, receive) => {
    let unlisten: (() => void) | null = null;
    let closed = false;
    void listen<unknown>("terminal-output", (event) => {
      receive(event.payload);
    }).then((registered) => {
      if (closed) {
        registered();
      } else {
        unlisten = registered;
      }
    });
    return () => {
      closed = true;
      unlisten?.();
    };
  },
};
