/**
 * Frontend mirror of the backend config DTOs (src-tauri/src/config/dto.rs).
 *
 * The authoritative definitions live in Rust; these TypeScript types plus
 * their runtime guards are the typed frontend side of the contract
 * (MVP_IMPLEMENTATION_SPEC.md §4/§13). T04 (Session Core IPC) and T06 (UI
 * shell fixtures) consume these shapes without re-mapping.
 *
 * String *values* deliberately keep the YAML vocabulary — "on_error",
 * "service", "captured" — so config files, backend enums and UI labels
 * share one set of words.
 */

/** Session kind: managed process or interactive shell. */
export type SessionTypeValue = "service" | "terminal";

/** Log mode after `auto` resolution; `auto` never reaches the frontend. */
export type EffectiveLogModeValue = "off" | "always" | "on_error" | "manual";

/** Where persisted log content comes from. */
export type LogSourceValue = "none" | "captured" | "external";

/** Effective logging state of a session (LOGGING.md §1.4: the UI must
 * reveal whether persistence is active and where it writes). */
export interface EffectiveLoggingDto {
  mode: EffectiveLogModeValue;
  source: LogSourceValue;
  /** Application-owned log path; only present for source "external". */
  externalPath?: string;
}

/** One validated session configuration. */
export interface SessionConfigDto {
  id: string;
  name: string;
  sessionType: SessionTypeValue;
  cwd?: string;
  /** Service sessions only. */
  command?: string;
  /** Service sessions only; http(s) URL. */
  url?: string;
  /** Service sessions only; 1-65535. */
  port?: number;
  purpose?: string;
  closeImpact?: string;
  /** Terminal sessions only. */
  shell?: string;
  initialCommand?: string;
  logging: EffectiveLoggingDto;
}

/** One actionable configuration problem; index 0 means file-level. */
export interface SessionConfigErrorDto {
  index: number;
  sessionId?: string;
  field?: string;
  message: string;
}

/** Result of loading the config file: valid sessions plus per-session
 * errors. One broken entry never removes the others. */
export interface ConfigReportDto {
  sessions: SessionConfigDto[];
  errors: SessionConfigErrorDto[];
}

const SESSION_TYPES: readonly SessionTypeValue[] = ["service", "terminal"];

/** Every valid log mode, for runtime guards on both sides of the contract. */
export const LOG_MODES: readonly EffectiveLogModeValue[] = ["off", "always", "on_error", "manual"];

/** Every valid log source, for runtime guards on both sides of the contract. */
export const LOG_SOURCES: readonly LogSourceValue[] = ["none", "captured", "external"];

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function optionalString(source: Record<string, unknown>, key: string): boolean {
  const value = source[key];
  return value === undefined || typeof value === "string";
}

/** Runtime guard for values received from the backend config layer. */
export function isSessionConfigDto(value: unknown): value is SessionConfigDto {
  if (!isObject(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  if (
    typeof candidate.id !== "string" ||
    typeof candidate.name !== "string" ||
    !SESSION_TYPES.includes(candidate.sessionType as SessionTypeValue)
  ) {
    return false;
  }
  for (const key of [
    "cwd",
    "command",
    "url",
    "purpose",
    "closeImpact",
    "shell",
    "initialCommand",
  ]) {
    if (!optionalString(candidate, key)) {
      return false;
    }
  }
  if (candidate.port !== undefined && typeof candidate.port !== "number") {
    return false;
  }
  return isEffectiveLoggingDto(candidate.logging);
}

/** Runtime guard for the effective logging block. */
export function isEffectiveLoggingDto(value: unknown): value is EffectiveLoggingDto {
  if (!isObject(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    LOG_MODES.includes(candidate.mode as EffectiveLogModeValue) &&
    LOG_SOURCES.includes(candidate.source as LogSourceValue) &&
    optionalString(candidate, "externalPath")
  );
}

/** Runtime guard for a whole config report. */
export function isConfigReportDto(value: unknown): value is ConfigReportDto {
  if (!isObject(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  if (!Array.isArray(candidate.sessions) || !Array.isArray(candidate.errors)) {
    return false;
  }
  return (
    candidate.sessions.every(isSessionConfigDto) && candidate.errors.every(isSessionConfigErrorDto)
  );
}

/** Runtime guard for one config error entry. */
export function isSessionConfigErrorDto(value: unknown): value is SessionConfigErrorDto {
  if (!isObject(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.index === "number" &&
    Number.isInteger(candidate.index) &&
    typeof candidate.message === "string" &&
    optionalString(candidate, "sessionId") &&
    optionalString(candidate, "field")
  );
}
