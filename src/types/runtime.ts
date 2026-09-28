/**
 * Frontend mirror of the backend runtime DTOs
 * (src-tauri/src/session/runtime.rs and src-tauri/src/session/event.rs).
 *
 * The authoritative definitions live in Rust; these TypeScript types plus
 * their runtime guards are the typed frontend side of the contract
 * (MVP_IMPLEMENTATION_SPEC.md §4/§9). T06 (#7) fixtures are shaped exactly
 * like these snapshots so T07–T10 can swap fixtures for `list_sessions`
 * payloads without re-mapping.
 *
 * Wire note: `SessionRuntime` crosses the IPC boundary as-is, and its nested
 * logging block is the config-layer `EffectiveLogging` — which serializes
 * `external_path` in snake_case, unlike the config DTO's `externalPath`.
 * The mirror below is deliberately faithful to the actual wire shape; if the
 * backend unifies the casing (a one-line serde change), update this mirror
 * together with it.
 *
 * ## `null` is a value here, and the types say so
 *
 * The two halves of the backend spell "nothing here" differently and neither is
 * wrong: the config DTOs skip absent fields (`skip_serializing_if`), while the
 * domain structs — a snapshot, a run record — carry `Option` fields serde
 * serializes as `null` by default. Both spellings arrive, so every optional
 * field below is typed `| null` as well as optional, and read sites must
 * compare with `!= null` rather than `!== undefined`. Typing them honestly is
 * what makes the compiler catch the difference: `record.endedAt === undefined`
 * is true for nothing when the wire sent `null`, and a live run would be
 * rendered as a failed one.
 */

import { LOG_MODES, LOG_SOURCES, type EffectiveLogModeValue, type LogSourceValue } from "./config";

/**
 * The event names Session Core publishes (`src-tauri/src/session/event.rs`,
 * spec §9). Literals, not derived names: a rename on the backend has to break
 * the listener that reads it rather than silently stop firing it.
 */
export const SESSION_STATE_CHANGED = "session-state-changed";
export const RUN_RECORD_UPDATED = "run-record-updated";
export const APP_SUMMARY_CHANGED = "app-summary-changed";

/** Lifecycle states, serialized snake_case by `SessionStatus`. */
export type SessionStatusValue =
  "stopped" | "starting" | "running" | "stopping" | "exited" | "error";

/** Why a session command was refused (`SessionErrorKind`). */
export type SessionErrorKindValue =
  "unknown_session" | "already_registered" | "invalid_transition" | "unsupported" | "failed";

const SESSION_STATUSES: readonly SessionStatusValue[] = [
  "stopped",
  "starting",
  "running",
  "stopping",
  "exited",
  "error",
];

/** Effective logging block as it appears inside a runtime snapshot. */
export interface RuntimeEffectiveLoggingDto {
  mode: EffectiveLogModeValue;
  source: LogSourceValue;
  /** Application-owned log path; snake_case on this nested block (see the
   * file note above — config DTOs use `externalPath`, runtime embeds the
   * config-layer struct verbatim). */
  external_path?: string | null;
}

/** Bounded in-memory scrollback summary; the content is read on demand. */
export interface BufferSummaryDto {
  bytes: number;
  lines: number;
  droppedBytes: number;
}

/** Structured failure the UI can show without parsing strings. */
export interface SessionErrorInfoDto {
  operation: string;
  message: string;
}

/**
 * Why a command was refused or failed, as `SessionError` crosses the wire
 * (MVP_IMPLEMENTATION_SPEC.md §9; `src-tauri/src/session/core.rs`).
 *
 * Distinct from the snapshot's `lastError`: that is a note the session carries
 * about a run, this is the answer to a command the user just asked for. Every
 * command that can refuse returns it, so the UI has one shape to render and
 * never has to parse a transport error.
 */
export interface SessionErrorDto {
  kind: SessionErrorKindValue;
  sessionId: string;
  operation: string;
  message: string;
  /** The lifecycle state the session was in when the move was refused. */
  from?: SessionStatusValue | null;
}

const SESSION_ERROR_KINDS: readonly SessionErrorKindValue[] = [
  "unknown_session",
  "already_registered",
  "invalid_transition",
  "unsupported",
  "failed",
];

/** Runtime guard for a structured command refusal. */
export function isSessionErrorDto(value: unknown): value is SessionErrorDto {
  if (!isObject(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  if (
    !SESSION_ERROR_KINDS.includes(candidate.kind as SessionErrorKindValue) ||
    typeof candidate.sessionId !== "string" ||
    typeof candidate.operation !== "string" ||
    typeof candidate.message !== "string"
  ) {
    return false;
  }
  const from = candidate.from;
  return (
    from === undefined || from === null || SESSION_STATUSES.includes(from as SessionStatusValue)
  );
}

/** Everything the UI needs to render one session right now. */
export interface SessionRuntimeDto {
  sessionId: string;
  status: SessionStatusValue;
  /** Set while a run is starting or running. */
  pid?: number | null;
  /** The run this snapshot describes; absent before the first start. */
  runId?: string | null;
  /** RFC 3339 UTC timestamp of the current run's start. */
  startedAt?: string | null;
  /** Known once a run has ended, including an unexpected exit. */
  exitCode?: number | null;
  ptyAttached: boolean;
  logging: RuntimeEffectiveLoggingDto;
  buffer: BufferSummaryDto;
  lastError?: SessionErrorInfoDto | null;
}

/** App-wide session counts for the window chrome and tray summary. */
export interface AppSummaryDto {
  total: number;
  running: number;
  error: number;
}

/** One managed start, live (`endedAt` absent) or finished. */
export interface RunRecordDto {
  runId: string;
  sessionId: string;
  startedAt: string;
  endedAt?: string | null;
  exitCode?: number | null;
  pid?: number | null;
  logMode: EffectiveLogModeValue;
  logSource: LogSourceValue;
  logFile?: string | null;
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

/**
 * `null` counts as absent here, not as a wrong type.
 *
 * The two halves of the backend answer differently and neither is wrong: the
 * config DTOs skip absent fields (`skip_serializing_if`), while the domain
 * types embedded in a runtime snapshot — `pid`, `runId`, `endedAt`, `logFile`,
 * `external_path` — serialize them as `null`, which is serde's default for an
 * `Option`. A guard that accepted only `undefined` would reject every real
 * record the moment one of those fields was empty, so "missing" covers both
 * spellings.
 */
function optionalInteger(source: Record<string, unknown>, key: string): boolean {
  const value = source[key];
  if (value === undefined || value === null) {
    return true;
  }
  return typeof value === "number" && Number.isInteger(value);
}

function optionalString(source: Record<string, unknown>, key: string): boolean {
  const value = source[key];
  if (value === undefined || value === null) {
    return true;
  }
  return typeof value === "string";
}

/** Runtime guard for the nested effective-logging block. */
export function isRuntimeEffectiveLoggingDto(value: unknown): value is RuntimeEffectiveLoggingDto {
  if (!isObject(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.mode === "string" &&
    LOG_MODES.includes(candidate.mode as EffectiveLogModeValue) &&
    typeof candidate.source === "string" &&
    LOG_SOURCES.includes(candidate.source as LogSourceValue) &&
    optionalString(candidate, "external_path")
  );
}

/** Runtime guard for the scrollback summary. */
export function isBufferSummaryDto(value: unknown): value is BufferSummaryDto {
  if (!isObject(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.bytes === "number" &&
    Number.isInteger(candidate.bytes) &&
    candidate.bytes >= 0 &&
    typeof candidate.lines === "number" &&
    Number.isInteger(candidate.lines) &&
    candidate.lines >= 0 &&
    typeof candidate.droppedBytes === "number" &&
    Number.isInteger(candidate.droppedBytes) &&
    candidate.droppedBytes >= 0
  );
}

/** Runtime guard for values shaped like a `SessionRuntime` snapshot. */
export function isSessionRuntimeDto(value: unknown): value is SessionRuntimeDto {
  if (!isObject(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.sessionId === "string" &&
    typeof candidate.status === "string" &&
    SESSION_STATUSES.includes(candidate.status as SessionStatusValue) &&
    optionalInteger(candidate, "pid") &&
    optionalString(candidate, "runId") &&
    optionalString(candidate, "startedAt") &&
    optionalInteger(candidate, "exitCode") &&
    typeof candidate.ptyAttached === "boolean" &&
    isRuntimeEffectiveLoggingDto(candidate.logging) &&
    isBufferSummaryDto(candidate.buffer) &&
    isSessionErrorInfoDtoOrAbsent(candidate.lastError)
  );
}

function isSessionErrorInfoDtoOrAbsent(value: unknown): boolean {
  if (value === undefined) {
    return true;
  }
  if (!isObject(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return typeof candidate.operation === "string" && typeof candidate.message === "string";
}

/** Runtime guard for the app-wide summary. */
export function isAppSummaryDto(value: unknown): value is AppSummaryDto {
  if (!isObject(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.total === "number" &&
    Number.isInteger(candidate.total) &&
    typeof candidate.running === "number" &&
    Number.isInteger(candidate.running) &&
    typeof candidate.error === "number" &&
    Number.isInteger(candidate.error)
  );
}

/** Runtime guard for one run record. */
export function isRunRecordDto(value: unknown): value is RunRecordDto {
  if (!isObject(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.runId === "string" &&
    typeof candidate.sessionId === "string" &&
    typeof candidate.startedAt === "string" &&
    optionalString(candidate, "endedAt") &&
    optionalInteger(candidate, "exitCode") &&
    optionalInteger(candidate, "pid") &&
    typeof candidate.logMode === "string" &&
    LOG_MODES.includes(candidate.logMode as EffectiveLogModeValue) &&
    typeof candidate.logSource === "string" &&
    LOG_SOURCES.includes(candidate.logSource as LogSourceValue) &&
    optionalString(candidate, "logFile")
  );
}
