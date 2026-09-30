import { describe, expect, it } from "vitest";
import {
  WINDOW_CONTROL_LABELS,
  runWindowControl,
  trackMaximized,
  type WindowControl,
  type WindowHost,
} from "./window-controls";

/**
 * The merged title bar owns Windows' window operations (issue #68). What is
 * worth testing about that is not the DOM — the repo's frontend tests run in
 * the node environment with no DOM at all (#25) — but the two rules the buttons
 * encode: every control reaches the one operation it names, and the maximize
 * glyph is a *reading* of the window rather than a local toggle. Both run here
 * against a fake host, the same way `backend-connection.test.ts` drives the
 * ping.
 */

/** A host that records what it was asked, and answers what it was told to. */
function fakeHost(start: { maximized: boolean }) {
  const state = {
    maximized: start.maximized,
    /** Operations the OS refuses, and whether the size reading is refused. */
    refuses: new Set<WindowControl>(),
    refusesRead: false,
  };
  const asked: WindowControl[] = [];
  const resizes: (() => void)[] = [];
  let unlistened = 0;

  const host: WindowHost = {
    minimize: async () => {
      asked.push("minimize");
      if (state.refuses.has("minimize")) throw new Error("access denied");
    },
    toggleMaximize: async () => {
      asked.push("toggle-maximize");
      if (state.refuses.has("toggle-maximize")) throw new Error("refused");
      state.maximized = !state.maximized;
    },
    close: async () => {
      asked.push("close");
      if (state.refuses.has("close")) throw new Error("the window is gone");
    },
    isMaximized: async () => {
      if (state.refusesRead) throw new Error("no answer");
      return state.maximized;
    },
    onResized: (receive) => {
      resizes.push(receive);
      return () => {
        unlistened += 1;
      };
    },
  };

  return {
    host,
    state,
    asked,
    unlistened: () => unlistened,
    /** The window changed size — what Tauri reports as `tauri://resize`. */
    resize: () => resizes.forEach((receive) => receive()),
  };
}

/** Let every already-settled promise in the microtask queue run. */
const settled = () => new Promise((done) => setTimeout(done, 0));

describe("runWindowControl", () => {
  it("asks the host for the operation the control names", async () => {
    for (const control of ["minimize", "toggle-maximize", "close"] as const) {
      const fake = fakeHost({ maximized: false });

      expect(await runWindowControl(fake.host, control)).toEqual({ ok: true });
      expect(fake.asked).toEqual([control]);
    }
  });

  /** A window operation that did not land has to say so: a click that silently
   * does nothing is the one thing a window control must never be. */
  it("reports a refused operation instead of throwing, naming what failed", async () => {
    const fake = fakeHost({ maximized: false });
    fake.state.refuses.add("minimize");

    const outcome = await runWindowControl(fake.host, "minimize");

    expect(outcome.ok).toBe(false);
    if (outcome.ok) throw new Error("unreachable");
    expect(outcome.message).toContain(WINDOW_CONTROL_LABELS.minimize);
    expect(outcome.message).toContain("access denied");
  });
});

describe("trackMaximized", () => {
  /** The button is a reading, not a toggle: it shows what the window reports. */
  it("reports the window's own reading, then every change of it", async () => {
    const fake = fakeHost({ maximized: false });
    const readings: boolean[] = [];

    const stop = trackMaximized(fake.host, (maximized) => readings.push(maximized));
    await settled();
    expect(readings).toEqual([false]);

    await runWindowControl(fake.host, "toggle-maximize");
    fake.resize();
    await settled();
    expect(readings).toEqual([false, true]);

    stop();
    expect(fake.unlistened()).toBe(1);
  });

  /** An unreadable window is not an unmaximized one: a failed read leaves the
   * last reading standing rather than flipping the glyph on a guess. */
  it("keeps the last reading when a read fails", async () => {
    const fake = fakeHost({ maximized: true });
    const readings: boolean[] = [];

    trackMaximized(fake.host, (maximized) => readings.push(maximized));
    await settled();
    fake.state.refusesRead = true;
    fake.resize();
    await settled();

    expect(readings).toEqual([true]);
  });

  /** A reading that arrives after the title bar is gone must not report into
   * it — the same rule the backend ping and the tray listener follow. */
  it("stops reporting once it is stopped, including late answers", async () => {
    const fake = fakeHost({ maximized: false });
    const readings: boolean[] = [];

    const stop = trackMaximized(fake.host, (maximized) => readings.push(maximized));
    stop();
    await settled();

    expect(readings).toEqual([]);
  });
});
