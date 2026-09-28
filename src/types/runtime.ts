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
 * Wire note 2: a session-layer `Option<T>` is serialized as **`null`**, not as
 * an absent key — those structs have no `skip_serializing_if`, and the Rust
 * tests assert exactly that (`value["pid"].is_null()`). So an optional field
 * here is `T | null`, and a site that reads one asks `isPresent(...)` rather
 * than comparing against `undefined`. The *config* DTOs are the other way
 * round (they do skip absent fields), which is why `types/config.ts` keeps
 * `undefined`-only optionals: the two layers differ, and each mirror says what
 * its own layer does.
 */

import { LOG_MODES, LOG_SOURCES, type EffectiveLogModeValue, type LogSourceValue } from "./config";

/** Lifecycle states, serialized snake_case by `SessionStatus`. */
export type SessionStatusValue =
  "stopped" | "starting" | "running" | "stopping" | "exited" | "error";

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
   * config-layer struct verbatim). `null` when the session has none. */
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

/** Everything the UI needs to render one session right now. */
export interface SessionRuntimeDto {
  sessionId: string;
  status: SessionStatusValue;
  /** Set while a run is starting or running; `null` when it is not. */
  pid?: number | null;
  /** The run this snapshot describes; `null` before the first start. */
  runId?: string | null;
  /** RFC 3339 UTC timestamp of the current run's start; `null` before it. */
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

/** Payload of the `session-state-changed` event (§9). */
export interface SessionStateChangedDto {
  sessionId: string;
  /** The full post-transition snapshot, so a listener never applies a delta. */
  runtime: SessionRuntimeDto;
}

/** Why a lifecycle command was refused or failed (`session::core::SessionErrorKind`). */
export type SessionErrorValue =
  "unknown_session" | "already_registered" | "invalid_transition" | "unsupported" | "failed";

const SESSION_ERROR_KINDS: readonly SessionErrorValue[] = [
  "unknown_session",
  "already_registered",
  "invalid_transition",
  "unsupported",
  "failed",
];

/**
 * A refused or failed command, as every session command reports it.
 *
 * The message is actionable by contract (`docs/DEVELOPMENT.md` §9: it names
 * the operation), so the UI shows it as it arrives rather than re-wording it.
 */
export interface SessionErrorDto {
  kind: SessionErrorValue;
  sessionId: string;
  operation: string;
  message: string;
  /** The state the session was in when the move was refused; `null` when the
   * refusal was not about a state (an unknown session, a bad payload). */
  from?: SessionStatusValue | null;
}

/** One managed start, live (`endedAt` absent) or finished. */
export interface RunRecordDto {
  runId: string;
  sessionId: string;
  startedAt: string;
  /** `null` while the run is live. */
  endedAt?: string | null;
  exitCode?: number | null;
  pid?: number | null;
  logMode: EffectiveLogModeValue;
  logSource: LogSourceValue;
  /** `null` for the modes that only learn their file when the run ends. */
  logFile?: string | null;
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function optionalInteger(source: Record<string, unknown>, key: string): boolean {
  const value = source[key];
  return (
    value === undefined || value === null || (typeof value === "number" && Number.isInteger(value))
  );
}

function optionalString(source: Record<string, unknown>, key: string): boolean {
  const value = source[key];
  return value === undefined || value === null || typeof value === "string";
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
  if (value === undefined || value === null) {
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

/** Runtime guard for a `session-state-changed` payload. */
export function isSessionStateChangedDto(value: unknown): value is SessionStateChangedDto {
  if (!isObject(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return typeof candidate.sessionId === "string" && isSessionRuntimeDto(candidate.runtime);
}

/** Runtime guard for the structured failure a session command reports. */
export function isSessionErrorDto(value: unknown): value is SessionErrorDto {
  if (!isObject(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.kind === "string" &&
    SESSION_ERROR_KINDS.includes(candidate.kind as SessionErrorValue) &&
    typeof candidate.sessionId === "string" &&
    typeof candidate.operation === "string" &&
    typeof candidate.message === "string" &&
    (candidate.from === undefined ||
      candidate.from === null ||
      (typeof candidate.from === "string" &&
        SESSION_STATUSES.includes(candidate.from as SessionStatusValue)))
  );
}

/**
 * Whether a wire optional carries a value.
 *
 * The one way to read an optional on these DTOs: `null` is what the backend
 * writes for "nothing", `undefined` is what a JavaScript object literal may
 * have, and a check for only one of them is a bug that renders `PID null` or
 * `run-undefined` — or throws, where the value is dereferenced.
 */
export function isPresent<T>(value: T | null | undefined): value is T {
  return value !== null && value !== undefined;
}

/**
 * The message to show for a rejected command.
 *
 * A session command's rejection is the structured `SessionErrorDto`, whose
 * message already names the operation and the reason; anything else (a
 * transport failure, a broken payload) falls back to its own text so a real
 * problem is never rendered as an empty notice.
 */
export function sessionErrorMessage(cause: unknown): string {
  if (isSessionErrorDto(cause)) {
    return cause.message;
  }
  if (cause instanceof Error) {
    return cause.message;
  }
  return String(cause);
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
