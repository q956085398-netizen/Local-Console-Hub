/**
 * Frontend mirror of the logging DTOs (src-tauri/src/logging/ and
 * src-tauri/src/ipc/logs.rs).
 *
 * The authoritative definitions live in Rust; these TypeScript types plus
 * their runtime guards are the typed frontend side of the contract
 * (MVP_IMPLEMENTATION_SPEC.md §9) and are what the Logs tab renders from.
 *
 * ## Null is the wire's word for "absent"
 *
 * The logging structs carry `Option` fields without `skip_serializing_if`
 * (`LogStatus::log_file`, a run record's `endedAt`), so an absent value arrives
 * as `null`, not as a missing key. The guards below accept both spellings of
 * "nothing here", the types admit both, and a reader compares with `!= null`
 * — see the note in `types/runtime.ts`.
 *
 * ## What these types are not
 *
 * There is no `logEntries` list anywhere, and deliberately so: the Hub does
 * not read log *content* back (`docs/LOGGING.md` §10 hands the file to the
 * OS instead), and it does not index it. A view that listed lines would be a
 * second, worse reader of a file the user can open.
 */

import { LOG_MODES, LOG_SOURCES, type EffectiveLogModeValue, type LogSourceValue } from "./config";
import {
  isBufferSummaryDto,
  isRunRecordDto,
  type BufferSummaryDto,
  type RunRecordDto,
} from "./runtime";

/** Effective persistence state of a session (`LOGGING.md` §1.4). */
export type LogStateValue = "off" | "capturing" | "external" | "on_error";

const LOG_STATES: readonly LogStateValue[] = ["off", "capturing", "external", "on_error"];

/** A logging failure: which operation, on which file, and what to do. */
export interface LogErrorDto {
  operation: string;
  message: string;
  /** The file the operation was about; absent when none was involved. */
  path?: string | null;
}

/**
 * The complete "is this being logged?" answer for one session
 * (`LOGGING.md` §1.4) — the payload of `get_log_info`.
 */
export interface LogStatusDto {
  sessionId: string;
  /** Effective mode from the config (`auto` already resolved away). */
  mode: EffectiveLogModeValue;
  source: LogSourceValue;
  /** What is happening right now: the one field that changes while a run runs. */
  state: LogStateValue;
  /** The file the Hub writes for the current run, if it is writing one. */
  logFile?: string | null;
  /**
   * Whether the file this card names for the current run is on disk right now
   * (`docs/DECISIONS.md` D-022).
   *
   * The same question the run history answers per entry, asked of the file the
   * card's actions act on — which for an `external` session is
   * `externalLog`, the application's own (`D-005`), not a Hub-written file it
   * does not have. Read from the filesystem when the status is read and never
   * stored, so a log removed by hand is answered exactly like one retention
   * swept. `false` when the session names no file at all, which is why the
   * reading rule is "a path **and** this".
   */
  logFilePresent: boolean;
  /** The application-owned log this session is linked to (`source: external`). */
  externalLog?: string | null;
  /** Where this session's Hub-written logs live, so the folder can be found
   * without the Hub (D-011). */
  sessionLogDir?: string | null;
  /** Always false in the MVP: stdin is never persisted (`LOGGING.md` §4). The
   * field exists so the UI can state it rather than imply it. */
  recordsInput: boolean;
  /** The in-memory scrollback this session currently holds. */
  buffer: BufferSummaryDto;
  /** The current run's file hit its size cap and stopped taking output. */
  truncated: boolean;
  /** Why logging is not working as configured, if it is not. */
  lastError?: LogErrorDto | null;
}

/**
 * One entry of the run history: the run, and whether its log is still on disk
 * (`src-tauri/src/logging/metadata.rs`).
 *
 * The backend flattens the record and adds one key rather than nesting the
 * record under a new one, so every field below keeps the name and place it has
 * in a run record everywhere else (`types/runtime.ts`).
 */
export interface RunHistoryEntryDto extends RunRecordDto {
  /**
   * Whether the file `logFile` names is on disk right now.
   *
   * Retention deletes log files and never the record of the run that wrote
   * them (`LOGGING.md` §9), so a row can outlive the file it points at. This is
   * how it says so, instead of offering an action that fails when it is
   * clicked. `false` for a run that never wrote a log at all, which is why the
   * reading rule is "`logFile` names a file **and** this is true".
   */
  logFilePresent: boolean;
}

/** A session's run history as it exists on disk (`LOGGING.md` §6). */
export interface RunHistoryDto {
  /** Completed runs, most recent start first. */
  runs: RunHistoryEntryDto[];
  /** Entries that exist but could not be read: the list is incomplete, and the
   * view has to be able to say so rather than present it as the whole truth. */
  unreadable: LogErrorDto[];
}

/** What a cleanup did, or would do (`LOGGING.md` §9). */
export interface CleanupReportDto {
  /** Files that were (or would be) removed. */
  removed: string[];
  freedBytes: number;
  /** Files the OS refused to remove. */
  failures: LogErrorDto[];
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

/** `undefined` and `null` both mean "the backend sent nothing here". */
function absent(value: unknown): boolean {
  return value === undefined || value === null;
}

function optionalString(source: Record<string, unknown>, key: string): boolean {
  return absent(source[key]) || typeof source[key] === "string";
}

/** Runtime guard for a logging failure. */
export function isLogErrorDto(value: unknown): value is LogErrorDto {
  if (!isObject(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.operation === "string" &&
    typeof candidate.message === "string" &&
    optionalString(candidate, "path")
  );
}

/** Runtime guard for `get_log_info`'s payload. */
export function isLogStatusDto(value: unknown): value is LogStatusDto {
  if (!isObject(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.sessionId === "string" &&
    LOG_MODES.includes(candidate.mode as EffectiveLogModeValue) &&
    LOG_SOURCES.includes(candidate.source as LogSourceValue) &&
    LOG_STATES.includes(candidate.state as LogStateValue) &&
    optionalString(candidate, "logFile") &&
    typeof candidate.logFilePresent === "boolean" &&
    optionalString(candidate, "externalLog") &&
    optionalString(candidate, "sessionLogDir") &&
    typeof candidate.recordsInput === "boolean" &&
    isBufferSummaryDto(candidate.buffer) &&
    typeof candidate.truncated === "boolean" &&
    (absent(candidate.lastError) || isLogErrorDto(candidate.lastError))
  );
}

/**
 * Runtime guard for one run-history entry.
 *
 * The record guard answers for everything the entry inherits; the single field
 * it adds is the one that has to be checked here. A payload that carried the
 * record without the file answer is rejected rather than rendered as a row
 * whose file actions are decided by a guess.
 */
export function isRunHistoryEntryDto(value: unknown): value is RunHistoryEntryDto {
  if (!isRunRecordDto(value)) {
    return false;
  }
  const candidate: Partial<RunHistoryEntryDto> = value;
  return typeof candidate.logFilePresent === "boolean";
}

/** Runtime guard for `get_run_history`'s payload. */
export function isRunHistoryDto(value: unknown): value is RunHistoryDto {
  if (!isObject(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    Array.isArray(candidate.runs) &&
    candidate.runs.every(isRunHistoryEntryDto) &&
    Array.isArray(candidate.unreadable) &&
    candidate.unreadable.every(isLogErrorDto)
  );
}

/** Runtime guard for a `preview_log_cleanup` / `cleanup_logs` payload. */
export function isCleanupReportDto(value: unknown): value is CleanupReportDto {
  if (!isObject(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    Array.isArray(candidate.removed) &&
    candidate.removed.every((path) => typeof path === "string") &&
    typeof candidate.freedBytes === "number" &&
    Number.isInteger(candidate.freedBytes) &&
    candidate.freedBytes >= 0 &&
    Array.isArray(candidate.failures) &&
    candidate.failures.every(isLogErrorDto)
  );
}
