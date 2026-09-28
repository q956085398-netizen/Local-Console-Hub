import { describe, expect, it } from "vitest";

import {
  isAppSummaryDto,
  isBufferSummaryDto,
  isRunRecordDto,
  isSessionRuntimeDto,
  type AppSummaryDto,
  type BufferSummaryDto,
  type RunRecordDto,
  type SessionRuntimeDto,
} from "./runtime";

/** A snapshot exactly as `SessionRuntime` serializes on the wire (camelCase,
 * nested `logging.external_path` snake_case — see the type doc for why). */
function snapshot(overrides: Partial<SessionRuntimeDto> = {}): SessionRuntimeDto {
  return {
    sessionId: "sillytavern",
    status: "running",
    pid: 12384,
    runId: "20260928-0a1b2c3d",
    startedAt: "2026-09-28T06:00:00Z",
    exitCode: undefined,
    ptyAttached: true,
    logging: { mode: "always", source: "captured", external_path: undefined },
    buffer: { bytes: 40960, lines: 512, droppedBytes: 0 },
    lastError: undefined,
    ...overrides,
  };
}

describe("isSessionRuntimeDto", () => {
  it("accepts a well-formed running snapshot", () => {
    expect(isSessionRuntimeDto(snapshot())).toBe(true);
  });

  it("accepts a stopped snapshot with all optional fields absent", () => {
    expect(
      isSessionRuntimeDto(
        snapshot({
          status: "stopped",
          pid: undefined,
          runId: undefined,
          startedAt: undefined,
          ptyAttached: false,
          logging: { mode: "off", source: "none", external_path: undefined },
        }),
      ),
    ).toBe(true);
  });

  it("accepts an external logging path under the wire's snake_case key", () => {
    const value = snapshot({
      logging: { mode: "on_error", source: "external", external_path: "D:/Tools/app/data/app.log" },
    });
    expect(isSessionRuntimeDto(value)).toBe(true);
  });

  it("rejects an unknown lifecycle status", () => {
    expect(isSessionRuntimeDto(snapshot({ status: "paused" as never }))).toBe(false);
  });

  it("rejects a snapshot without the buffer summary", () => {
    const value = snapshot() as unknown as Record<string, unknown>;
    delete value.buffer;
    expect(isSessionRuntimeDto(value)).toBe(false);
  });

  it("rejects a non-integer pid", () => {
    expect(isSessionRuntimeDto(snapshot({ pid: 12.5 }))).toBe(false);
  });

  it("rejects a buffer with a negative dropped-bytes counter", () => {
    const buffer: BufferSummaryDto = { bytes: 1, lines: 1, droppedBytes: -1 };
    expect(isBufferSummaryDto(buffer)).toBe(false);
  });
});

describe("isRunRecordDto", () => {
  function record(overrides: Partial<RunRecordDto> = {}): RunRecordDto {
    return {
      runId: "20260928-0a1b2c3d",
      sessionId: "sillytavern",
      startedAt: "2026-09-28T06:00:00Z",
      endedAt: undefined,
      exitCode: undefined,
      pid: 12384,
      logMode: "always",
      logSource: "captured",
      logFile: "%LOCALAPPDATA%/LocalConsoleHub/logs/sillytavern/2026-09/run.log",
      ...overrides,
    };
  }

  it("accepts a live run without an end", () => {
    expect(isRunRecordDto(record())).toBe(true);
  });

  it("accepts a finished run with an exit code", () => {
    expect(
      isRunRecordDto(
        record({
          endedAt: "2026-09-28T07:00:00Z",
          exitCode: 0,
          pid: undefined,
          logMode: "off",
          logSource: "none",
          logFile: undefined,
        }),
      ),
    ).toBe(true);
  });

  it("rejects an unknown log mode", () => {
    expect(isRunRecordDto(record({ logMode: "auto" as never }))).toBe(false);
  });

  it("rejects a run whose timestamps are not RFC 3339 strings", () => {
    expect(isRunRecordDto(record({ startedAt: 1760000000000 as never }))).toBe(false);
  });
});

describe("isAppSummaryDto", () => {
  it("accepts plain counts", () => {
    const summary: AppSummaryDto = { total: 4, running: 2, error: 0 };
    expect(isAppSummaryDto(summary)).toBe(true);
  });

  it("rejects missing counts", () => {
    expect(isAppSummaryDto({ total: 4, running: 2 })).toBe(false);
  });
});
