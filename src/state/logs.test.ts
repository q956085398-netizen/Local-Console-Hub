/**
 * Logs-tab derivations (T10 #11).
 *
 * The rules the tab renders by are asserted here rather than through the DOM,
 * the same way T06 tested its shell rules: what the effective state means,
 * which file an action points at, and what a retention sweep says it will do.
 */

import { describe, expect, it } from "vitest";
import type { CleanupReportDto, LogStatusDto } from "../types/logs";
import type { RunRecordDto } from "../types/runtime";
import { FIXTURE_SESSIONS, type FixtureSession } from "./fixtures";
import {
  bufferNote,
  cleanupOutcome,
  cleanupPrompt,
  currentLogPath,
  formatBytes,
  logActionAvailability,
  logPathEntries,
  logStateLabel,
  logStateTone,
  previewLogStatus,
  runsNewestFirst,
  runHasLog,
  showsSourceBadge,
} from "./logs";

function status(overrides: Partial<LogStatusDto> = {}): LogStatusDto {
  return {
    sessionId: "svc",
    mode: "always",
    source: "captured",
    state: "capturing",
    logFile: "C:/logs/svc/2026-09/run.log",
    externalLog: undefined,
    sessionLogDir: "C:/logs/svc/2026-09",
    recordsInput: false,
    buffer: { bytes: 2048, lines: 12, droppedBytes: 0 },
    truncated: false,
    lastError: undefined,
    ...overrides,
  };
}

function run(overrides: Partial<RunRecordDto> = {}): RunRecordDto {
  return {
    runId: "aaaa",
    sessionId: "svc",
    startedAt: "2026-09-29T08:00:00Z",
    logMode: "always",
    logSource: "captured",
    logFile: "C:/logs/svc/2026-09/run.log",
    ...overrides,
  };
}

function fixture(id: string): FixtureSession {
  const found = FIXTURE_SESSIONS.find((session) => session.config.id === id);
  if (found === undefined) {
    throw new Error(`no fixture session ${id}`);
  }
  return found;
}

describe("effective state", () => {
  it("labels the four states in the logging vocabulary", () => {
    expect(logStateLabel("off")).toBe("Off");
    expect(logStateLabel("capturing")).toBe("Capturing");
    expect(logStateLabel("external")).toBe("External");
    expect(logStateLabel("on_error")).toBe("On error");
  });

  /// Colour is lifecycle truth (UI_STYLE_GUIDE §10), and "nothing is being
  /// written" is not a healthy-green state — a green badge beside an idle
  /// session is the reading LOGGING §1.4 exists to prevent.
  it("keeps every not-writing state neutral and only colours the writing one", () => {
    expect(logStateTone("capturing")).toBe("run");
    expect(logStateTone("off")).toBe("idle");
    expect(logStateTone("on_error")).toBe("warn");
    expect(logStateTone("external")).toBe("warn");
  });
});

describe("which file an action points at", () => {
  it("uses the current run's file for a Hub-captured session", () => {
    expect(currentLogPath(status())).toBe("C:/logs/svc/2026-09/run.log");
  });

  /// D-005: the Hub links an application's log rather than copying it, so an
  /// external session's action points at the application's file even when an
  /// older run record named a Hub path.
  it("uses the application's own log for an external session", () => {
    const external = status({
      source: "external",
      mode: "always",
      state: "external",
      logFile: "C:/logs/svc/2026-09/hub-written.log",
      externalLog: "D:/Tools/SillyTavern/data/access.log",
    });

    expect(currentLogPath(external)).toBe("D:/Tools/SillyTavern/data/access.log");
  });

  it("offers no path when a session persists nothing", () => {
    const off = status({ mode: "off", source: "none", state: "off", logFile: undefined });

    expect(currentLogPath(off)).toBeUndefined();
    expect(logPathEntries(off)).toEqual([{ label: "日志目录", value: "C:/logs/svc/2026-09" }]);
  });

  /// The wire's other spelling of "no file": a session that writes nothing
  /// sends `null`, and `null` must not become a path the UI offers to open.
  it("treats null paths as no path at all", () => {
    const nulled = status({
      mode: "off",
      source: "none",
      state: "off",
      logFile: null,
      externalLog: null,
      sessionLogDir: null,
    });

    expect(currentLogPath(nulled)).toBeUndefined();
    expect(logPathEntries(nulled)).toEqual([]);
    expect(logActionAvailability(nulled).openCurrent).toBe(false);
  });

  it("names the Hub folder as unused for an external session", () => {
    const external = status({
      source: "external",
      state: "external",
      externalLog: "D:/Tools/SillyTavern/data/access.log",
      sessionLogDir: "C:/logs/sillytavern/2026-09",
    });

    expect(logPathEntries(external)).toEqual([
      { label: "应用日志", value: "D:/Tools/SillyTavern/data/access.log" },
      { label: "Hub 日志目录（本会话空白）", value: "C:/logs/sillytavern/2026-09" },
    ]);
  });
});

describe("policy badges", () => {
  /// One fact, one badge: the `external` state already says what the source
  /// badge would repeat (UI_STYLE_GUIDE §13).
  it("drops the source badge when the state already says external", () => {
    expect(showsSourceBadge(status({ source: "external", state: "external" }))).toBe(false);
  });

  it("keeps it when the two badges say different things", () => {
    expect(showsSourceBadge(status({ source: "captured", state: "capturing" }))).toBe(true);
    expect(showsSourceBadge(status({ source: "none", state: "off" }))).toBe(true);
  });
});

describe("which actions the policy offers", () => {
  it("offers nothing file-shaped to a session that writes nothing", () => {
    const off = status({ mode: "off", source: "none", state: "off", logFile: undefined });

    expect(logActionAvailability(off)).toEqual({
      openCurrent: false,
      saveRunLog: false,
      recording: null,
    });
  });

  /// LOGGING §3: `on_error` keeps a buffer and commits it on request, so the
  /// save action is offered by the policy rather than by the presence of a
  /// file — the run that has not failed yet is exactly the case it serves.
  it("offers the save action to an on_error policy before it has a file", () => {
    const onError = status({ mode: "on_error", state: "on_error", logFile: undefined });

    expect(logActionAvailability(onError).saveRunLog).toBe(true);
    // Saving is offered, opening is not: there is nothing on disk to open.
    expect(logActionAvailability(onError).openCurrent).toBe(false);
  });

  it("reads a manual run's switch off its state, not its mode", () => {
    const idle = status({ mode: "manual", state: "off", logFile: undefined });
    const recording = status({ mode: "manual", state: "capturing" });

    expect(logActionAvailability(idle).recording).toBe("start");
    expect(logActionAvailability(recording).recording).toBe("stop");
  });

  it("has no recording switch outside manual mode", () => {
    expect(logActionAvailability(status()).recording).toBeNull();
  });
});

describe("run history", () => {
  it("is ordered newest first whatever the source order was", () => {
    const runs = runsNewestFirst([
      run({ runId: "old", startedAt: "2026-09-27T08:00:00Z" }),
      run({ runId: "new", startedAt: "2026-09-29T08:00:00Z" }),
      run({ runId: "mid", startedAt: "2026-09-28T08:00:00Z" }),
    ]);

    expect(runs.map((entry) => entry.runId)).toEqual(["new", "mid", "old"]);
  });

  /// A run that left no file is still a run: it gets a row, but no file
  /// actions, so the tab never offers to open something that does not exist.
  it("distinguishes a run with a file from one without", () => {
    expect(runHasLog(run())).toBe(true);
    expect(runHasLog(run({ logFile: undefined }))).toBe(false);
    // The wire's other spelling of "no file" — a live run arrives this way.
    expect(runHasLog(run({ logFile: null }))).toBe(false);
  });
});

describe("retention wording", () => {
  const report = (overrides: Partial<CleanupReportDto> = {}): CleanupReportDto => ({
    removed: ["C:/logs/svc/2026-09/run-old.log"],
    freedBytes: 2048,
    failures: [],
    ...overrides,
  });

  it("says how much a sweep would take, and what it will not touch", () => {
    const prompt = cleanupPrompt(report());

    expect(prompt).toContain("1 个文件");
    expect(prompt).toContain("2.0 KiB");
    expect(prompt).toContain("最近一次运行");
  });

  it("does not fabricate a sweep when there is nothing to take", () => {
    expect(cleanupPrompt(report({ removed: [], freedBytes: 0 }))).toContain("没有");
    expect(cleanupOutcome(report({ removed: [], freedBytes: 0 }))).toContain("没有");
  });

  it("reports the files the OS refused rather than claiming a clean sweep", () => {
    const outcome = cleanupOutcome(
      report({ failures: [{ operation: "removing a log file", message: "拒绝访问。" }] }),
    );

    expect(outcome).toContain("已删除 1 个文件");
    expect(outcome).toContain("1 个文件无法删除");
    expect(outcome).toContain("拒绝访问");
  });
});

describe("preview data", () => {
  /// The fixture has to pass the same guards as live data, or the tab would be
  /// rendering a shape the backend never sends.
  it("turns a fixture into the payload get_log_info answers with", () => {
    const preview = previewLogStatus(fixture("comfyui"));

    expect(preview.sessionId).toBe("comfyui");
    expect(preview.mode).toBe("always");
    expect(preview.source).toBe("captured");
    expect(preview.state).toBe("capturing");
    // The current run's file, taken from the fixture's own run record rather
    // than invented for the preview.
    expect(preview.logFile).toContain("__run-c8aa.log");
  });

  /// A stopped session still has a file to open when its last run left one:
  /// Core answers with the last run's file, and the preview has to read the
  /// same way or the tab would hide a log the backend would open.
  it("falls back to the newest run's file for a stopped session", () => {
    const preview = previewLogStatus(fixture("test-api"));

    expect(preview.state).toBe("on_error");
    expect(preview.logFile).toContain("__run-e3f0.log");
    expect(logActionAvailability(preview).openCurrent).toBe(true);
  });

  /// A fixture's `auto`-resolved external session reads as `External` with the
  /// application's path and no Hub-owned file (D-005).
  it("reads an external fixture as external", () => {
    const preview = previewLogStatus(fixture("sillytavern"));

    expect(preview.state).toBe("external");
    expect(preview.logFile).toBeUndefined();
    expect(preview.externalLog).toBe("D:\\Tools\\SillyTavern\\data\\access.log");
    expect(currentLogPath(preview)).toBe("D:\\Tools\\SillyTavern\\data\\access.log");
  });

  /// The interactive terminal's default policy is the one the whole logging
  /// design is built around: a buffer, no file, no stdin (§1.2, §4).
  it("reads a logging-off terminal as off with nothing on disk", () => {
    const preview = previewLogStatus(fixture("pwsh"));

    expect(preview.state).toBe("off");
    expect(preview.mode).toBe("off");
    expect(preview.source).toBe("none");
    expect(currentLogPath(preview)).toBeUndefined();
    expect(preview.recordsInput).toBe(false);
    expect(bufferNote(preview)).toContain("内存缓冲");
  });

  it("sizes the scrollback in units a person reads", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(2048)).toBe("2.0 KiB");
    expect(formatBytes(1024 * 1024 * 3)).toBe("3.0 MiB");
  });

  it("mentions discarded output instead of showing a gapped history", () => {
    const note = bufferNote(status({ buffer: { bytes: 100, lines: 2, droppedBytes: 4096 } }));

    expect(note).toContain("4.0 KiB");
    expect(note).toContain("已丢弃");
  });
});
