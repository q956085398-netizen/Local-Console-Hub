import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  QUESTION_TIMEOUT_MS,
  RETRY_BACKOFF_MS,
  watchBackendConnection,
  type BackendConnection,
} from "./backend-connection";
import type { PingResponse } from "../types/ipc";

/**
 * The connection decision the shell renders from (#29), driven through an
 * injected ping and fake timers.
 *
 * These are the cases the ask-once hook could not have: a first request that is
 * lost, the host that comes up a moment later, the hostless browser preview
 * where no answer will ever come, the question that is never answered at all,
 * and the answer that arrives after the view that asked it went away. Nothing
 * here touches a DOM or a Tauri host — the ping is a test double, so what is
 * under test is only what the module decides from the answers it gets.
 */

const PING_OK: PingResponse = {
  appName: "Local Console Hub",
  appVersion: "0.1.0",
  protocol: 1,
};

const NO_HOST_MESSAGE = "no host is answering";
const ONE_HOUR_MS = 60 * 60_000;
const TEN_MINUTES_MS = 10 * 60_000;

/** The schedule's ceiling, as the module reads it: the last step, repeating. */
const CEILING_MS = RETRY_BACKOFF_MS[RETRY_BACKOFF_MS.length - 1];

/** How the host answers a question. */
type Answer = () => Promise<unknown>;

const ANSWERS: Record<string, Answer> = {
  /** A host that is up: the typed ping, answered. */
  host: () => Promise.resolve(PING_OK),
  /** No host at all — what `invoke` does in a browser preview. */
  noHost: () => Promise.reject(new Error(NO_HOST_MESSAGE)),
  /** A reply that is not the typed ping: not a host answering this question. */
  notAPing: () => Promise.resolve({ greeting: "hello" }),
};

/**
 * The injected ping, driven by the test.
 *
 * A question is answered by whatever `answers` last set, so "the host came up
 * between two questions" is the same watch in one test. With no answer set the
 * question is *parked* — taken and not yet replied to — which is how a probe in
 * flight, and a reply that arrives after the watch was stopped, both become
 * states the test can hold the module in.
 *
 * `askedAt` is the mocked clock at each question: the cadence is measured from
 * what the module did, not from a second copy of its schedule.
 */
function fakeHost(answer?: Answer) {
  const askedAt: number[] = [];
  const parked: Array<(value: unknown) => void> = [];
  let current: Answer = answer ?? (() => new Promise<unknown>((resolve) => parked.push(resolve)));

  return {
    ping: (): Promise<unknown> => {
      askedAt.push(Date.now());
      return current();
    },
    /** Every question asked so far, at the mocked clock time it went out. */
    askedAt,
    /** What the host says from now on. */
    answers(next: Answer) {
      current = next;
    },
    /** Reply to the parked question — the answer that arrives late. */
    replyLate(value: unknown) {
      for (const resolve of parked.splice(0)) {
        resolve(value);
      }
    },
  };
}

/** Let the callbacks the module queued run, without moving the clock. */
async function settle(): Promise<void> {
  await vi.advanceTimersByTimeAsync(0);
}

/** The decisions a watch reported, in order — what the shell would render. */
function watching(host: { ping: () => Promise<unknown> }) {
  const reported: BackendConnection[] = [];
  return {
    reported,
    stop: watchBackendConnection(host.ping, (connection) => reported.push(connection)),
  };
}

describe("backend connection", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("reaches connected on the first ping that lands, asking once", async () => {
    const host = fakeHost(ANSWERS.host);
    const { reported } = watching(host);

    // The question goes out as soon as there is a watch — the shell asks at
    // startup rather than waiting to be told to.
    expect(host.askedAt).toHaveLength(1);

    await settle();

    expect(reported).toEqual([{ state: "connected", version: PING_OK.appVersion }]);

    // A host that is answering is not asked again, however long the app runs.
    await vi.advanceTimersByTimeAsync(ONE_HOUR_MS);
    expect(host.askedAt).toHaveLength(1);
  });

  it("reports a rejection as no host answering, and keeps asking", async () => {
    const host = fakeHost(ANSWERS.noHost);
    const { reported } = watching(host);

    await settle();

    expect(reported).toEqual([{ state: "unavailable" }]);

    // The next question goes out on its own, a step of the schedule later.
    await vi.advanceTimersByTimeAsync(RETRY_BACKOFF_MS[0]);
    expect(host.askedAt).toHaveLength(2);

    await settle();
    expect(reported.at(-1)).toEqual({ state: "unavailable" });
    await vi.advanceTimersByTimeAsync(RETRY_BACKOFF_MS[1]);
    expect(host.askedAt).toHaveLength(3);
  });

  it("reaches connected when a later ping lands, and the asking stops", async () => {
    const host = fakeHost(ANSWERS.noHost);
    const { reported } = watching(host);

    await settle();

    // The host was there all along; the question the schedule sends next is the
    // one that reaches it.
    host.answers(ANSWERS.host);
    await vi.advanceTimersByTimeAsync(RETRY_BACKOFF_MS[0]);
    expect(host.askedAt).toHaveLength(2);
    await settle();

    expect(reported).toEqual([
      { state: "unavailable" },
      { state: "connected", version: PING_OK.appVersion },
    ]);

    await vi.advanceTimersByTimeAsync(ONE_HOUR_MS);
    expect(host.askedAt).toHaveLength(2);
  });

  it("treats a reply that is not the typed ping as no host answering", async () => {
    const host = fakeHost(ANSWERS.notAPing);
    const { reported } = watching(host);

    await settle();

    expect(reported).toEqual([{ state: "unavailable" }]);
    await vi.advanceTimersByTimeAsync(RETRY_BACKOFF_MS[0]);
    expect(host.askedAt).toHaveLength(2);
  });

  it("does not ask again while a question is still being awaited", async () => {
    const host = fakeHost();
    const { reported } = watching(host);

    await settle();
    await vi.advanceTimersByTimeAsync(QUESTION_TIMEOUT_MS - 1);

    expect(host.askedAt).toHaveLength(1);
    // Still pending: a question nobody has answered yet decides nothing.
    expect(reported).toEqual([]);
  });

  it("counts a question nobody answers as no host answering, and asks again", async () => {
    const host = fakeHost();
    const { reported } = watching(host);

    await settle();
    await vi.advanceTimersByTimeAsync(QUESTION_TIMEOUT_MS);

    // A request left hanging is the lost request this module exists to survive,
    // not a promise that something will eventually reply.
    expect(reported).toEqual([{ state: "unavailable" }]);

    await vi.advanceTimersByTimeAsync(RETRY_BACKOFF_MS[0]);
    expect(host.askedAt).toHaveLength(2);
  });

  it("connects on an answer that arrives after the question was given up on", async () => {
    const host = fakeHost();
    const { reported } = watching(host);

    await settle();
    await vi.advanceTimersByTimeAsync(QUESTION_TIMEOUT_MS);
    expect(reported).toEqual([{ state: "unavailable" }]);

    // The slow host finally answers the question that was already abandoned.
    host.replyLate(PING_OK);
    await settle();

    expect(reported.at(-1)).toEqual({ state: "connected", version: PING_OK.appVersion });

    // And that stops the asking: the retry the timeout scheduled is dropped.
    const asked = host.askedAt.length;
    await vi.advanceTimersByTimeAsync(ONE_HOUR_MS);
    expect(host.askedAt).toHaveLength(asked);
  });

  it("settles a hostless preview into a quiet, bounded poll", async () => {
    const host = fakeHost(ANSWERS.noHost);
    const { reported } = watching(host);

    await settle();
    expect(host.askedAt).toHaveLength(1);

    // Nothing is asked before the first step of the schedule is up: a refusal
    // does not immediately re-ask.
    await vi.advanceTimersByTimeAsync(RETRY_BACKOFF_MS[0] - 1);
    expect(host.askedAt).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(host.askedAt).toHaveLength(2);

    await vi.advanceTimersByTimeAsync(TEN_MINUTES_MS);

    // Bounded: one question per CEILING_MS once the schedule stops growing,
    // plus the short steps at the start. A loop that re-asked on every failure
    // would be in the thousands here.
    expect(host.askedAt.length).toBeLessThanOrEqual(
      RETRY_BACKOFF_MS.length + TEN_MINUTES_MS / CEILING_MS,
    );

    const gaps = host.askedAt.slice(1).map((at, index) => at - host.askedAt[index]);
    expect(Math.min(...gaps)).toBeGreaterThanOrEqual(RETRY_BACKOFF_MS[0]);
    expect(gaps.slice(-3)).toEqual([CEILING_MS, CEILING_MS, CEILING_MS]);

    // And the caller is not told "unavailable" again for every quiet poll: the
    // decision has not changed, so the shell has nothing to re-render.
    expect(reported).toEqual([{ state: "unavailable" }]);
  });

  it("asks nothing, and reports nothing, after the watch is stopped", async () => {
    const host = fakeHost(ANSWERS.noHost);
    const { reported, stop } = watching(host);

    await settle();
    expect(reported).toHaveLength(1);

    // Stopped while the next question is only a pending timer.
    stop();
    host.answers(ANSWERS.host);
    await vi.advanceTimersByTimeAsync(ONE_HOUR_MS);

    expect(host.askedAt).toHaveLength(1);
    expect(reported).toHaveLength(1);
  });

  it("drops an answer that arrives after the watch is stopped", async () => {
    const host = fakeHost();
    const { reported, stop } = watching(host);

    await settle();
    expect(host.askedAt).toHaveLength(1);

    // The view that asked is gone; the question it left in flight is answered
    // by nobody's watch.
    stop();
    host.replyLate(PING_OK);
    await settle();
    await vi.advanceTimersByTimeAsync(ONE_HOUR_MS);

    expect(reported).toEqual([]);
    expect(host.askedAt).toHaveLength(1);
  });
});
