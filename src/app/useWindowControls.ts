import { useCallback, useEffect, useMemo, useState } from "react";
import { runWindowControl, trackMaximized, type WindowControl } from "./window-controls";
import { tauriWindowHost } from "./tauriWindowHost";

/**
 * The title bar's window controls, wired to the real window.
 *
 * An adapter, and only that: `window-controls.ts` decides what a control means
 * and how the maximize reading is kept current, `tauriWindowHost.ts` supplies
 * the window, and this hook hands both to a component. Everything the rules
 * depend on is on one side or the other of it.
 */
export interface WindowControlsState {
  /** Whether there is an OS window behind this title bar. */
  available: boolean;
  /** What the window reports about itself — never a local toggle. */
  maximized: boolean;
  run(control: WindowControl): void;
}

/**
 * `onFailure` receives the message of an operation the OS refused; the title
 * bar has nowhere of its own to put one, so it goes to the shell's notice.
 */
export function useWindowControls(onFailure?: (message: string) => void): WindowControlsState {
  // Asked once per mount: the host is the window this page is running in, and
  // that does not change while the app is up.
  const host = useMemo(() => tauriWindowHost(), []);
  const [maximized, setMaximized] = useState(false);

  useEffect(() => (host === null ? undefined : trackMaximized(host, setMaximized)), [host]);

  const run = useCallback(
    (control: WindowControl) => {
      if (host === null) return;
      void runWindowControl(host, control).then((outcome) => {
        if (!outcome.ok) onFailure?.(outcome.message);
      });
    },
    [host, onFailure],
  );

  return { available: host !== null, maximized, run };
}
