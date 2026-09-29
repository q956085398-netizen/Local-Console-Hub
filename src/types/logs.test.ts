/**
 * Frontend guards for the logging DTOs (T10 #11).
 *
 * These are the mirror of `src-tauri/src/logging/` as the IPC contract test in
 * `src-tauri/src/ipc/logs.rs` serializes it: the payload shapes asserted there
 * are the ones accepted here. A field renamed on one side has to fail on the
 * other, which is what makes the mirror worth having.
 */

import { describe, expect, it } from "vitest";
import {
  isCleanupReportDto,
  isLogErrorDto,
  isLogStatusDto,
  isRunHistoryDto,
  isRunHistoryEntryDto,
} from "./logs";

/** The payload `get_log_info` actually sends, nulls and all. */
const wire = {
  sessionId: "comfyui",
  mode: "always",
  source: "captured",
  state: "capturing",
  logFile: "C:/logs/comfyui/2026-09/run.log",
  logFilePresent: true,
  externalLog: null,
  sessionLogDir: "C:/logs/comfyui/2026-09",
  recordsInput: false,
  buffer: { bytes: 24, droppedBytes: 0, lines: 3 },
  truncated: false,
  lastError: null,
};

/** The same payload with one key left out, the way a field rename would send it. */
function without(key: keyof typeof wire): Record<string, unknown> {
  return Object.fromEntries(Object.entries(wire).filter(([name]) => name !== key));
}

describe("log status", () => {
  it("accepts the payload the backend sends, nulls included", () => {
    expect(isLogStatusDto(wire)).toBe(true);
  });

  /// The no-file case is real: a session that persists nothing sends no
  /// `logFile`, and a guard that demanded one would reject it.
  it("accepts a status with no file at all", () => {
    expect(
      isLogStatusDto({
        ...wire,
        mode: "off",
        source: "none",
        state: "off",
        logFile: null,
        logFilePresent: false,
      }),
    ).toBe(true);
  });

  /// The card's file answer, the same question a run-history entry carries:
  /// a payload without it is rejected rather than rendered with a guess about
  /// whether the file the card names is still there (D-022).
  it("requires the current run's file answer", () => {
    expect(isLogStatusDto({ ...wire, logFilePresent: false })).toBe(true);
    expect(isLogStatusDto(without("logFilePresent"))).toBe(false);
    expect(isLogStatusDto({ ...wire, logFilePresent: "true" })).toBe(false);
  });

  it("rejects a state outside the vocabulary", () => {
    expect(isLogStatusDto({ ...wire, state: "writing" })).toBe(false);
  });

  it("rejects a mode the frontend does not know", () => {
    expect(isLogStatusDto({ ...wire, mode: "auto" })).toBe(false);
  });

  it("rejects a status missing its scrollback summary", () => {
    expect(isLogStatusDto(without("buffer"))).toBe(false);
  });

  /// `recordsInput` is the field the UI states rather than implies (§4), so a
  /// payload without it is one the view could not answer honestly.
  it("rejects a status without the stdin answer", () => {
    expect(isLogStatusDto(without("recordsInput"))).toBe(false);
  });

  it("accepts a status carrying a structured logging failure", () => {
    expect(
      isLogStatusDto({
        ...wire,
        lastError: { operation: "opening the run log", message: "拒绝访问。" },
      }),
    ).toBe(true);
  });

  it("rejects a failure that is not shaped like one", () => {
    expect(isLogStatusDto({ ...wire, lastError: { message: "拒绝访问。" } })).toBe(false);
    expect(isLogErrorDto({ operation: "x", message: "y", path: 3 })).toBe(false);
  });
});

describe("run history", () => {
  const history = {
    runs: [
      {
        runId: "c8aa",
        sessionId: "comfyui",
        startedAt: "2026-09-27T02:07:00Z",
        endedAt: null,
        exitCode: null,
        pid: 19002,
        logMode: "always",
        logSource: "captured",
        logFile: "C:/logs/comfyui/2026-09/run.log",
        logFilePresent: true,
      },
    ],
    unreadable: [],
  };

  it("accepts a record whose optional fields are null", () => {
    expect(isRunHistoryDto(history)).toBe(true);
  });

  it("keeps the unreadable list: an incomplete history has to say so", () => {
    expect(isRunHistoryDto({ ...history, unreadable: undefined })).toBe(false);
    expect(
      isRunHistoryDto({
        ...history,
        unreadable: [{ operation: "reading a run record", message: "truncated" }],
      }),
    ).toBe(true);
  });

  it("rejects a history whose runs are not run records", () => {
    expect(isRunHistoryDto({ runs: [{ runId: "c8aa" }], unreadable: [] })).toBe(false);
  });

  /// The one field an entry adds to a record, and the reason the mirror is not
  /// the record's guard alone: a row has to be able to say its log was swept
  /// (`LOGGING.md` §9), and a payload that did not say so would leave the view
  /// offering an action that fails.
  it("reads the file answer off every entry", () => {
    expect(isRunHistoryEntryDto(history.runs[0])).toBe(true);
    expect(isRunHistoryEntryDto({ ...history.runs[0], logFile: null, logFilePresent: false })).toBe(
      true,
    );
  });

  it("rejects an entry that does not say whether the file is there", () => {
    const record = Object.fromEntries(
      Object.entries(history.runs[0]).filter(([key]) => key !== "logFilePresent"),
    );

    expect(isRunHistoryEntryDto(record)).toBe(false);
    expect(isRunHistoryDto({ ...history, runs: [record] })).toBe(false);
    // The wrong spelling of the answer is not an answer either.
    expect(isRunHistoryEntryDto({ ...history.runs[0], logFilePresent: "true" })).toBe(false);
  });
});

describe("cleanup report", () => {
  it("accepts what a sweep reports, including paths as strings", () => {
    expect(
      isCleanupReportDto({
        removed: ["C:/logs/comfyui/2026-09/run-old.log"],
        freedBytes: 4096,
        failures: [],
      }),
    ).toBe(true);
  });

  it("accepts a sweep that took nothing", () => {
    expect(isCleanupReportDto({ removed: [], freedBytes: 0, failures: [] })).toBe(true);
  });

  it("rejects a negative or fractional byte count", () => {
    expect(isCleanupReportDto({ removed: [], freedBytes: -1, failures: [] })).toBe(false);
    expect(isCleanupReportDto({ removed: [], freedBytes: 1.5, failures: [] })).toBe(false);
  });

  it("rejects a removed entry that is not a path string", () => {
    expect(isCleanupReportDto({ removed: [42], freedBytes: 0, failures: [] })).toBe(false);
  });
});
