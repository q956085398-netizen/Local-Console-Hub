import { isConfigReportDto, isSessionConfigDto, type ConfigReportDto } from "../types/config";
import {
  isSessionRuntimeDto,
  isSessionStateChangedDto,
  sessionErrorMessage,
  type SessionRuntimeDto,
} from "../types/runtime";
import { sessionsFromLive, type SessionView } from "./session-view";
import { retryDelayMs } from "./retry-schedule";

/** The Tauri operations needed to establish a consistent live session view. */
export interface SessionRegistryBackend {
  /** Resolve only after the listener is installed. */
  subscribe(receive: (payload: unknown) => void): Promise<() => void>;
  listConfigs(): Promise<unknown>;
  listSessions(): Promise<unknown>;
  getConfigReport(): Promise<unknown>;
}

export interface SessionRegistrySnapshot {
  phase: "loading" | "ready";
  sessions: SessionView[];
  /** The last initialization failure, kept visible while a retry is pending. */
  error: string | null;
  /** Increases only when a live lifecycle event is applied after the snapshot. */
  sessionRevision: number;
  configReport: ConfigReportDto | null;
  configReportError: string | null;
}

/** Limit on unique session events retained before the initial config is known. */
export const MAX_BUFFERED_SESSION_EVENTS = 256;

/**
 * Keep the configured-session view in step with Session Core.
 *
 * The subscription is installed before either list is read. Full state events
 * received while those reads are in flight are retained per session and
 * overlaid on the snapshot, so an older response cannot undo a newer event.
 * Events are never used to invent config rows. Both the event cache and retry
 * cadence are bounded, and stopping the watch invalidates every late result.
 */
export function watchSessionRegistry(
  backend: SessionRegistryBackend,
  report: (snapshot: SessionRegistrySnapshot) => void,
): () => void {
  let stopped = false;
  let generation = 0;
  let retryTimer: ReturnType<typeof setTimeout> | undefined;
  let reportRetryTimer: ReturnType<typeof setTimeout> | undefined;
  let failedAttempts = 0;
  let reportFailedAttempts = 0;
  let phase: SessionRegistrySnapshot["phase"] = "loading";
  let sessions: SessionView[] = [];
  let error: string | null = null;
  let sessionRevision = 0;
  let configReport: ConfigReportDto | null = null;
  let configReportError: string | null = null;
  let configuredIds: Set<string> | null = null;
  let unlisten: (() => void) | undefined;
  const bufferedEvents = new Map<string, SessionRuntimeDto>();
  let bufferedEventsOverflowed = false;

  const publish = () =>
    report({ phase, sessions, error, sessionRevision, configReport, configReportError });

  const remember = (runtime: SessionRuntimeDto) => {
    bufferedEvents.delete(runtime.sessionId);
    bufferedEvents.set(runtime.sessionId, runtime);
    if (bufferedEvents.size > MAX_BUFFERED_SESSION_EVENTS) {
      const oldest = bufferedEvents.keys().next().value;
      if (oldest !== undefined) bufferedEvents.delete(oldest);
      bufferedEventsOverflowed = true;
    }
  };

  const stopListening = (stop: (() => void) | undefined) => {
    if (!stop) return;
    try {
      stop();
    } catch {
      // Listener cleanup must not prevent a failed read from being reported
      // or retried, and late callbacks are independently rejected below.
    }
  };

  const startAttempt = () => {
    if (stopped) return;
    const attempt = ++generation;
    let active = true;
    let attemptUnlisten: (() => void) | undefined;
    const isCurrent = () => !stopped && active && generation === attempt;

    const receive = (payload: unknown) => {
      if (!isCurrent() || !isSessionStateChangedDto(payload)) return;
      const { sessionId, runtime } = payload;
      if (runtime.sessionId !== sessionId) return;

      if (configuredIds === null || phase !== "ready") {
        remember(runtime);
        return;
      }
      if (!configuredIds.has(sessionId)) return;

      const next = replaceRuntime(sessions, sessionId, runtime);
      if (next === sessions) return;
      sessions = next;
      sessionRevision += 1;
      error = null;
      publish();
    };

    void (async () => {
      try {
        attemptUnlisten = await backend.subscribe(receive);
        if (!isCurrent()) {
          stopListening(attemptUnlisten);
          return;
        }
        unlisten = attemptUnlisten;

        const [rawConfigs, rawRuntimes] = await Promise.all([
          backend.listConfigs(),
          backend.listSessions(),
        ]);
        if (!isCurrent()) return;
        if (!Array.isArray(rawConfigs) || !rawConfigs.every(isSessionConfigDto)) {
          throw new Error("list_session_configs 返回了无法识别的载荷");
        }
        if (!Array.isArray(rawRuntimes) || !rawRuntimes.every(isSessionRuntimeDto)) {
          throw new Error("list_sessions 返回了无法识别的载荷");
        }
        if (bufferedEventsOverflowed) {
          throw new Error("初始化期间会话变动过多，正在重新同步");
        }

        configuredIds = new Set(rawConfigs.map((config) => config.id));
        let next = sessionsFromLive(rawConfigs, rawRuntimes);
        for (const [sessionId, runtime] of bufferedEvents) {
          if (configuredIds.has(sessionId)) {
            next = replaceRuntime(next, sessionId, runtime);
          }
        }
        bufferedEvents.clear();
        bufferedEventsOverflowed = false;
        sessions = next;
        phase = "ready";
        error = null;
        failedAttempts = 0;
        publish();
      } catch (cause) {
        if (!isCurrent()) return;
        active = false;
        if (unlisten === attemptUnlisten) unlisten = undefined;
        stopListening(attemptUnlisten);
        configuredIds = null;
        bufferedEvents.clear();
        bufferedEventsOverflowed = false;
        phase = "loading";
        sessions = [];
        error = sessionErrorMessage(cause);
        publish();
        const delay = retryDelayMs(failedAttempts++);
        retryTimer = setTimeout(startAttempt, delay);
      }
    })();
  };

  const readConfigReport = async () => {
    try {
      const rawReport = await backend.getConfigReport();
      if (stopped) return;
      if (!isConfigReportDto(rawReport)) {
        throw new Error("get_config_report 返回了无法识别的载荷");
      }
      configReport = rawReport;
      configReportError = null;
      reportFailedAttempts = 0;
      publish();
    } catch (cause) {
      if (stopped) return;
      configReport = null;
      configReportError = sessionErrorMessage(cause);
      publish();
      const delay = retryDelayMs(reportFailedAttempts++);
      reportRetryTimer = setTimeout(() => void readConfigReport(), delay);
    }
  };

  publish();
  startAttempt();
  void readConfigReport();

  return () => {
    stopped = true;
    generation += 1;
    if (retryTimer !== undefined) clearTimeout(retryTimer);
    if (reportRetryTimer !== undefined) clearTimeout(reportRetryTimer);
    stopListening(unlisten);
    unlisten = undefined;
  };
}

function replaceRuntime(
  sessions: SessionView[],
  sessionId: string,
  runtime: SessionRuntimeDto,
): SessionView[] {
  const index = sessions.findIndex((session) => session.config.id === sessionId);
  if (index < 0) return sessions;
  const next = [...sessions];
  next[index] = { ...next[index], runtime };
  return next;
}
