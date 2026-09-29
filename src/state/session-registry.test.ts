import { afterEach, describe, expect, it, vi } from "vitest";
import { FIXTURE_SESSIONS } from "./fixtures";
import {
  MAX_BUFFERED_SESSION_EVENTS,
  watchSessionRegistry,
  type SessionRegistryBackend,
  type SessionRegistrySnapshot,
} from "./session-registry";
import { retryDelayMs } from "./retry-schedule";
import type { SessionRuntimeDto } from "../types/runtime";

const CONFIG_REPORT = {
  fileStatus: "loaded" as const,
  sessions: FIXTURE_SESSIONS.map((session) => session.config),
  errors: [],
};

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

async function flushPromises() {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

afterEach(() => {
  vi.useRealTimers();
});

function event(sessionId: string, status: SessionRuntimeDto["status"]) {
  const source = FIXTURE_SESSIONS[0];
  return {
    sessionId,
    runtime: { ...source.runtime, sessionId, status },
  };
}

describe("the live session registry", () => {
  it("subscribes before reading and lets events during loading win over the older snapshot", async () => {
    const subscription = deferred<() => void>();
    const configs = deferred<unknown>();
    const runtimes = deferred<unknown>();
    let receive: (payload: unknown) => void = () => {};
    const unlisten = vi.fn();
    const backend: SessionRegistryBackend = {
      subscribe: vi.fn((handler) => {
        receive = handler;
        return subscription.promise;
      }),
      listConfigs: vi.fn(() => configs.promise),
      listSessions: vi.fn(() => runtimes.promise),
      getConfigReport: vi.fn(() => Promise.resolve(CONFIG_REPORT)),
    };
    const reports: SessionRegistrySnapshot[] = [];
    const stop = watchSessionRegistry(backend, (snapshot) => reports.push(snapshot));
    const configured = FIXTURE_SESSIONS[0];

    expect(reports[0]).toMatchObject({ phase: "loading", sessions: [] });
    expect(backend.listConfigs).not.toHaveBeenCalled();
    receive(event(configured.config.id, "running"));
    subscription.resolve(unlisten);
    await flushPromises();

    expect(backend.listConfigs).toHaveBeenCalledOnce();
    expect(backend.listSessions).toHaveBeenCalledOnce();

    receive(event(configured.config.id, "running"));
    configs.resolve([configured.config]);
    await flushPromises();
    receive(event(configured.config.id, "stopping"));
    receive(event("not-configured", "running"));
    runtimes.resolve([{ ...configured.runtime, status: "stopped" }]);
    await flushPromises();

    const ready = reports.at(-1);
    expect(ready?.phase).toBe("ready");
    expect(ready?.sessions).toHaveLength(1);
    expect(ready?.sessions[0]?.config.id).toBe(configured.config.id);
    expect(ready?.sessions[0]?.runtime.status).toBe("stopping");
    expect(ready?.sessions.some((session) => session.config.id === "not-configured")).toBe(false);
    receive(event(configured.config.id, "running"));
    expect(reports.at(-1)?.sessions[0]?.runtime.status).toBe("running");
    expect(reports.at(-1)?.sessionRevision).toBe(1);
    const reportCount = reports.length;
    receive(event("not-configured", "error"));
    expect(reports).toHaveLength(reportCount);

    stop();
    expect(unlisten).toHaveBeenCalledOnce();
  });

  it("applies an event when the runtime list resolves before the config list", async () => {
    const configured = FIXTURE_SESSIONS[0];
    const configs = deferred<unknown>();
    const runtimes = deferred<unknown>();
    let receive: (payload: unknown) => void = () => {};
    const backend: SessionRegistryBackend = {
      subscribe: vi.fn((handler) => {
        receive = handler;
        return Promise.resolve(() => {});
      }),
      listConfigs: () => configs.promise,
      listSessions: () => runtimes.promise,
      getConfigReport: () => Promise.resolve(CONFIG_REPORT),
    };
    const reports: SessionRegistrySnapshot[] = [];
    const stop = watchSessionRegistry(backend, (snapshot) => reports.push(snapshot));
    await flushPromises();

    runtimes.resolve([{ ...configured.runtime, status: "stopped" }]);
    await flushPromises();
    receive(event(configured.config.id, "running"));
    configs.resolve([configured.config]);
    await flushPromises();

    expect(reports.at(-1)).toMatchObject({
      phase: "ready",
      sessions: [{ runtime: { status: "running" } }],
    });
    stop();
  });

  it("ignores snapshot results that settle after the registry has stopped", async () => {
    const subscription = deferred<() => void>();
    const configs = deferred<unknown>();
    const runtimes = deferred<unknown>();
    const configReport = deferred<unknown>();
    const unlisten = vi.fn();
    const backend: SessionRegistryBackend = {
      subscribe: () => subscription.promise,
      listConfigs: () => configs.promise,
      listSessions: () => runtimes.promise,
      getConfigReport: () => configReport.promise,
    };
    const reports: SessionRegistrySnapshot[] = [];
    const stop = watchSessionRegistry(backend, (snapshot) => reports.push(snapshot));

    subscription.resolve(unlisten);
    await flushPromises();
    stop();
    configs.resolve(FIXTURE_SESSIONS.map((session) => session.config));
    runtimes.resolve(FIXTURE_SESSIONS.map((session) => session.runtime));
    configReport.resolve(CONFIG_REPORT);
    await flushPromises();

    expect(unlisten).toHaveBeenCalledOnce();
    expect(reports).toHaveLength(1);
    expect(reports[0]?.phase).toBe("loading");
    expect(reports[0]?.sessions).toEqual([]);
  });

  it("unsubscribes a listener that resolves after cancellation without starting reads", async () => {
    const subscription = deferred<() => void>();
    const unlisten = vi.fn();
    const backend: SessionRegistryBackend = {
      subscribe: () => subscription.promise,
      listConfigs: vi.fn(() => Promise.resolve([])),
      listSessions: vi.fn(() => Promise.resolve([])),
      getConfigReport: () => Promise.resolve(CONFIG_REPORT),
    };
    const reports: SessionRegistrySnapshot[] = [];
    const stop = watchSessionRegistry(backend, (snapshot) => reports.push(snapshot));

    stop();
    subscription.resolve(unlisten);
    await flushPromises();

    expect(unlisten).toHaveBeenCalledOnce();
    expect(backend.listConfigs).not.toHaveBeenCalled();
    expect(backend.listSessions).not.toHaveBeenCalled();
    expect(reports).toHaveLength(1);
  });

  it("shows subscription failures and recovers on the capped retry cadence", async () => {
    vi.useFakeTimers();
    const configured = FIXTURE_SESSIONS[0];
    const unlisten = vi.fn();
    let subscriptions = 0;
    const backend: SessionRegistryBackend = {
      subscribe: vi.fn(() => {
        subscriptions += 1;
        return subscriptions === 1
          ? Promise.reject(new Error("listener unavailable"))
          : Promise.resolve(unlisten);
      }),
      listConfigs: vi.fn(() => Promise.resolve([configured.config])),
      listSessions: vi.fn(() => Promise.resolve([configured.runtime])),
      getConfigReport: vi.fn(() => Promise.resolve(CONFIG_REPORT)),
    };
    const reports: SessionRegistrySnapshot[] = [];
    const stop = watchSessionRegistry(backend, (snapshot) => reports.push(snapshot));
    await flushPromises();

    expect(reports.at(-1)).toMatchObject({ phase: "loading", error: "listener unavailable" });
    expect(backend.listConfigs).not.toHaveBeenCalled();
    expect(retryDelayMs(99)).toBe(30_000);

    await vi.advanceTimersByTimeAsync(retryDelayMs(0));
    expect(subscriptions).toBe(2);
    expect(reports.at(-1)).toMatchObject({ phase: "ready", error: null });

    stop();
    expect(unlisten).toHaveBeenCalledOnce();
  });

  it("shows list failures and later replaces them with the recovered snapshot", async () => {
    vi.useFakeTimers();
    const configured = FIXTURE_SESSIONS[0];
    let reads = 0;
    const unlisten = vi.fn();
    const subscribe = vi.fn(() => Promise.resolve(unlisten));
    const backend: SessionRegistryBackend = {
      subscribe,
      listConfigs: () => {
        reads += 1;
        return reads === 1
          ? Promise.reject(new Error("config list unavailable"))
          : Promise.resolve([configured.config]);
      },
      listSessions: () => Promise.resolve([configured.runtime]),
      getConfigReport: () => Promise.resolve(CONFIG_REPORT),
    };
    const reports: SessionRegistrySnapshot[] = [];
    const stop = watchSessionRegistry(backend, (snapshot) => reports.push(snapshot));
    await flushPromises();

    expect(reports.at(-1)).toMatchObject({ phase: "loading", error: "config list unavailable" });
    expect(unlisten).toHaveBeenCalledOnce();
    await vi.advanceTimersByTimeAsync(retryDelayMs(0));
    expect(reports.at(-1)).toMatchObject({ phase: "ready", error: null });
    expect(subscribe).toHaveBeenCalledTimes(2);
    expect(unlisten).toHaveBeenCalledOnce();
    expect(reports.at(-1)?.sessions.map((session) => session.config.id)).toEqual([
      configured.config.id,
    ]);

    stop();
    expect(unlisten).toHaveBeenCalledTimes(2);
  });

  it("discards events from a failed initialization attempt before retrying", async () => {
    vi.useFakeTimers();
    const configured = FIXTURE_SESSIONS[0];
    const firstConfigRead = deferred<unknown>();
    const receives: Array<(payload: unknown) => void> = [];
    let configReads = 0;
    const backend: SessionRegistryBackend = {
      subscribe: vi.fn((receive) => {
        receives.push(receive);
        return Promise.resolve(() => {});
      }),
      listConfigs: () => {
        configReads += 1;
        return configReads === 1 ? firstConfigRead.promise : Promise.resolve([configured.config]);
      },
      listSessions: () => Promise.resolve([{ ...configured.runtime, status: "stopped" }]),
      getConfigReport: () => Promise.resolve(CONFIG_REPORT),
    };
    const reports: SessionRegistrySnapshot[] = [];
    const stop = watchSessionRegistry(backend, (snapshot) => reports.push(snapshot));
    await flushPromises();
    receives[0]?.(event(configured.config.id, "running"));
    firstConfigRead.reject(new Error("temporary config read failure"));
    await flushPromises();

    expect(reports.at(-1)).toMatchObject({
      phase: "loading",
      error: "temporary config read failure",
    });
    await vi.advanceTimersByTimeAsync(retryDelayMs(0));

    expect(receives).toHaveLength(2);
    expect(reports.at(-1)).toMatchObject({
      phase: "ready",
      sessions: [{ runtime: { status: "stopped" } }],
    });
    stop();
  });

  it("resynchronizes when the bounded pre-snapshot event cache overflows", async () => {
    vi.useFakeTimers();
    const configs = deferred<unknown>();
    const runtimes = deferred<unknown>();
    const receives: Array<(payload: unknown) => void> = [];
    const source = FIXTURE_SESSIONS[0];
    const allConfigs = Array.from({ length: MAX_BUFFERED_SESSION_EVENTS + 1 }, (_, index) => ({
      ...source.config,
      id: `session-${index}`,
      name: `Session ${index}`,
    }));
    let readCount = 0;
    const backend: SessionRegistryBackend = {
      subscribe: (handler) => {
        receives.push(handler);
        return Promise.resolve(() => {});
      },
      listConfigs: () => (readCount === 0 ? configs.promise : Promise.resolve(allConfigs)),
      listSessions: () => {
        readCount += 1;
        return readCount === 1
          ? runtimes.promise
          : Promise.resolve(
              allConfigs.map((config) => ({
                ...source.runtime,
                sessionId: config.id,
                status: "stopped",
              })),
            );
      },
      getConfigReport: () => Promise.resolve(CONFIG_REPORT),
    };
    const reports: SessionRegistrySnapshot[] = [];
    const stop = watchSessionRegistry(backend, (snapshot) => reports.push(snapshot));
    await flushPromises();

    for (const config of allConfigs) {
      receives[0]?.({
        sessionId: config.id,
        runtime: { ...source.runtime, sessionId: config.id, status: "stopped" },
      });
    }
    configs.resolve(allConfigs);
    runtimes.resolve(allConfigs.map((config) => ({ ...source.runtime, sessionId: config.id })));
    await flushPromises();

    expect(reports.at(-1)).toMatchObject({ phase: "loading", error: expect.any(String) });
    expect(reports.some((snapshot) => snapshot.phase === "ready")).toBe(false);

    await vi.advanceTimersByTimeAsync(retryDelayMs(0));

    expect(receives).toHaveLength(2);
    expect(reports.at(-1)?.phase).toBe("ready");
    expect(reports.at(-1)?.sessions).toHaveLength(MAX_BUFFERED_SESSION_EVENTS + 1);
    expect(reports.at(-1)?.sessions.every((session) => session.runtime.status === "stopped")).toBe(
      true,
    );
    stop();
  });

  it("shows config-report read failures and recovers without blocking sessions", async () => {
    vi.useFakeTimers();
    const configured = FIXTURE_SESSIONS[0];
    let reportReads = 0;
    const backend: SessionRegistryBackend = {
      subscribe: () => Promise.resolve(() => {}),
      listConfigs: () => Promise.resolve([configured.config]),
      listSessions: () => Promise.resolve([configured.runtime]),
      getConfigReport: () => {
        reportReads += 1;
        return reportReads === 1
          ? Promise.reject(new Error("config report unavailable"))
          : Promise.resolve(CONFIG_REPORT);
      },
    };
    const reports: SessionRegistrySnapshot[] = [];
    const stop = watchSessionRegistry(backend, (snapshot) => reports.push(snapshot));
    await flushPromises();

    expect(reports.at(-1)).toMatchObject({
      phase: "ready",
      configReport: null,
      configReportError: "config report unavailable",
    });

    await vi.advanceTimersByTimeAsync(retryDelayMs(0));
    expect(reports.at(-1)?.configReport).toEqual(CONFIG_REPORT);
    expect(reports.at(-1)?.configReportError).toBeNull();
    stop();
  });
});
