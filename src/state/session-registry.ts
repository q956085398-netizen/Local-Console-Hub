import {
  isConfigReportDto,
  isSessionConfigDto,
  type ConfigReportDto,
  type SessionConfigDto,
} from "../types/config";
import {
  isSessionCreatedDto,
  isSessionRemovedDto,
  isSessionRuntimeDto,
  isSessionStateChangedDto,
  sessionErrorMessage,
  type CreatedSessionDto,
  type SessionRuntimeDto,
} from "../types/runtime";
import { groupForConfig, sessionsFromLive, stoppedRuntime, type SessionView } from "./session-view";
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

/**
 * What a running registry watch offers besides reporting (#62).
 *
 * The two verbs are the ones a caller can know something the event stream has
 * not delivered yet: a command that created a session answered with it, and
 * one that removed a session answered that it was removed. Both are the same
 * facts the `session-created` / `session-removed` events carry, applied
 * idempotently — so an answer that arrives before its event, after it, or
 * without it leaving the view in one state.
 */
export interface SessionRegistryController {
  /** Stop watching. Every late result is invalidated. */
  stop(): void;
  /** The backend answered that this session exists. */
  adopt(created: CreatedSessionDto): void;
  /** The backend answered that this session is gone. */
  forget(sessionId: string): void;
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
 *
 * ## Sessions that appear and disappear (#62)
 *
 * Since the quick entry exists, the registry's *membership* changes while the
 * app is running, and it changes from the backend's side: `session-created`
 * and `session-removed` are how every listener learns about it. A creation
 * carries the configuration, which is the half a state event cannot — without
 * it a listener would have to re-read `list_session_configs` for a change it
 * was just told about, which is the polling this protocol exists to avoid.
 *
 * The same three-way discipline the state events follow applies to membership:
 * applied in arrival order once the snapshot is ready, retained and merged if
 * it arrives while the snapshot is still coming, and dropped for a listener
 * that has already stopped. A removal is applied last, so a session created
 * and removed inside one initialization window does not come back.
 *
 * `adopt`/`forget` are the command-answer half of the same two facts. A row
 * that arrived from an event is left alone by them: the caller's answer is a
 * moment older than the newest event, so it may fill a gap but never overwrite
 * one.
 */
export function watchSessionRegistry(
  backend: SessionRegistryBackend,
  report: (snapshot: SessionRegistrySnapshot) => void,
): SessionRegistryController {
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
  /** Configurations announced before the snapshot was ready, by id. */
  const bufferedCreations = new Map<string, SessionConfigDto>();
  /** Sessions announced as gone before the snapshot was ready. */
  const bufferedRemovals = new Set<string>();
  let bufferOverflowed = false;

  const publish = () =>
    report({ phase, sessions, error, sessionRevision, configReport, configReportError });

  /** Whether the snapshot is settled enough to apply membership changes to. */
  const ready = () => configuredIds !== null && phase === "ready";

  const remember = (runtime: SessionRuntimeDto) => {
    bufferedEvents.delete(runtime.sessionId);
    bufferedEvents.set(runtime.sessionId, runtime);
    if (bufferedEvents.size > MAX_BUFFERED_SESSION_EVENTS) {
      const oldest = bufferedEvents.keys().next().value;
      if (oldest !== undefined) bufferedEvents.delete(oldest);
      bufferOverflowed = true;
    }
  };

  const rememberCreation = (config: SessionConfigDto) => {
    bufferedRemovals.delete(config.id);
    bufferedCreations.delete(config.id);
    bufferedCreations.set(config.id, config);
    if (bufferedCreations.size > MAX_BUFFERED_SESSION_EVENTS) {
      const oldest = bufferedCreations.keys().next().value;
      if (oldest !== undefined) bufferedCreations.delete(oldest);
      bufferOverflowed = true;
    }
  };

  const rememberRemoval = (sessionId: string) => {
    bufferedCreations.delete(sessionId);
    bufferedEvents.delete(sessionId);
    bufferedRemovals.add(sessionId);
    if (bufferedRemovals.size > MAX_BUFFERED_SESSION_EVENTS) {
      const oldest = bufferedRemovals.values().next().value;
      if (oldest !== undefined) bufferedRemovals.delete(oldest);
      bufferOverflowed = true;
    }
  };

  const viewOf = (config: SessionConfigDto, runtime?: SessionRuntimeDto): SessionView => ({
    config,
    runtime: runtime ?? stoppedRuntime(config),
    runs: [],
    group: groupForConfig(config),
  });

  /** Apply a creation to the rendered list, in arrival order. */
  const addSession = (config: SessionConfigDto) => {
    if (configuredIds === null) return;
    // A session already in the list is not news: its event and the command
    // answer that carried it are the same fact, and a configuration does not
    // change under a running app (the file is read at startup, and a new entry
    // is a new id). So a repeat publishes nothing rather than re-rendering a
    // row whose runtime the state events have already moved on from.
    if (sessions.some((session) => session.config.id === config.id)) return;
    configuredIds.add(config.id);
    sessions = [...sessions, viewOf(config)];
    sessionRevision += 1;
    publish();
  };

  /** The two facts a caller can learn before the event stream delivers them. */
  const applyCreation = (config: SessionConfigDto) => {
    if (ready()) {
      addSession(config);
    } else {
      rememberCreation(config);
    }
  };

  const applyRemoval = (sessionId: string) => {
    if (ready()) {
      dropSession(sessionId);
    } else {
      rememberRemoval(sessionId);
    }
  };

  /** Apply a removal to the rendered list. */
  const dropSession = (sessionId: string) => {
    if (configuredIds === null || !configuredIds.has(sessionId)) return;
    configuredIds.delete(sessionId);
    bufferedEvents.delete(sessionId);
    sessions = sessions.filter((session) => session.config.id !== sessionId);
    sessionRevision += 1;
    publish();
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
      if (!isCurrent()) return;

      // Membership first: a creation is a configuration, a removal carries
      // nothing but its id, and both are what let a state event below find a
      // session to belong to.
      if (isSessionStateChangedDto(payload)) {
        const { sessionId, runtime } = payload;
        if (runtime.sessionId !== sessionId) return;
        if (!ready()) {
          remember(runtime);
          return;
        }
        if (!configuredIds!.has(sessionId)) return;

        const next = replaceRuntime(sessions, sessionId, runtime);
        if (next === sessions) return;
        sessions = next;
        sessionRevision += 1;
        error = null;
        publish();
        return;
      }
      if (isSessionCreatedDto(payload)) {
        applyCreation(payload.config);
        return;
      }
      if (isSessionRemovedDto(payload)) {
        applyRemoval(payload.sessionId);
      }
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
        if (bufferOverflowed) {
          throw new Error("初始化期间会话变动过多，正在重新同步");
        }

        configuredIds = new Set(rawConfigs.map((config) => config.id));
        let next = sessionsFromLive(rawConfigs, rawRuntimes);
        // A session created while the lists were in flight is newer than they
        // are: it is merged in whether or not the snapshot happened to include
        // it, and a removal is applied last so it wins over its own creation.
        for (const [sessionId, config] of bufferedCreations) {
          if (!configuredIds.has(sessionId)) {
            next.push(viewOf(config));
          }
          configuredIds.add(sessionId);
        }
        for (const [sessionId, runtime] of bufferedEvents) {
          if (configuredIds.has(sessionId)) {
            next = replaceRuntime(next, sessionId, runtime);
          }
        }
        for (const sessionId of bufferedRemovals) {
          configuredIds.delete(sessionId);
          next = next.filter((session) => session.config.id !== sessionId);
        }
        bufferedEvents.clear();
        bufferedCreations.clear();
        bufferedRemovals.clear();
        bufferOverflowed = false;
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
        bufferedCreations.clear();
        bufferedRemovals.clear();
        bufferOverflowed = false;
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

  return {
    stop() {
      stopped = true;
      generation += 1;
      if (retryTimer !== undefined) clearTimeout(retryTimer);
      if (reportRetryTimer !== undefined) clearTimeout(reportRetryTimer);
      stopListening(unlisten);
      unlisten = undefined;
    },
    adopt(created) {
      if (stopped) return;
      applyCreation(created.config);
    },
    forget(sessionId) {
      if (stopped) return;
      applyRemoval(sessionId);
    },
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
