import { describe, expect, it, vi } from "vitest";

import {
  createLogActionCoordinator,
  type LogActionBackend,
  type LogActionHandlers,
} from "./log-actions";
import type { LogStatusDto } from "../types/logs";

function status(overrides: Partial<LogStatusDto> = {}): LogStatusDto {
  return {
    sessionId: "term",
    mode: "manual",
    source: "captured",
    state: "capturing",
    logFile: "C:/logs/term/run-1.log",
    logFilePresent: true,
    externalLog: null,
    sessionLogDir: "C:/logs/term",
    recordsInput: false,
    buffer: { bytes: 12, lines: 1, droppedBytes: 0 },
    truncated: false,
    lastError: null,
    ...overrides,
  };
}

function recorder() {
  const received: {
    busy: boolean[];
    statuses: LogStatusDto[];
    successes: string[];
    failures: string[];
    refreshes: number;
  } = { busy: [], statuses: [], successes: [], failures: [], refreshes: 0 };
  const handlers: LogActionHandlers = {
    onStart: vi.fn(),
    onBusyChange: (busy) => received.busy.push(busy),
    onStatus: (value) => received.statuses.push(value),
    onSuccess: (message) => received.successes.push(message),
    onFailure: (message) => received.failures.push(message),
    onRefresh: () => {
      received.refreshes += 1;
    },
  };
  return { received, handlers };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

describe("log action coordination", () => {
  it("applies Session Core's recording state before reporting success", async () => {
    const invoke = vi.fn(async () => status());
    const backend: LogActionBackend = { invoke };
    const sink = recorder();
    const coordinator = createLogActionCoordinator("term", backend, sink.handlers);

    await coordinator.run({ type: "recording", recording: true, successMessage: "已开始记录" });

    expect(invoke).toHaveBeenCalledWith("set_log_recording", {
      sessionId: "term",
      recording: true,
    });
    expect(sink.received.statuses).toEqual([status()]);
    expect(sink.received.successes).toEqual(["已开始记录"]);
    expect(sink.received.failures).toEqual([]);
    expect(sink.received.busy).toEqual([true, false]);
  });

  it("shows a returned last_error instead of claiming a log was saved", async () => {
    const failure = status({
      mode: "on_error",
      state: "on_error",
      logFile: null,
      logFilePresent: false,
      lastError: {
        operation: "writing the log",
        message: "Access is denied; check the folder's permissions",
        path: "C:/logs/term/run-8.log",
      },
    });
    const invoke = vi.fn(async () => failure);
    const backend: LogActionBackend = { invoke };
    const sink = recorder();
    const coordinator = createLogActionCoordinator("term", backend, sink.handlers);

    await coordinator.run({ type: "save", successMessage: "本次运行的缓冲已写入日志文件" });

    expect(sink.received.statuses).toEqual([failure]);
    expect(sink.received.successes).toEqual([]);
    expect(sink.received.failures).toEqual([
      "会话 term · 操作 保存本次运行日志 (save_run_log) · writing the log 失败：Access is denied; check the folder's permissions · 路径：C:/logs/term/run-8.log",
    ]);
    expect(sink.received.refreshes).toBe(0);
  });

  it("exposes the returned saved path before reporting success", async () => {
    const saved = status({
      mode: "on_error",
      state: "on_error",
      logFile: "C:/logs/term/run-8.log",
      logFilePresent: true,
    });
    const invoke = vi.fn(async () => saved);
    const backend: LogActionBackend = { invoke };
    const sink = recorder();
    const events: string[] = [];
    sink.handlers.onStatus = (value) => {
      sink.received.statuses.push(value);
      events.push(`status:${value.logFile}`);
    };
    sink.handlers.onSuccess = (message) => {
      sink.received.successes.push(message);
      events.push("success");
    };
    const coordinator = createLogActionCoordinator("term", backend, sink.handlers);

    await coordinator.run({ type: "save", successMessage: "saved" });

    expect(sink.received.statuses).toEqual([saved]);
    expect(events).toEqual(["status:C:/logs/term/run-8.log", "success"]);
  });

  it("refreshes authoritative state after the command rejects", async () => {
    const invoke = vi.fn(async () => Promise.reject({ message: "no run to save" }));
    const backend: LogActionBackend = { invoke };
    const sink = recorder();
    const coordinator = createLogActionCoordinator("term", backend, sink.handlers, (error) =>
      typeof error === "object" && error !== null && "message" in error
        ? String(error.message)
        : "unknown error",
    );

    await coordinator.run({ type: "save", successMessage: "saved" });

    expect(sink.received.failures).toEqual([
      "会话 term · 操作 保存本次运行日志 (save_run_log)失败：no run to save。正在重新读取日志状态。",
    ]);
    expect(sink.received.refreshes).toBe(1);
    expect(sink.received.successes).toEqual([]);
  });

  it("does not overlap rapid requests and accepts the next action after the first settles", async () => {
    const first = deferred<unknown>();
    const invoke = vi
      .fn<LogActionBackend["invoke"]>()
      .mockReturnValueOnce(first.promise)
      .mockResolvedValueOnce(status({ state: "off" }));
    const backend: LogActionBackend = { invoke };
    const sink = recorder();
    const coordinator = createLogActionCoordinator("term", backend, sink.handlers);

    const starting = coordinator.run({
      type: "recording",
      recording: true,
      successMessage: "started",
    });
    await coordinator.run({ type: "recording", recording: false, successMessage: "stopped" });
    first.resolve(status());
    await starting;
    await coordinator.run({ type: "recording", recording: false, successMessage: "stopped" });

    expect(invoke).toHaveBeenCalledTimes(2);
    expect(sink.received.statuses.map((value) => value.state)).toEqual(["capturing", "off"]);
    expect(sink.received.successes).toEqual(["started", "stopped"]);
  });

  it("ignores a late response after its session view is disposed", async () => {
    const response = deferred<unknown>();
    const invoke = vi.fn(() => response.promise);
    const backend: LogActionBackend = { invoke };
    const sink = recorder();
    const coordinator = createLogActionCoordinator("term", backend, sink.handlers);

    const starting = coordinator.run({
      type: "recording",
      recording: true,
      successMessage: "started",
    });
    coordinator.dispose();
    response.resolve(status());
    await starting;

    expect(sink.received.statuses).toEqual([]);
    expect(sink.received.successes).toEqual([]);
    expect(sink.received.failures).toEqual([]);
    expect(sink.received.busy).toEqual([true]);
  });

  it("rejects a valid status belonging to another session", async () => {
    const invoke = vi.fn(async () => status({ sessionId: "other" }));
    const backend: LogActionBackend = { invoke };
    const sink = recorder();
    const coordinator = createLogActionCoordinator("term", backend, sink.handlers);

    await coordinator.run({ type: "recording", recording: true, successMessage: "started" });

    expect(sink.received.statuses).toEqual([]);
    expect(sink.received.successes).toEqual([]);
    expect(sink.received.failures).toEqual([
      "会话 term · 操作 开始记录 (set_log_recording)失败：后端返回的日志状态无效或属于其他会话。正在重新读取；若问题持续，请检查 Session Core 的日志状态。",
    ]);
    expect(sink.received.refreshes).toBe(1);
  });

  it("explains an unconfirmed recording change with its session and recovery advice", async () => {
    const invoke = vi.fn(async () => status({ state: "off" }));
    const backend: LogActionBackend = { invoke };
    const sink = recorder();
    const coordinator = createLogActionCoordinator("term", backend, sink.handlers);

    await coordinator.run({ type: "recording", recording: true, successMessage: "started" });

    expect(sink.received.failures).toEqual([
      "会话 term · 操作 开始记录 (set_log_recording)失败：后端未确认开始记录。请检查当前日志状态和策略后重试。",
    ]);
  });
});
