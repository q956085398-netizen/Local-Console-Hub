/**
 * Logs-tab derivations (T10 #11).
 *
 * The rules the tab renders by are asserted here rather than through the DOM,
 * the same way T06 tested its shell rules: what the effective state means,
 * which file an action points at, and what a retention sweep says it will do.
 */

import { describe, expect, it } from "vitest";
import type { CleanupReportDto, LogStatusDto, RunHistoryEntryDto } from "../types/logs";
import { FIXTURE_SESSIONS } from "./fixtures";
import type { SessionView } from "./session-view";
import {
  bufferNote,
  cleanupOutcome,
  cleanupPrompt,
  currentLogFile,
  currentLogPath,
  fileActions,
  formatBytes,
  logActionAvailability,
  logPathEntries,
  logStateLabel,
  logStateTone,
  previewLogStatus,
  previewRuns,
  runsNewestFirst,
  runFilePathNote,
  runHasLog,
  runLogFile,
  runLogPresent,
  showsSourceBadge,
} from "./logs";

function status(overrides: Partial<LogStatusDto> = {}): LogStatusDto {
  return {
    sessionId: "svc",
    mode: "always",
    source: "captured",
    state: "capturing",
    logFile: "C:/logs/svc/2026-09/run.log",
    logFilePresent: true,
    externalLog: undefined,
    sessionLogDir: "C:/logs/svc/2026-09",
    recordsInput: false,
    buffer: { bytes: 2048, lines: 12, droppedBytes: 0 },
    truncated: false,
    lastError: undefined,
    ...overrides,
  };
}

function run(overrides: Partial<RunHistoryEntryDto> = {}): RunHistoryEntryDto {
  return {
    runId: "aaaa",
    sessionId: "svc",
    startedAt: "2026-09-29T08:00:00Z",
    logMode: "always",
    logSource: "captured",
    logFile: "C:/logs/svc/2026-09/run.log",
    logFilePresent: true,
    ...overrides,
  };
}

function fixture(id: string): SessionView {
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
    expect(fileActions(currentLogFile(nulled))).toEqual({
      open: false,
      copy: false,
      folder: false,
    });
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

describe("the current run's file answer", () => {
  /// D-022 asked of the card, not only of the history: a log deleted behind the
  /// Hub's back — by the OS, by a person, by retention — is answered from the
  /// filesystem when the tab reads, so the card stops offering an action that
  /// fails when it is clicked.
  it("says a file that is not on disk is not on disk", () => {
    const gone = status({ logFilePresent: false });

    expect(currentLogFile(gone)).toEqual({ path: "C:/logs/svc/2026-09/run.log", present: false });
    expect(logPathEntries(gone)).toEqual([
      {
        label: "当前运行",
        value: "日志文件不在磁盘上（运行记录保留） · C:/logs/svc/2026-09/run.log",
      },
      { label: "日志目录", value: "C:/logs/svc/2026-09" },
    ]);
  });

  /// The wording names the fact and not a cause: this slot cannot tell a swept
  /// log from an `external` application's file that has not been written yet,
  /// and "已被清理" would be a claim about something nobody watched happen.
  it("does not name a cause for the missing file", () => {
    const note = logPathEntries(status({ logFilePresent: false }))[0].value;

    expect(note).toContain("不在磁盘上");
    expect(note).not.toContain("清理");
    expect(note).not.toContain("删除");
  });

  it("leaves the path alone when the file is there", () => {
    expect(logPathEntries(status())).toEqual([
      { label: "当前运行", value: "C:/logs/svc/2026-09/run.log" },
      { label: "日志目录", value: "C:/logs/svc/2026-09" },
    ]);
  });

  /// One run cannot be offered two different action sets depending on whether
  /// the user looks at the card or at its row in the history: both read
  /// `fileActions`, so a file that is gone costs both of them 打开日志 and
  /// 复制路径 while the folder survives in both (D-022, LOGGING §9/§10).
  it("offers the card and the run's row the same actions for the same file", () => {
    const present = status();
    const presentRow = run();
    expect(fileActions(currentLogFile(present))).toEqual(fileActions(runLogFile(presentRow)));
    expect(fileActions(currentLogFile(present))).toEqual({
      open: true,
      copy: true,
      folder: true,
    });

    const gone = status({ logFilePresent: false });
    const goneRow = run({ logFilePresent: false });
    expect(fileActions(currentLogFile(gone))).toEqual(fileActions(runLogFile(goneRow)));
    expect(fileActions(currentLogFile(gone))).toEqual({
      open: false,
      copy: false,
      folder: true,
    });
  });

  /// D-005: an `external` session's current file is the application's own, and
  /// the card keeps naming it — so the answer has to be about that file, not
  /// about the Hub-written one the session does not have. Its *rows* are
  /// unchanged; this is the card reading the same fact about the same file.
  it("asks an external session's answer about the application's file", () => {
    const applicationOwned = "D:/Tools/SillyTavern/data/access.log";
    const external = status({
      source: "external",
      state: "external",
      logFile: undefined,
      externalLog: applicationOwned,
      logFilePresent: true,
    });

    expect(currentLogFile(external)).toEqual({ path: applicationOwned, present: true });
    expect(fileActions(currentLogFile(external))).toEqual({
      open: true,
      copy: true,
      folder: true,
    });

    const unwritten = status({
      source: "external",
      state: "external",
      logFile: undefined,
      externalLog: applicationOwned,
      logFilePresent: false,
    });

    expect(logPathEntries(unwritten)[0]).toEqual({
      label: "应用日志",
      value: `日志文件不在磁盘上（运行记录保留） · ${applicationOwned}`,
    });
    expect(fileActions(currentLogFile(unwritten))).toEqual({
      open: false,
      copy: false,
      folder: true,
    });
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
    const off = status({
      mode: "off",
      source: "none",
      state: "off",
      logFile: undefined,
      logFilePresent: false,
    });

    expect(logActionAvailability(off, "running")).toEqual({ saveRunLog: false, recording: null });
    expect(fileActions(currentLogFile(off))).toEqual({ open: false, copy: false, folder: false });
  });

  /// LOGGING §3: `on_error` keeps a buffer and commits it on request while a
  /// Core owns a current run during Running and Stopping. A completed failing
  /// run already has its auto-saved history entry, so it must not offer Save.
  it("offers Save only while Session Core owns the on_error run", () => {
    const onError = status({
      mode: "on_error",
      state: "on_error",
      logFile: undefined,
      logFilePresent: false,
    });

    expect(logActionAvailability(onError, "running").saveRunLog).toBe(true);
    expect(logActionAvailability(onError, "stopping").saveRunLog).toBe(true);
    expect(logActionAvailability(onError, "starting").saveRunLog).toBe(false);
    expect(logActionAvailability(onError, "error").saveRunLog).toBe(false);
    expect(logActionAvailability(onError, "exited").saveRunLog).toBe(false);
    expect(logActionAvailability(onError, "stopped").saveRunLog).toBe(false);
    // Saving is offered, opening is not: there is nothing on disk to open.
    expect(fileActions(currentLogFile(onError))).toEqual({
      open: false,
      copy: false,
      folder: false,
    });
  });

  it("reads a manual run's switch off its state, not its mode", () => {
    const idle = status({ mode: "manual", state: "off", logFile: undefined });
    const recording = status({ mode: "manual", state: "capturing" });

    expect(logActionAvailability(idle, "running").recording).toBe("start");
    expect(logActionAvailability(recording, "running").recording).toBe("stop");
    expect(logActionAvailability(recording, "stopping").recording).toBe("stop");
    expect(logActionAvailability(idle, "starting").recording).toBeNull();
    expect(logActionAvailability(idle, "stopping").recording).toBeNull();
    expect(logActionAvailability(idle, "stopped").recording).toBeNull();
  });

  it("has no recording switch outside manual mode", () => {
    expect(logActionAvailability(status(), "running").recording).toBeNull();
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

  /// §9's sweep and §6's history, from the row's point of view: the log a run
  /// wrote can be gone while the run itself is still the record that it ran.
  /// The row has to say so — an action offered on a file that no longer exists
  /// is what the user would otherwise see, as a failure notice.
  it("marks a run whose log is no longer on disk", () => {
    const swept = run({ logFilePresent: false });

    expect(runHasLog(swept)).toBe(true);
    expect(runLogPresent(swept)).toBe(false);
    expect(runLogFile(swept)).toEqual({ path: "C:/logs/svc/2026-09/run.log", present: false });
  });

  /// The three row states, each said in its own words — and the third says
  /// only what this slot knows: a swept log and an `external` file that has not
  /// been written yet look the same from here, so the row does not name a cause
  /// it never observed (§1.4).
  it("says where a run's log is, or why there is nothing to open", () => {
    const path = "C:/logs/svc/2026-09/run.log";

    expect(runFilePathNote(run())).toBe(path);
    expect(runFilePathNote(run({ logFile: null, logFilePresent: false }))).toBe("未落盘");
    expect(runFilePathNote(run({ logFilePresent: false }))).toBe(
      `日志文件不在磁盘上（运行记录保留） · ${path}`,
    );
  });

  /// The two questions a row asks are different ones, and neither implies the
  /// other: a run that wrote nothing has no log and nothing missing, while a
  /// swept run still names the file it wrote. Both states cost the row its
  /// open/copy actions, and only the second keeps the folder.
  it("separates 'never wrote a log' from 'the log is gone'", () => {
    const nothingWritten = run({ logFile: null, logFilePresent: false });
    const swept = run({ logFilePresent: false });

    expect(runHasLog(nothingWritten)).toBe(false);
    expect(runLogFile(nothingWritten).path).toBeUndefined();
    expect(runLogFile(swept).path).toBe("C:/logs/svc/2026-09/run.log");
    expect(fileActions(runLogFile(nothingWritten))).toEqual({
      open: false,
      copy: false,
      folder: false,
    });
  });

  /// The row's buttons, decided here rather than in the component: what a run
  /// can do is a fact about its file, and the panel is a renderer of that.
  /// §10 lists the actions; §9 is why the third row loses two of them — the
  /// folder survives a sweep, the file does not.
  it("offers a row only the file actions its run can carry out", () => {
    expect(fileActions(runLogFile(run()))).toEqual({ open: true, copy: true, folder: true });
    expect(fileActions(runLogFile(run({ logFilePresent: false })))).toEqual({
      open: false,
      copy: false,
      folder: true,
    });
    expect(fileActions(runLogFile(run({ logFile: null, logFilePresent: false })))).toEqual({
      open: false,
      copy: false,
      folder: false,
    });
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

    expect(prompt).toContain("1 个日志文件");
    expect(prompt).toContain("2.0 KiB");
    expect(prompt).toContain("最近一次运行");
  });

  /// The confirmation has to be about files: a sweep takes logs, never the run
  /// records, and a user agreeing to it should not have to wonder whether they
  /// are about to lose their history (§6, §9).
  it("promises the run history survives the sweep", () => {
    expect(cleanupPrompt(report())).toContain("运行历史记录");
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
    expect(preview.logFilePresent).toBe(true);
  });

  /// A stopped session still has a file to open when its last run left one:
  /// Core answers with the last run's file, and the preview has to read the
  /// same way or the tab would hide a log the backend would open.
  it("falls back to the newest run's file for a stopped session", () => {
    const preview = previewLogStatus(fixture("test-api"));

    expect(preview.state).toBe("on_error");
    expect(preview.logFile).toContain("__run-e3f0.log");
    expect(preview.logFilePresent).toBe(true);
    expect(fileActions(currentLogFile(preview))).toEqual({ open: true, copy: true, folder: true });
  });

  /// A fixture's `auto`-resolved external session reads as `External` with the
  /// application's path and no Hub-owned file (D-005).
  it("reads an external fixture as external", () => {
    const preview = previewLogStatus(fixture("sillytavern"));

    expect(preview.state).toBe("external");
    expect(preview.logFile).toBeUndefined();
    expect(preview.externalLog).toBe("D:\\Tools\\SillyTavern\\data\\access.log");
    expect(currentLogPath(preview)).toBe("D:\\Tools\\SillyTavern\\data\\access.log");
    // The linked application log is the file the card acts on, so the file
    // answer is about it — the Hub's own nil `logFile` is not the question.
    expect(preview.logFilePresent).toBe(true);
  });

  /// The interactive terminal's default policy is the one the whole logging
  /// design is built around: a buffer, no file, no stdin (§1.2, §4).
  it("reads a logging-off terminal as off with nothing on disk", () => {
    const preview = previewLogStatus(fixture("pwsh"));

    expect(preview.state).toBe("off");
    expect(preview.mode).toBe("off");
    expect(preview.source).toBe("none");
    expect(currentLogPath(preview)).toBeUndefined();
    expect(preview.logFilePresent).toBe(false);
    expect(preview.recordsInput).toBe(false);
    expect(bufferNote(preview)).toContain("内存缓冲");
  });

  /// The preview mirror answers the same question the live payload does, or a
  /// fixture row would be rendered through a shape the backend never sends and
  /// the guards would reject it.
  ///
  /// comfyui carries one swept run on purpose (`FixtureRun.logFilePresent`):
  /// without it there is no way to look at that row without a live backend and
  /// a real sweep, and the fixture is the only place that state can be seen.
  it("gives a fixture's runs the file answer the live payload carries", () => {
    const runs = previewRuns(fixture("comfyui"));

    expect(runs.length).toBeGreaterThan(1);
    // Exactly one of them names a file that is not on disk.
    const gone = runs.filter((entry) => runHasLog(entry) && !runLogPresent(entry));
    expect(gone.map((entry) => entry.runId)).toEqual(["c711"]);
    expect(runs.filter(runLogPresent)).toHaveLength(runs.length - 1);
  });

  /// The `off` terminal's records name no file, which is not the same as one
  /// whose file is gone: the row says "未落盘", not "已被清理" (§1.2).
  it("reads a logging-off terminal's runs as having nothing to open", () => {
    const runs = previewRuns(fixture("pwsh"));

    expect(runs.length).toBeGreaterThan(0);
    expect(runs.every((entry) => !runHasLog(entry))).toBe(true);
    expect(runs.every((entry) => runFilePathNote(entry) === "未落盘")).toBe(true);
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
