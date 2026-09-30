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

/** Startup state of the config file, distinct from an empty valid file. */
export type ConfigFileStatusValue = "missing" | "loaded" | "unreadable" | "unavailable";

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
  /**
   * Whether this session was created from the window rather than loaded from
   * the config file (#62). Absent means configured — the backend omits the
   * flag for a session that came from the file, so an older payload and a
   * configured session are the same thing here.
   *
   * It is the one difference a row's controls act on: a temporary terminal can
   * be removed once it has ended, and is never restored after the app exits.
   */
  temporary?: boolean;
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
  fileStatus: ConfigFileStatusValue;
  configPath?: string;
  sessions: SessionConfigDto[];
  errors: SessionConfigErrorDto[];
}

/**
 * The "添加应用" form, as the dialog sends it (src-tauri/src/app/applications.rs).
 *
 * `name`, `cwd` and `command` are the required three; the rest is optional and
 * is validated by the config layer, which is also what decides the defaults
 * when `logging` is absent. Empty optional inputs are omitted rather than sent
 * as empty strings — an empty string is a value the config layer rejects as a
 * typo, and a blank box is not a typo.
 */
export interface NewApplicationFormDto {
  name: string;
  cwd: string;
  command: string;
  purpose?: string;
  closeImpact?: string;
  port?: number;
  url?: string;
  logging?: {
    /** `auto` is accepted and resolved by the config layer. */
    mode?: "off" | "always" | "on_error" | "manual" | "auto";
    source?: LogSourceValue;
    /** Application-owned log file; only with `source: "external"`. */
    path?: string;
  };
}

/**
 * The "保存启动配置" form, as the dialog sends it (#65,
 * src-tauri/src/app/terminals.rs).
 *
 * The launch method is deliberately absent: the shell and the working
 * directory are read from the terminal being saved, so this form carries only
 * the words the Hub cannot know. `name` is required — it is what the row will
 * be called after a restart — and the two optional fields are the free text
 * D-027 gives both session types.
 */
export interface SaveTerminalFormDto {
  name: string;
  purpose?: string;
  closeImpact?: string;
}

/**
 * A save from a form that was refused.
 *
 * Shared by the two entries that save a launch configuration (#64's "添加
 * 应用" and #65's "保存启动配置"), because the backend answers both with one
 * shape (src-tauri/src/app/form.rs) and two declarations of it would be two
 * places to forget one of them. `field` names the form input the message
 * belongs to when one of the config layer's validations refused it, so a
 * dialog can put the sentence beside the offending box instead of only in a
 * banner.
 */
export interface FormErrorDto {
  field?: string;
  message: string;
}

/**
 * The answer to one form save (#64, #65).
 *
 * One type for both ends of the call — the registry hook that performs it and
 * the dialog that renders it — because the success half carries the id the
 * workspace selects and the failure half carries the field the dialog places
 * the message on.
 */
export type FormSaveOutcome =
  { ok: true; sessionId: string } | { ok: false; message: string; field?: string };

/**
 * Runtime guard for a refusal a form's dialog can place.
 *
 * It excludes anything carrying a session-error `kind`, which a save never
 * produces: a `SessionErrorDto` also has a string `message` and no `field`, so
 * without the exclusion a refused lifecycle command would be read as a form
 * refusal and shown without the operation that actually failed.
 */
export function isFormErrorDto(value: unknown): value is FormErrorDto {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.message === "string" &&
    candidate.kind === undefined &&
    (candidate.field === undefined || typeof candidate.field === "string")
  );
}

const SESSION_TYPES: readonly SessionTypeValue[] = ["service", "terminal"];
const CONFIG_FILE_STATUSES: readonly ConfigFileStatusValue[] = [
  "missing",
  "loaded",
  "unreadable",
  "unavailable",
];

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
  if (candidate.temporary !== undefined && typeof candidate.temporary !== "boolean") {
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
  if (
    !CONFIG_FILE_STATUSES.includes(candidate.fileStatus as ConfigFileStatusValue) ||
    !optionalString(candidate, "configPath") ||
    !Array.isArray(candidate.sessions) ||
    !Array.isArray(candidate.errors)
  ) {
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
