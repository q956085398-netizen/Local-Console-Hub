/**
 * Is a host answering the typed ping? (#29, T06 #7's bootstrap signal)
 *
 * **Transport liveness, not session runtime truth.** This module answers one
 * question about the *connection* to Session Core, and holds nothing about any
 * session: `src/state/README.md` gives the runtime model to Rust, and a
 * frontend that kept one here would be the second copy of it the spec forbids.
 * A lost ping is therefore "no host is answering" — never "a session stopped",
 * never a terminal failure, and no error text leaves this module to be shown as
 * either.
 *
 * ## Why the question is asked more than once
 *
 * Asking once makes a lost request permanent. The first `invoke("ping")` can
 * lose to a slow dev build, a cold WebView, or a scanner holding the file, and
 * a window that never asks again stays on the fixture workspace and the
 * read-only terminal body for good — the read-only/fake terminal path T07's
 * acceptance criterion forbids, and nothing in the window could notice. So
 * `unavailable` is not terminal, and neither is a question that is never
 * answered: while no host is answering, the module asks again and stops the
 * moment an answer lands.
 *
 * ## The cadence
 *
 * `RETRY_BACKOFF_MS` is the whole schedule, and its last step is the bound on
 * it: the wait grows with the failures in a row and then holds there. One
 * question is awaited at a time, for at most `QUESTION_TIMEOUT_MS` — a refusal,
 * a reply that is not the typed ping, and a question nobody answers all say the
 * same thing, and each schedules the next one on the ramp. A genuine browser
 * preview, where `invoke` has no host to call, therefore settles into one quiet
 * question per step instead of a hot loop.
 *
 * ## Where it is used
 *
 * The shell renders the preview workspace whenever the answer is not
 * `connected`, and takes over the live workspace the moment a late host starts
 * answering — through the path that already runs when the first listing
 * returns, which is `useSessionRegistry`'s effect on this state.
 */

import { isPingResponse } from "../types/ipc";

/**
 * The typed ping (`ping` in `src-tauri/src/ipc/mod.rs`), as this module sees
 * it. Injected rather than reached for — the terminal attachment protocol takes
 * its backend the same way — so the decision can be driven with no Tauri host,
 * which is also why no component calls the Tauri API for the ping.
 *
 * A rejection is an answer like any other: "no host is answering".
 */
export type BackendPing = () => Promise<unknown>;

/** What the shell can say about the backend connection, and all it can say.
 *
 * `pending` is the answer before the first reply lands — including while a
 * question is still being awaited; `unavailable` is "no host is answering",
 * which is also what a reply that is not the typed ping means. The preview
 * workspace renders for both, and the version a live host reports is the only
 * thing carried through. */
export type BackendConnection =
  { state: "pending" } | { state: "connected"; version: string } | { state: "unavailable" };

/**
 * How long the module waits before asking again, in order, after failures.
 *
 * The last value repeats for every failure past it, so this list is both the
 * ramp and the bound: early steps are short enough that a backend which is
 * still starting is noticed within a second or two, and the ceiling is long
 * enough that a browser preview, where no answer will ever come, stays quiet.
 */
export const RETRY_BACKOFF_MS = [500, 1_000, 2_000, 5_000, 10_000, 15_000] as const;

/**
 * How long one question may go unanswered before it counts as no answer.
 *
 * A pending request is not a promise that something will eventually reply: a
 * WebView that has not finished starting, or an app binary a scanner is
 * holding, can leave `invoke("ping")` with nothing to say for as long as the
 * app runs. Waiting on it forever strands the window exactly as asking once
 * did, so the question is given up on and the schedule carries on.
 *
 * The wait is far longer than any local IPC round trip, so it does not cut off
 * a host that is merely slow: an answer that arrives after it, saying a host is
 * up, still connects (see `ask`).
 */
export const QUESTION_TIMEOUT_MS = 5_000;

/**
 * Whether two decisions are the same answer. A retry that fails again decides
 * nothing the caller has not already been told, and saying it twice would
 * re-render the shell for a preview that was never going to change.
 */
function sameDecision(a: BackendConnection, b: BackendConnection): boolean {
  if (a.state === "connected" && b.state === "connected") {
    return a.version === b.version;
  }
  return a.state === b.state;
}

/**
 * Ask whether a host is answering, and keep asking until one does.
 *
 * The first question goes out immediately, because that is the app starting up
 * and the answer decides what the window shows. `onChange` is called with every
 * new decision — never with the one the caller already has, and never with a
 * `pending` it started from.
 *
 * Returns the way to stop: after it, nothing more is asked, the outstanding
 * question and any scheduled retry are dropped, and an answer that arrives late
 * is not reported. That is what the caller's effect cleanup is.
 */
export function watchBackendConnection(
  ping: BackendPing,
  onChange: (connection: BackendConnection) => void,
): () => void {
  let connection: BackendConnection = { state: "pending" };
  let failures = 0;
  let retry: ReturnType<typeof setTimeout> | null = null;
  let unanswered: ReturnType<typeof setTimeout> | null = null;
  let stopped = false;

  const report = (next: BackendConnection) => {
    if (sameDecision(connection, next)) return;
    connection = next;
    onChange(next);
  };

  /** Drop both clocks: the scheduled retry and the wait on the live question. */
  const stopTimers = () => {
    if (retry !== null) {
      clearTimeout(retry);
      retry = null;
    }
    if (unanswered !== null) {
      clearTimeout(unanswered);
      unanswered = null;
    }
  };

  /** The answer was "no host": say so, and ask again on the next step. */
  const retryAfterFailure = () => {
    report({ state: "unavailable" });
    // Scheduled after the answer, never alongside the question that produced
    // it: questions that ran while an earlier one was unanswered are how a
    // retry becomes a loop.
    const delay = RETRY_BACKOFF_MS[Math.min(failures, RETRY_BACKOFF_MS.length - 1)];
    failures += 1;
    retry = setTimeout(() => {
      retry = null;
      ask();
    }, delay);
  };

  /** Ask, and turn whatever comes back into a decision. */
  function ask(): void {
    /** Set once the question has been given up on. Its answer is still read. */
    let abandoned = false;
    unanswered = setTimeout(() => {
      unanswered = null;
      abandoned = true;
      retryAfterFailure();
    }, QUESTION_TIMEOUT_MS);

    /** Whether this answer belongs to a watch that is still running. */
    const answered = () => {
      if (unanswered !== null) {
        clearTimeout(unanswered);
        unanswered = null;
      }
      return !stopped;
    };

    ping().then(
      (reply) => {
        if (!answered()) return;
        if (isPingResponse(reply)) {
          // A host that answers is a host answering, however late it is: this
          // stops the asking, including a retry a timeout already scheduled.
          stopTimers();
          report({ state: "connected", version: reply.appVersion });
        } else if (!abandoned) {
          // A reply that is not the typed ping is a host that did not answer
          // this question — the same fact as a rejection, not a separate state.
          retryAfterFailure();
        }
      },
      () => {
        if (answered() && !abandoned) {
          retryAfterFailure();
        }
      },
    );
  }

  ask();

  return () => {
    stopped = true;
    stopTimers();
  };
}
