import { afterEach, describe, expect, it, vi } from "vitest";

import {
  INITIAL_CONNECTION,
  retryDelayMs,
  watchBackendConnection,
  type BackendConnection,
} from "./backend-connection";

/** A settled ping that this app would accept. */
const PING_RESPONSE = { appName: "Local Console Hub", appVersion: "0.1.0", protocol: 1 };

/** A host that never answers — the browser preview, and a cold WebView. */
function noHost(): Promise<unknown> {
  return Promise.reject(new Error("no host"));
}

/** A host that is not answering yet, then is. */
function hostAfter(failures: number): () => Promise<unknown> {
  let calls = 0;
  return () => {
    calls += 1;
    return calls <= failures ? noHost() : Promise.resolve(PING_RESPONSE);
  };
}

/**
 * Drive the module with fake timers and collect what it reported.
 *
 * `advanceTimersByTimeAsync` flushes the promises the timers resolve into, so
 * a test asserts on what the *module* decided rather than sprinkling awaits
 * that would hide where the decision is.
 */
async function watch(ping: () => Promise<unknown>, milliseconds: number) {
  const reports: BackendConnection[] = [];
  const stop = watchBackendConnection(ping, (connection) => reports.push(connection));
  await vi.advanceTimersByTimeAsync(milliseconds);
  return { reports, stop };
}

describe("the backend connection decision", () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("starts pending, before anything has asked", () => {
    expect(INITIAL_CONNECTION).toEqual({ state: "pending" });
  });

  it("reports connected on the first answer and never asks again", async () => {
    vi.useFakeTimers();
    const ping = vi.fn(() => Promise.resolve(PING_RESPONSE));

    const { reports } = await watch(ping, 10 * 60_000);

    expect(reports).toEqual([{ state: "connected", version: "0.1.0" }]);
    expect(ping).toHaveBeenCalledTimes(1);
  });

  it("reports a rejection as no host answering, and keeps asking", async () => {
    vi.useFakeTimers();
    const ping = vi.fn(noHost);

    const { reports } = await watch(ping, 1_000);

    expect(reports).toEqual([{ state: "unavailable" }]);
    expect(ping.mock.calls.length).toBeGreaterThan(1);
  });

  it("picks up a host that answers later, and then stops asking", async () => {
    vi.useFakeTimers();
    const ping = vi.fn(hostAfter(2));

    // Two failures cost the first two gaps of the schedule: 500 ms, then
    // 1 000 ms, so the third attempt lands inside a two-second window.
    const { reports } = await watch(ping, 2_000);

    expect(reports).toEqual([{ state: "unavailable" }, { state: "connected", version: "0.1.0" }]);
    const asked = ping.mock.calls.length;
    await vi.advanceTimersByTimeAsync(10 * 60_000);
    expect(ping.mock.calls.length).toBe(asked);
  });

  it("does not call a settled answer that is not the ping contract a connection", async () => {
    vi.useFakeTimers();
    // A host that answers, but not with this app's handshake — a protocol
    // mismatch is not a live backend, and reading it as one would render the
    // live workspace from payloads nothing can parse.
    const ping = vi.fn(() => Promise.resolve({ hello: "world" }));

    const { reports } = await watch(ping, 1_000);

    expect(reports).toEqual([{ state: "unavailable" }]);
  });

  it("keeps a bounded cadence instead of spinning", async () => {
    vi.useFakeTimers();
    const ping = vi.fn(noHost);

    const { reports } = await watch(ping, 60_000);

    // A hot loop would be thousands of calls in a minute; a bounded backoff
    // is a handful. The exact number is the schedule's business — the test
    // only pins that it is bounded and that the answer is one reading, not
    // one per attempt.
    expect(ping.mock.calls.length).toBeLessThanOrEqual(12);
    expect(reports).toEqual([{ state: "unavailable" }]);
  });

  it("stops asking when the watch is stopped, and ignores a late answer", async () => {
    vi.useFakeTimers();
    let settle: (value: unknown) => void = () => {};
    const ping = vi.fn(() => new Promise<unknown>((resolve) => (settle = resolve)));

    const { reports, stop } = await watch(ping, 0);
    const asked = ping.mock.calls.length;
    stop();
    settle(PING_RESPONSE);
    await vi.advanceTimersByTimeAsync(10 * 60_000);

    expect(reports).toEqual([]);
    expect(ping.mock.calls.length).toBe(asked);
  });
});

describe("the retry schedule", () => {
  it("never gets shorter, and holds at its ceiling", () => {
    const later = (attempt: number) => retryDelayMs(attempt);

    for (let attempt = 1; attempt < 20; attempt += 1) {
      expect(later(attempt)).toBeGreaterThanOrEqual(later(attempt - 1));
    }
    expect(later(999)).toBe(later(5));
    expect(later(0)).toBeGreaterThan(0);
  });
});
