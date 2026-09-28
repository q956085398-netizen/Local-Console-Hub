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
   * config-layer struct verbatim). */
  external_path?: string;
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
  /** Set while a run is starting or running. */
  pid?: number;
  /** The run this snapshot describes; absent before the first start. */
  runId?: string;
  /** RFC 3339 UTC timestamp of the current run's start. */
  startedAt?: string;
  /** Known once a run has ended, including an unexpected exit. */
  exitCode?: number;
  ptyAttached: boolean;
  logging: RuntimeEffectiveLoggingDto;
  buffer: BufferSummaryDto;
  lastError?: SessionErrorInfoDto;
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
  endedAt?: string;
  exitCode?: number;
  pid?: number;
  logMode: EffectiveLogModeValue;
  logSource: LogSourceValue;
  logFile?: string;
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function optionalInteger(source: Record<string, unknown>, key: string): boolean {
  const value = source[key];
  return value === undefined || (typeof value === "number" && Number.isInteger(value));
}

function optionalString(source: Record<string, unknown>, key: string): boolean {
  const value = source[key];
  return value === undefined || typeof value === "string";
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
