/**
 * Whether a Local Console Hub host is answering — transport liveness, and
 * nothing else.
 *
 * This module owns exactly one question: is the typed ping being answered? It
 * is **not** a session runtime model, and it must not become one.
 * `src/state/README.md` forbids the frontend from owning process truth: a
 * session's state arrives as `SessionRuntimeDto` from Session Core, and a host
 * that stops answering says nothing about any session. A failed ping means "no
 * host is answering"; it never means "your services stopped".
 *
 * ## Why it is a module and not a hook
 *
 * The decision — pending, connected, unavailable, and what to do about
 * `unavailable` — used to be written inside the React hook, where nothing
 * could test it: the T07 acceptance criterion "the UI does not substitute a
 * read-only/fake terminal path" was violated by a single lost request, and no
 * test in the repo could notice (issue #29). The ping is injected here the
 * same way the terminal attachment protocol injects a backend
 * (`terminal-attach.ts`), so the decision runs in the node test environment
 * with no DOM and no Tauri host. The hook is an adapter that supplies the real
 * `invoke("ping")` and this module's state.
 *
 * ## `unavailable` is not terminal
 *
 * A host can be late (a slow dev build, a cold WebView, a scanner holding the
 * file) and still arrive. So while nothing answers, the module keeps asking on
 * a bounded backoff, and it stops asking the moment something does. A browser
 * preview, where no host exists at all, therefore settles into a slow, quiet
 * poll instead of a hot loop — and a host that comes up later is picked up
 * without the user restarting the app.
 */

import { isPingResponse } from "../types/ipc";
import { retryDelayMs } from "./retry-schedule";

export { RETRY_DELAYS_MS, retryDelayMs } from "./retry-schedule";

/** What the status bar can say about the backend connection. */
export type BackendConnection =
  { state: "pending" } | { state: "connected"; version: string } | { state: "unavailable" };

/** Before anything has asked. */
export const INITIAL_CONNECTION: BackendConnection = { state: "pending" };

/**
 * The typed ping, injected.
 *
 * Its return value is checked against the IPC contract by the caller that
 * knows the contract (`isPingResponse`); this module only cares whether the
 * promise settles successfully, so it can be driven in a test by any function
 * with that shape.
 */
export type BackendPing = () => Promise<unknown>;

/**
 * Whether two readings say the same thing, so a report can be skipped.
 *
 * `pending` and `unavailable` carry nothing but their name, so for those the
 * state is the whole reading. `connected` also carries the version, and two
 * answers from different builds are a change worth telling the shell about —
 * which is why this is not merely `a.state === b.state`.
 */
function same(a: BackendConnection, b: BackendConnection): boolean {
  if (a.state !== "connected" || b.state !== "connected") {
    return a.state === b.state;
  }
  return a.version === b.version;
}

/**
 * Watch for a host, reporting every connection state it enters.
 *
 * Calls `ping` immediately, and — while it settles unsuccessfully — keeps
 * calling it on [`retryDelayMs`]'s backoff. The first successful answer
 * reports `connected` and asks nothing further: a host that is answering does
 * not need to be re-checked, and a healthy app pays for exactly one request.
 *
 * `report` is called only when the reading changes, so a preview that stays
 * unavailable does not re-render the shell every few seconds.
 *
 * Returns the stop function: it cancels the pending attempt and makes every
 * later answer a no-op, so a host that answers after the shell has gone away
 * cannot report into it. This is what a `useEffect` cleanup returns.
 */
export function watchBackendConnection(
  ping: BackendPing,
  report: (connection: BackendConnection) => void,
): () => void {
  let stopped = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let reported: BackendConnection = INITIAL_CONNECTION;
  let failedAttempts = 0;

  const publish = (connection: BackendConnection) => {
    if (same(reported, connection)) return;
    reported = connection;
    report(connection);
  };

  const ask = () => {
    if (stopped) return;
    void ping().then((value) => {
      if (stopped) return;
      if (isPingResponse(value)) {
        publish({ state: "connected", version: value.appVersion });
        return;
      }
      // A settled answer that is not this app's handshake is not this app's
      // host. That is the reading the void-returning `ping` had before this
      // module existed, and the retry keeps it from being a permanent
      // verdict: a host that comes back with the right contract is picked
      // up on the next attempt.
      unavailable();
    }, unavailable);
  };

  const unavailable = () => {
    if (stopped) return;
    failedAttempts += 1;
    publish({ state: "unavailable" });
    timer = setTimeout(ask, retryDelayMs(failedAttempts - 1));
  };

  ask();
  return () => {
    stopped = true;
    if (timer !== undefined) clearTimeout(timer);
  };
}
