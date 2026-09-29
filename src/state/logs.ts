/**
 * Pure derivations for the Logs tab (T10 #11).
 *
 * Everything the tab renders by — the effective-state badge, "is this being
 * logged?", which paths to show, what a retention sweep is about to do — is a
 * pure function of the logging DTOs (`src/types/logs.ts`), so the component
 * stays thin and the rules are unit-tested without a DOM. The wording follows
 * `docs/LOGGING.md` §1.4/§9/§10 and the V2 shell's label vocabulary.
 *
 * ## One code path for live and preview data
 *
 * [`previewLogStatus`] turns a T06 fixture into the same DTO the backend
 * answers `get_log_info` with, so the tab never branches on where its data
 * came from. Provenance is displayed (see `useSessionLogs`), not inferred.
 */

import type { EffectiveLogModeValue, LogSourceValue } from "../types/config";
import type { CleanupReportDto, LogStatusDto, RunHistoryEntryDto } from "../types/logs";
import { isPresent, type RunRecordDto } from "../types/runtime";
import type { SessionView } from "./session-view";
import type { StatusTone } from "./derivations";

/** Human label of the effective persistence state (`LOGGING.md` §1.4). */
export function logStateLabel(state: LogStatusDto["state"]): string {
  switch (state) {
    case "off":
      return "Off";
    case "capturing":
      return "Capturing";
    case "external":
      return "External";
    case "on_error":
      return "On error";
  }
}

/** Tone for the state badge (UI_STYLE_GUIDE §10: colour is lifecycle truth).
 *
 * Only "recording right now" earns a colour; the states that mean "nothing is
 * being written" stay neutral, because a green badge next to an idle session
 * is exactly the "you thought it was recording" reading §1.4 forbids.
 */
export function logStateTone(state: LogStatusDto["state"]): StatusTone {
  switch (state) {
    case "capturing":
      return "run";
    case "on_error":
    case "external":
      return "warn";
    case "off":
      return "idle";
  }
}

/**
 * The file a "open this log" action should act on, or `undefined`.
 *
 * Mirrors Session Core's own resolution order: an `external` session's log
 * belongs to the application (D-005), so the current Hub-written file is not
 * offered for it even if a run record happens to name one.
 */
export function currentLogPath(status: LogStatusDto): string | undefined {
  // `?? undefined` is not decoration: the wire says `null` for "no file", and
  // the rest of this module asks the question with `undefined` (see the note in
  // `types/logs.ts`).
  return (status.source === "external" ? status.externalLog : status.logFile) ?? undefined;
}

/**
 * The current run's file, and whether it is on disk (`docs/DECISIONS.md`
 * D-022).
 *
 * The card's half of [`LogFileFacts`]: the path is [`currentLogPath`]'s, the
 * file answer is the status's own — asked of the filesystem when the status
 * was read, so the card and a run row answer one question one way.
 */
export function currentLogFile(status: LogStatusDto): LogFileFacts {
  return { path: currentLogPath(status), present: status.logFilePresent };
}

/** Which of the tab's actions this session's policy actually offers. */
export interface LogActionAvailability {
  /** Commit an `on_error` run's buffer now (`LOGGING.md` §3). */
  saveRunLog: boolean;
  /** Switch a `manual` run's recording, or `null` when the mode is not manual. */
  recording: "start" | "stop" | null;
}

/**
 * Derive the action set from the effective policy, not from what renders.
 *
 * The file actions are deliberately *not* here: they belong to the file
 * ([`fileActions`]), and both the card and a run row read them from it.
 */
export function logActionAvailability(status: LogStatusDto): LogActionAvailability {
  // `manual` reads its own switch state off the state badge: `capturing` means
  // this run is being recorded right now (`docs/LOGGING.md` §3).
  const recording =
    status.mode === "manual" ? (status.state === "capturing" ? "stop" : "start") : null;
  return {
    // Offered whenever the policy *could* produce a file on request. A run
    // that has none yet is the case this exists for, and the backend answers
    // with the reason when there is no run to save — better than a button that
    // silently disappears while the session is starting.
    saveRunLog: status.mode === "on_error" && status.source === "captured",
    recording,
  };
}

/**
 * Whether the source badge adds anything next to the state badge.
 *
 * The `external` state and the `external` source are one fact, and two badges
 * saying it in two vocabularies is the repetition UI_STYLE_GUIDE §13 forbids.
 * Asked of the policy rather than of the rendered labels, so renaming a label
 * cannot quietly change which badges appear.
 */
export function showsSourceBadge(status: LogStatusDto): boolean {
  return !(status.state === "external" && status.source === "external");
}

/**
 * What a slot says about a file that is named and not on disk (`DECISIONS.md`
 * D-022).
 *
 * One sentence, used by the card's path row and by a run row alike, so the two
 * halves of the tab cannot describe the same state in two ways. It states the
 * fact and stops: neither slot can tell a swept log from an `external`
 * application's file that has not been written yet, and naming a cause it
 * never observed is the invention the rest of the tab is built to avoid.
 */
const FILE_NOT_ON_DISK = "日志文件不在磁盘上（运行记录保留）";

/**
 * A named file's path slot: where the file is, or the fact that it is not on
 * disk (`docs/DECISIONS.md` D-022).
 *
 * One expression for both slots — the card's row for the current run and a run
 * row in the history — so the two halves of the tab cannot describe one state
 * in two ways. D-022's rule is why the second says only what it knows: neither
 * slot can tell a swept log from an `external` application's file that has not
 * been written yet, and naming a cause it never observed would be the
 * invention the rest of the tab is built to avoid.
 */
function filePathText(path: string, present: boolean): string {
  return present ? path : `${FILE_NOT_ON_DISK} · ${path}`;
}

/** Path rows for the policy card, in the order a user asks about them. */
export function logPathEntries(status: LogStatusDto): Array<{ label: string; value: string }> {
  const entries: Array<{ label: string; value: string }> = [];
  const current = currentLogFile(status);
  if (current.path !== undefined) {
    entries.push({
      label: status.source === "external" ? "应用日志" : "当前运行",
      // The card's answer to "is the file there?", which is the same answer the
      // run history gives for the row of the run this card is showing.
      value: filePathText(current.path, current.present),
    });
  }
  if (isPresent(status.sessionLogDir)) {
    // An `external` session still has a Hub log folder — the one it does *not*
    // write into. Naming it answers "where would a Hub log go?" without
    // implying one exists there (D-011).
    entries.push({
      label: status.source === "external" ? "Hub 日志目录（本会话空白）" : "日志目录",
      value: status.sessionLogDir,
    });
  }
  return entries;
}

/** One-line answer to "what is the scrollback I am looking at?" (§8). */
export function bufferNote(status: LogStatusDto): string {
  const scope = `内存缓冲 ${formatBytes(status.buffer.bytes)} · ${status.buffer.lines} 行`;
  if (status.buffer.droppedBytes <= 0) {
    return `${scope} · 未写盘`;
  }
  return `${scope} · 更早的 ${formatBytes(status.buffer.droppedBytes)} 已丢弃`;
}

/** `12.4 MiB` / `512 B` — sizes a person reads, not byte counts. */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  const units = ["KiB", "MiB", "GiB", "TiB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${units[unit]}`;
}

/**
 * What a confirmed sweep would do, before it does it.
 *
 * A destructive action describes itself in the same units the result is
 * reported in, and states the one rule that keeps it safe to agree to
 * (`docs/LOGGING.md` §9: the newest run of a session is always kept).
 */
export function cleanupPrompt(report: CleanupReportDto): string {
  if (report.removed.length === 0) {
    return "没有超过保留规则的日志需要清理。";
  }
  // "Only files" is the part a user agreeing to this needs: the run history is
  // not what a sweep takes, and a row whose log was swept stays in the list
  // saying so (`runLogGone`). A confirmation that left that unsaid would make
  // "confirm" sound like it deletes runs.
  return `将删除 ${report.removed.length} 个日志文件 · 释放 ${formatBytes(report.freedBytes)} · 最近一次运行的日志始终保留 · 运行历史记录不会被删除`;
}

/** What a sweep did, as the notice after it ran. */
export function cleanupOutcome(report: CleanupReportDto): string {
  const failed = report.failures.length;
  if (report.removed.length === 0 && failed === 0) {
    return "没有超过保留规则的日志需要清理。";
  }
  const removed = `已删除 ${report.removed.length} 个文件 · 释放 ${formatBytes(report.freedBytes)}`;
  if (failed === 0) {
    return removed;
  }
  // A folder the OS refused to clean is something the user has to know about;
  // reporting "cleaned" over a partial sweep would be a lie of omission.
  return `${removed} · ${failed} 个文件无法删除（${report.failures[0].message}）`;
}

/** Whether a run's row can offer the file actions. */
export function runHasLog(run: RunRecordDto): boolean {
  return isPresent(run.logFile);
}

/**
 * Whether the log a run's record names is still on disk.
 *
 * A run outlives its log: retention deletes log *files* and never the record of
 * the run that wrote them (`docs/LOGGING.md` §9), so a row that offered "open
 * log" on the record's own word would offer an action that fails. A run that
 * never wrote a log answers `false` here too — that is [a different
 * fact](runHasLog) about the same row, and the two are asked separately.
 */
export function runLogPresent(run: RunHistoryEntryDto): boolean {
  return run.logFilePresent;
}

/**
 * Which of one run row's file actions that run actually offers
 * (`docs/LOGGING.md` §9/§10).
 */
export interface RunFileActions {
  /** Hand the log to the OS's default handler. */
  open: boolean;
  /** Put the path on the clipboard. */
  copy: boolean;
  /** Reveal the folder that holds the log. */
  folder: boolean;
}

/**
 * A file a session's actions act on, and whether it is on disk.
 *
 * The one shape both call sites read: a run-history row builds it from
 * `RunHistoryEntryDto` ([`runLogFile`]), the current-run card from
 * `LogStatusDto` ([`currentLogFile`]). The rule below is written once against
 * this, so the card and the rows cannot answer "which file actions does this
 * run offer?" differently — which is what they did while the card asked only
 * whether a path was named.
 */
export interface LogFileFacts {
  /** The path the actions act on, when the run names one. */
  path: string | undefined;
  /** Whether `path` is a file on disk *right now* (D-022: asked, never stored). */
  present: boolean;
}

/**
 * The file actions a run offers, read from its file facts and nothing else.
 *
 * A run that names no file offers nothing, and a run whose file is not on disk
 * keeps only the folder — the directory survives a sweep, while "open log" and
 * "copy path" would act on a file that is gone. Never read from the policy or
 * the state badge: those answer a different question (is this session being
 * persisted?), and inferring one from the other would make an `off` session
 * look like a broken one.
 *
 * The rule lives here rather than in the panel so both the card's actions and
 * a row's are asserted without a DOM.
 */
export function fileActions(file: LogFileFacts): RunFileActions {
  if (!isPresent(file.path)) {
    return { open: false, copy: false, folder: false };
  }
  return { open: file.present, copy: file.present, folder: true };
}

/** A run-history row's file facts. */
export function runLogFile(run: RunHistoryEntryDto): LogFileFacts {
  return { path: run.logFile ?? undefined, present: run.logFilePresent };
}

/**
 * The path slot of a run row: where the log went, or why there is nothing to
 * open there (`docs/LOGGING.md` §9/§10).
 *
 * Three states, one string each — a row with a file, a run that wrote none
 * (§1.2), and a run whose file is not on disk, in the words the card's path row
 * uses for the same state ([`filePathText`]).
 */
export function runFilePathNote(run: RunHistoryEntryDto): string {
  if (!isPresent(run.logFile)) {
    return "未落盘";
  }
  return filePathText(run.logFile, runLogPresent(run));
}

/** Run history, newest first, whatever order the source listed it in. */
export function runsNewestFirst<T extends RunRecordDto>(runs: T[]): T[] {
  return [...runs].sort((left, right) => Date.parse(right.startedAt) - Date.parse(left.startedAt));
}

/**
 * A fixture session's logging status, shaped like `get_log_info`'s answer.
 *
 * This is a *mirror*, not an invention: state comes from the same rule the
 * backend applies to a configured policy (`policy_state`, `docs/LOGGING.md`
 * §3), the paths come from the fixture's own run records, and the buffer is the
 * fixture snapshot's. A session the backend does not know therefore renders
 * through exactly the code path a registered one does — which is what makes
 * the fixtures useful for looking at the tab without a live run.
 */
export function previewLogStatus(session: SessionView): LogStatusDto {
  // The snapshot's block, not the config's: it is the same shape a live
  // runtime reports, which is what the tab renders from.
  const logging = session.runtime.logging;
  // The live run's record when there is one, and otherwise the last run's —
  // which is what a stopped session's `log_file` means in Core ("the file this
  // session is writing now, else the one the last run left behind").
  const currentRun =
    session.runs.find((run) => run.runId === session.runtime.runId) ??
    runsNewestFirst(session.runs)[0];
  // The Hub writes no file for an `external` session (D-005), so its current
  // file is the application's — reported through `externalLog`, never here.
  const hubWrittenFile = logging.source === "external" ? undefined : currentRun?.logFile;
  // The file the card acts on, chosen in [`currentLogPath`]'s order: the Hub's
  // own when there is one, the linked application log otherwise. It is the one
  // the file answer below has to be about.
  const cardFile = hubWrittenFile ?? logging.external_path;
  return {
    sessionId: session.config.id,
    mode: logging.mode,
    source: logging.source,
    state: policyState(logging.source, logging.mode),
    logFile: hubWrittenFile,
    // A fixture run that names a file has one unless the fixture says its file
    // was swept (`SessionRun.logFilePresent`) — the rule `previewRuns` applies
    // to the rows. An `external` session's file belongs to the application, so
    // there is nothing here to ask: a fixture that links one is taken at its
    // word, exactly as the live path offers the file the config names.
    logFilePresent: isPresent(cardFile) && currentRun?.logFilePresent !== false,
    externalLog: logging.external_path,
    sessionLogDir: undefined,
    recordsInput: false,
    buffer: session.runtime.buffer,
    truncated: false,
    lastError: undefined,
  };
}

/**
 * A fixture session's runs, shaped like `get_run_history`'s answer.
 *
 * The same mirror [`previewLogStatus`] is: a fixture record says where a log
 * went, and the file answer comes from the fixture — a run that names a log has
 * one unless the fixture says its file was swept (`SessionRun`). A run that
 * names no log — the `off` terminal's records — answers "nothing to open",
 * exactly as the backend does for one. A preview row is labelled as preview in
 * the tab (`useSessionLogs`), never passed off as a live read.
 */
export function previewRuns(session: SessionView): RunHistoryEntryDto[] {
  return session.runs.map((run) => ({
    ...run,
    logFilePresent: run.logFilePresent ?? runHasLog(run),
  }));
}

/** The state a configured policy implies, for a session with no live run.
 *
 * The backend's `policy_state` (`src-tauri/src/logging/plan.rs`), spelled out
 * here because a fixture has no run log to ask.
 */
function policyState(source: LogSourceValue, mode: EffectiveLogModeValue): LogStatusDto["state"] {
  if (source === "none") {
    return "off";
  }
  if (source === "external") {
    return "external";
  }
  switch (mode) {
    case "off":
      return "off";
    case "always":
      return "capturing";
    case "on_error":
      return "on_error";
    case "manual":
      // `manual` permits recording, it does not start it (§3).
      return "off";
  }
}
