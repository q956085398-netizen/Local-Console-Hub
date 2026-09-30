import { getCurrentWindow } from "@tauri-apps/api/window";
import type { WindowHost } from "./window-controls";

/**
 * The main window as the desktop app reaches it.
 *
 * The only place that knows both sides: `window-controls.ts` decides what a
 * window control means, and `tauri.conf.json` + `capabilities/default.json`
 * declare the window the desktop app actually opens (undecorated, resizable,
 * with the four `core:window:` permissions the controls need). Keeping this
 * adapter this small is the point.
 *
 * ## Why this returns `null` instead of a host
 *
 * The same frontend runs in two places: the desktop app, and a browser preview
 * (`npm run dev` with no Rust host — the fixture workspace the visual capture
 * drives). `getCurrentWindow()` reads `window.__TAURI_INTERNALS__`, so in a
 * browser it throws before it can ask for anything. `null` says "there is no
 * window here to control" in one place, and the title bar renders its controls
 * inert rather than offering buttons that would fail on click.
 *
 * `subscribe` has to return its unlisten synchronously while Tauri's
 * `onResized` resolves asynchronously, so the two orderings are both handled: a
 * view that stops tracking before the listener is registered unsubscribes the
 * moment it arrives (the same shape as `tauriTerminalBackend`).
 */
export function tauriWindowHost(): WindowHost | null {
  if (!("__TAURI_INTERNALS__" in window)) return null;

  const current = getCurrentWindow();

  return {
    minimize: () => current.minimize(),
    toggleMaximize: () => current.toggleMaximize(),
    close: () => current.close(),
    isMaximized: () => current.isMaximized(),

    onResized: (receive) => {
      let unlisten: (() => void) | null = null;
      let stopped = false;
      void current
        .onResized(() => receive())
        .then((registered) => {
          if (stopped) {
            registered();
          } else {
            unlisten = registered;
          }
        });
      return () => {
        stopped = true;
        unlisten?.();
      };
    },
  };
}
