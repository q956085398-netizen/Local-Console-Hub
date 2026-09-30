import { describe, expect, it } from "vitest";

import {
  isAppSummaryDto,
  isBufferSummaryDto,
  isCreatedSessionDto,
  isRunRecordDto,
  isServiceHealthDto,
  isSessionCreatedDto,
  isSessionRemovedDto,
  isSessionRuntimeDto,
  type AppSummaryDto,
  type BufferSummaryDto,
  type RunRecordDto,
  type ServiceHealthDto,
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

/**
 * The wire writes `null` for an absent optional.
 *
 * `SessionRuntime`'s fields are `Option<T>` on the Rust side with no
 * `skip_serializing_if`, and the Rust tests assert exactly that —
 * `a_session_that_has_never_started_claims_no_run` and
 * `a_live_run_reports_no_end_and_no_exit_code` in
 * `src-tauri/src/session/runtime.rs` assert `is_null()` for pid, runId,
 * startedAt, exitCode and lastError. If either side of that pair changes, both
 * must (`src/types/ipc.test.ts` pins `ping` the same way). These fixtures are the real one — the T06 tests
 * next to them use `undefined`, which is why a guard that rejected `null` went
 * unnoticed until a live snapshot was rendered (T07 #8).
 */
function wireSnapshot(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    sessionId: "sillytavern",
    status: "stopped",
    pid: null,
    runId: null,
    startedAt: null,
    exitCode: null,
    ptyAttached: false,
    logging: { mode: "off", source: "none", external_path: null },
    buffer: { bytes: 0, lines: 0, droppedBytes: 0 },
    health: null,
    lastError: null,
    ...overrides,
  };
}

/** A run record as the wire writes it: a live run has null for what has not
 * happened yet, and `logFile` is null for the modes that decide late. */
function wireRun(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    runId: "8f31",
    sessionId: "sillytavern",
    startedAt: "2026-09-28T06:00:00Z",
    endedAt: null,
    exitCode: null,
    pid: null,
    logMode: "on_error",
    logSource: "captured",
    logFile: null,
    ...overrides,
  };
}

describe("the wire's nulls", () => {
  it("accepts a snapshot whose absent optionals are null, not missing", () => {
    expect(isSessionRuntimeDto(wireSnapshot())).toBe(true);
  });

  it("accepts a running snapshot, whose exitCode and lastError are null", () => {
    expect(
      isSessionRuntimeDto(
        wireSnapshot({
          status: "running",
          pid: 12384,
          runId: "a1",
          startedAt: "2026-09-28T06:00:00Z",
        }),
      ),
    ).toBe(true);
  });

  it("accepts a run record whose optionals are null", () => {
    expect(isRunRecordDto(wireRun())).toBe(true);
  });

  it("accepts a health reading under the wire's camelCase keys", () => {
    expect(
      isSessionRuntimeDto(wireSnapshot({ health: { processAlive: true, portOpen: false } })),
    ).toBe(true);
  });
});

describe("isServiceHealthDto", () => {
  it("accepts a reading with both facts", () => {
    const reading: ServiceHealthDto = { processAlive: true, portOpen: true };
    expect(isServiceHealthDto(reading)).toBe(true);
  });

  it("rejects a half-written reading", () => {
    expect(isServiceHealthDto({ processAlive: true })).toBe(false);
    expect(isServiceHealthDto({ portOpen: false })).toBe(false);
    expect(isServiceHealthDto({ processAlive: "yes", portOpen: false })).toBe(false);
    expect(isServiceHealthDto(null)).toBe(false);
  });

  it("lets a snapshot carry no reading at all", () => {
    // Both spellings: the backend writes `null`, a fixture may omit the key.
    // Dropping either would discard a snapshot the UI could have rendered.
    const absent = wireSnapshot();
    delete absent.health;
    expect(isSessionRuntimeDto(absent)).toBe(true);
    expect(isSessionRuntimeDto(wireSnapshot({ health: null }))).toBe(true);
  });

  it("rejects a snapshot whose reading is malformed", () => {
    expect(isSessionRuntimeDto(wireSnapshot({ health: { portOpen: true } }))).toBe(false);
  });
});

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

/** A temporary terminal's configuration, as the created event carries it. */
function terminalConfig(id = "terminal-1a2b") {
  return {
    id,
    name: "PowerShell 1",
    sessionType: "terminal" as const,
    cwd: "C:\\Users\\example",
    shell: "pwsh",
    display: "internal" as const,
    lifecycle: "managed" as const,
    logging: { mode: "off" as const, source: "none" as const },
    temporary: true,
  };
}

describe("the membership payloads (#62)", () => {
  it("accepts a creation that carries its configuration", () => {
    const config = terminalConfig();
    expect(isSessionCreatedDto({ sessionId: config.id, config })).toBe(true);
    // The id has to be the one the configuration is about: a listener files
    // the row by it, and two ids that disagree would file it under a session
    // that does not exist.
    expect(isSessionCreatedDto({ sessionId: "other", config })).toBe(false);
    expect(isSessionCreatedDto({ sessionId: config.id })).toBe(false);
    expect(isSessionCreatedDto({ sessionId: config.id, config: { id: config.id } })).toBe(false);
  });

  it("accepts a removal, and only a removal", () => {
    expect(isSessionRemovedDto({ sessionId: "terminal-1a2b" })).toBe(true);
    expect(isSessionRemovedDto({})).toBe(false);
    // A state payload has a session id too; reading one as a removal would
    // turn "this session is running" into "this session is gone".
    expect(isSessionRemovedDto({ sessionId: "terminal-1a2b", runtime: snapshot() })).toBe(false);
    expect(isSessionRemovedDto({ sessionId: "terminal-1a2b", config: terminalConfig() })).toBe(
      false,
    );
  });

  it("requires both halves of a creation's answer", () => {
    const config = terminalConfig();
    const runtime = { ...snapshot(), sessionId: config.id, status: "running" as const };

    expect(isCreatedSessionDto({ config, runtime })).toBe(true);
    expect(isCreatedSessionDto({ config })).toBe(false);
    expect(isCreatedSessionDto({ runtime })).toBe(false);
    expect(
      isCreatedSessionDto({ config, runtime: { ...runtime, sessionId: "somebody-else" } }),
    ).toBe(false);
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
