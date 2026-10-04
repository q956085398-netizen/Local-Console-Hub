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

import {
  LOG_MODES,
  LOG_SOURCES,
  isSessionConfigDto,
  type EffectiveLogModeValue,
  type LogSourceValue,
  type SessionConfigDto,
} from "./config";

/**
 * The event names Session Core publishes (`src-tauri/src/session/event.rs`,
 * spec §9). Literals, not derived names: a rename on the backend has to break
 * the listener that reads it rather than silently stop firing it.
 */
export const SESSION_STATE_CHANGED = "session-state-changed";
/** A session entered the registry (#62) — a temporary terminal, in practice. */
export const SESSION_CREATED = "session-created";
/** A session's configuration became a saved one (#65) — "保存启动配置". */
export const SESSION_SAVED = "session-saved";
/** A session left the registry (#62). */
export const SESSION_REMOVED = "session-removed";
export const RUN_RECORD_UPDATED = "run-record-updated";
export const APP_SUMMARY_CHANGED = "app-summary-changed";
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

/**
 * What one HTTP GET returned (`health::HttpProbe`).
 *
 * Present only when the session has an http(s) URL and the probe was issued.
 * `ok: false` is that probe failing — a timeout, or a non-2xx status — which
 * is not "nothing is listening" and not "the process belongs to this session".
 */
export interface HttpProbeDto {
  /** True only when the response status was 2xx. */
  ok: boolean;
  /** Status code when a response arrived; null on timeout or transport failure. */
  status?: number | null;
}

/**
 * One reading of a running service's health (`src-tauri/src/health/mod.rs`,
 * spec §12).
 *
 * Facts from the moment of the probe, not a conclusion about the session:
 * whether the run's process was alive, whether the port its config names was
 * accepting connections, and — when an http(s) URL was configured — what one
 * GET of that URL returned. They are separate on purpose — `docs/PRODUCT_SPEC.md`
 * §3 requires the UI to distinguish "the process is alive" from "the service is
 * available", and `docs/DECISIONS.md` D-008 forbids reducing one to the other.
 * The HTTP result does not replace `portOpen`.
 */
export interface ServiceHealthDto {
  processAlive: boolean;
  portOpen: boolean;
  /**
   * The HTTP GET, when the session has an http(s) URL. Null or absent means
   * the probe was not issued.
   */
  http?: HttpProbeDto | null;
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
  /**
   * Whether this run is one the Hub did **not** start — an instance the user
   * was already running, which the Hub associated with this entry (#67).
   *
   * A row that says "停止" for an application the Hub never started is
   * offering something the backend will refuse, so a control that acts on a
   * run has to be able to tell (§59 decision 11).
   */
  external: boolean;
  logging: RuntimeEffectiveLoggingDto;
  buffer: BufferSummaryDto;
  /** The last health reading, for a service that names a port and has a run in
   * flight; `null` when nothing has been probed. `null` is not "the port is
   * closed" — it is "we did not check", and the UI shows nothing for it. */
  health?: ServiceHealthDto | null;
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

/**
 * The payload `session-created` (#62) and `session-saved` (#65) both carry:
 * a session id, and the configuration that session now has.
 *
 * The configuration travels because it is the half of a session a
 * `session-state-changed` payload cannot carry: a listener told about a state
 * for an id it has no name for could not render the row it is about. The
 * runtime is deliberately absent — states arrive as their own events, in the
 * order the backend published them.
 *
 * ## Why one shape and not two
 *
 * The two events mean different things to the *backend* — one announces a
 * session that was not there, the other a configuration that changed, and the
 * protocol names them accordingly. What they ask of a listener is the same
 * fact, though: this is the session with this id, and this is what it is. A
 * reader that applies the configuration it is given (drawing the row when it
 * has none) is right for both, so a second payload type would be a second
 * declaration of one wire shape — and a reader that had to *distinguish* them
 * from the payload alone could not: they are identical by construction.
 */
export interface SessionConfigEventDto {
  sessionId: string;
  config: SessionConfigDto;
}

/** Payload of the `session-removed` event (#62). */
export interface SessionRemovedDto {
  sessionId: string;
}

/** What `create_temporary_terminal` answers with (#62): both halves of the
 * session it made, from the operation that made it. */
export interface CreatedSessionDto {
  config: SessionConfigDto;
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
    typeof candidate.external === "boolean" &&
    isRuntimeEffectiveLoggingDto(candidate.logging) &&
    isBufferSummaryDto(candidate.buffer) &&
    isServiceHealthDtoOrAbsent(candidate.health) &&
    isSessionErrorInfoDtoOrAbsent(candidate.lastError)
  );
}

/** A status code an HTTP probe can report, or the null a timeout serializes. */
function isHttpStatus(value: unknown): boolean {
  return (
    value === undefined ||
    value === null ||
    (typeof value === "number" && Number.isInteger(value) && value >= 100 && value <= 599)
  );
}

/** Runtime guard for one HTTP probe result. */
function isHttpProbeDto(value: unknown): value is HttpProbeDto {
  if (!isObject(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return typeof candidate.ok === "boolean" && isHttpStatus(candidate.status);
}

/** Runtime guard for a health reading. */
export function isServiceHealthDto(value: unknown): value is ServiceHealthDto {
  if (!isObject(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  const http = candidate.http;
  return (
    typeof candidate.processAlive === "boolean" &&
    typeof candidate.portOpen === "boolean" &&
    (http === undefined || http === null || isHttpProbeDto(http))
  );
}

/**
 * A health reading, `null`, or absent.
 *
 * Both spellings, for the reason `isPresent` gives: the backend writes `null`
 * and a fixture or a hand-written payload may leave the key out. A guard that
 * accepted only one would drop a whole snapshot the frontend could have
 * rendered.
 */
function isServiceHealthDtoOrAbsent(value: unknown): boolean {
  return value === undefined || value === null || isServiceHealthDto(value);
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

/**
 * Runtime guard for a configuration-carrying event payload (#62, #65).
 *
 * The id has to be the configuration's own: an event whose `sessionId` and
 * `config.id` disagree describes no session a listener could render.
 */
export function isSessionConfigEventDto(value: unknown): value is SessionConfigEventDto {
  if (!isObject(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.sessionId === "string" &&
    isSessionConfigDto(candidate.config) &&
    candidate.config.id === candidate.sessionId
  );
}

/**
 * Runtime guard for a `session-removed` payload (#62).
 *
 * It says what the payload must *not* carry, which a bare `sessionId` check
 * would not: every session event has a session id, so a guard that accepted
 * any object with one would read the life out of a `session-state-changed`
 * payload that fell through its own check — turning "this session is running"
 * into "this session is gone". A removal is exactly an id and nothing else.
 */
export function isSessionRemovedDto(value: unknown): value is SessionRemovedDto {
  if (!isObject(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.sessionId === "string" &&
    candidate.runtime === undefined &&
    candidate.config === undefined
  );
}

/**
 * Runtime guard for the answer `create_temporary_terminal` gives (#62).
 *
 * Both halves are required: a caller selects the new session by the
 * configuration's id and renders it from the snapshot, so an answer missing
 * either one is not something the window can act on.
 */
export function isCreatedSessionDto(value: unknown): value is CreatedSessionDto {
  if (!isObject(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    isSessionConfigDto(candidate.config) &&
    isSessionRuntimeDto(candidate.runtime) &&
    candidate.runtime.sessionId === candidate.config.id
  );
}

/** What bringing an application's own window forward did (#66). */
export type WindowOutcomeValue = "focused" | "refused" | "no_window";

const WINDOW_OUTCOMES: readonly WindowOutcomeValue[] = ["focused", "refused", "no_window"];

/**
 * The window step of one activation (#66).
 *
 * Present only for entries that keep their own window. `notice` is what the
 * window shows the user, and it is absent exactly when the window did come
 * forward — a notice for the ordinary case would be noise (story 48).
 */
export interface WindowStepDto {
  outcome: WindowOutcomeValue;
  /** The window's caption, when one was found. Never used to *find* it. */
  title?: string;
  pid?: number;
  notice?: string;
}

/**
 * The answer to `activate_session` (#66; #64 gave it its first half).
 *
 * The window reads one thing out of it: whether the application's own window
 * came forward, and what to say when it did not. The runtime half is what the
 * command answered with before the window step existed, and the events that
 * follow carry the same state to everyone.
 */
export interface ActivationOutcomeDto {
  runtime: SessionRuntimeDto;
  started: boolean;
  window?: WindowStepDto;
  /**
   * The question the Hub asked instead of deciding (#67).
   *
   * Present means *nothing happened*: no run was started and no instance was
   * associated. The answer is the user's, through `resolve_session_open`.
   */
  choice?: OpenChoiceDto;
}

/** One instance that might be the application already running (#67). */
export interface ExternalCandidateDto {
  pid: number;
  /**
   * When the process started, as a decimal string.
   *
   * A string because it is a Windows `FILETIME` — far beyond what a JavaScript
   * number holds exactly — and it travels back to the backend as the half of
   * the identity that survives the pid being reused.
   */
  createdAt?: string;
  fileName: string;
  imagePath?: string;
  title?: string;
  /** Whether it has a window the Hub could bring forward. */
  hasWindow: boolean;
  /**
   * Whether associating it is something the Hub can do safely.
   *
   * The one flag the dialog's "associate" control turns on. What makes it false
   * is in `reason`, in the user's words.
   */
  associable: boolean;
  /** Absent when the configuration passes no arguments to compare. */
  argumentsAgree?: boolean;
  /** Why this one is uncertain, for the dialog to show beside it. */
  reason: string;
}

/** The question, and what it is about (#67). */
export interface OpenChoiceDto {
  reason: string;
  candidates: ExternalCandidateDto[];
}

/**
 * What the user answered when the Hub asked (#67).
 *
 * `associate` needs both halves of the identity it was shown — the creation
 * time is the half that survives the pid being reused, which is why it travels
 * back exactly as it arrived rather than being re-derived here.
 */
export interface OpenResolutionDto {
  kind: "associate" | "new";
  pid?: number;
  createdAt?: string;
}

/** The identity of one candidate, as the resolution has to carry it. */
export function candidateResolution(candidate: ExternalCandidateDto): OpenResolutionDto {
  return { kind: "associate", pid: candidate.pid, createdAt: candidate.createdAt };
}

function isExternalCandidateDto(value: unknown): value is ExternalCandidateDto {
  if (!isObject(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.pid === "number" &&
    Number.isInteger(candidate.pid) &&
    optionalString(candidate, "createdAt") &&
    typeof candidate.fileName === "string" &&
    optionalString(candidate, "imagePath") &&
    optionalString(candidate, "title") &&
    typeof candidate.hasWindow === "boolean" &&
    typeof candidate.associable === "boolean" &&
    (candidate.argumentsAgree === undefined || typeof candidate.argumentsAgree === "boolean") &&
    typeof candidate.reason === "string"
  );
}

function isOpenChoiceDto(value: unknown): value is OpenChoiceDto {
  if (!isObject(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.reason === "string" &&
    Array.isArray(candidate.candidates) &&
    candidate.candidates.every(isExternalCandidateDto)
  );
}

/** Runtime guard for the activation answer. */
export function isActivationOutcomeDto(value: unknown): value is ActivationOutcomeDto {
  if (!isObject(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  if (typeof candidate.started !== "boolean" || !isSessionRuntimeDto(candidate.runtime)) {
    return false;
  }
  if (candidate.choice !== undefined && !isOpenChoiceDto(candidate.choice)) {
    return false;
  }
  if (candidate.window === undefined) {
    return true;
  }
  const window = candidate.window;
  if (!isObject(window)) {
    return false;
  }
  return (
    WINDOW_OUTCOMES.includes(window.outcome as WindowOutcomeValue) &&
    optionalString(window, "title") &&
    optionalString(window, "notice") &&
    (window.pid === undefined || typeof window.pid === "number")
  );
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
