import {
  isConfigReportDto,
  isSessionConfigDto,
  type ConfigReportDto,
  type SessionConfigDto,
} from "../types/config";
import {
  isSessionConfigEventDto,
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
  /**
   * The backend answered with this session's configuration — the pair a
   * creation (#62) and a save (#65) both answer with.
   */
  adopt(session: CreatedSessionDto): void;
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
 *
 * ## A row whose configuration changes (#65)
 *
 * Saving a terminal does not add or remove a row: the session keeps its id, so
 * it keeps its position, its runtime and its scrollback, and what changes is
 * what it *is* — its name, and the fact that the config file describes it now.
 * `session-saved` announces that, and it is applied by the same rule a creation
 * is: the row says this, and a row that is not there yet is drawn. One rule
 * rather than two, because the two events carry the same shape and ask the same
 * thing of a listener (see `SessionConfigEventDto`), and because a save that
 * drew a second row would list one run twice — exactly the contradiction the
 * entry exists to avoid.
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
  /**
   * Configurations announced before the snapshot was ready, by id — a creation
   * and a save are the same fact here, and the newest one is the one a row
   * should be drawn from.
   */
  const bufferedConfigs = new Map<string, SessionConfigDto>();
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

  const rememberConfig = (config: SessionConfigDto) => {
    bufferedRemovals.delete(config.id);
    bufferedConfigs.delete(config.id);
    bufferedConfigs.set(config.id, config);
    if (bufferedConfigs.size > MAX_BUFFERED_SESSION_EVENTS) {
      const oldest = bufferedConfigs.keys().next().value;
      if (oldest !== undefined) bufferedConfigs.delete(oldest);
      bufferOverflowed = true;
    }
  };

  const rememberRemoval = (sessionId: string) => {
    bufferedConfigs.delete(sessionId);
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

  /**
   * Apply a session's configuration to the rendered list, in arrival order.
   *
   * The one rule for both configuration events — #62's creation and #65's save
   * — because they ask the same thing of a listener: the row for this id says
   * this, and a row that is not there yet is drawn. A save therefore corrects
   * the row it is about instead of adding a second one, which is what keeps a
   * running terminal that was saved from being listed twice; and a repeat of a
   * configuration the listener already has publishes nothing, so a command
   * answer and the event carrying the same fact cost one render between them.
   */
  const applyConfig = (config: SessionConfigDto) => {
    if (configuredIds === null) return;
    const index = sessions.findIndex((session) => session.config.id === config.id);
    if (index < 0) {
      configuredIds.add(config.id);
      sessions = [...sessions, viewOf(config)];
    } else if (!sameConfiguration(sessions[index].config, config)) {
      sessions = replaceConfigAt(sessions, index, config);
    } else {
      return;
    }
    sessionRevision += 1;
    publish();
  };

  /** The two facts a caller can learn before the event stream delivers them. */
  const applyAnnounced = (config: SessionConfigDto) => {
    if (ready()) {
      applyConfig(config);
    } else {
      rememberConfig(config);
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
      // A creation and a save are one shape, and one rule: the row says this.
      if (isSessionConfigEventDto(payload)) {
        applyAnnounced(payload.config);
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
        for (const [sessionId, config] of bufferedConfigs) {
          if (!configuredIds.has(sessionId)) {
            next.push(viewOf(config));
            configuredIds.add(sessionId);
          } else {
            // The read was issued before this configuration was announced, so
            // the announcement is the newer fact — including when the id was
            // already in the file, which is what a save looks like here (#65).
            next = replaceConfig(next, config);
          }
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
        bufferedConfigs.clear();
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
        bufferedConfigs.clear();
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
    adopt(session) {
      if (stopped) return;
      applyAnnounced(session.config);
    },
    forget(sessionId) {
      if (stopped) return;
      applyRemoval(sessionId);
    },
  };
}

/**
 * Whether two configurations would describe the same row.
 *
 * Compared as serialized text rather than field by field: both sides are the
 * same DTO of the same backend (`SessionConfigDto`), so the text is an exact
 * description of the row — and a field added to that DTO later cannot be
 * forgotten here, which a hand-written comparison would invite.
 *
 * The one thing this decides is whether to render, so being wrong is cheap in
 * one direction only: two configurations that are really equal but do not
 * serialize identically cost one redundant render, and a row is never left
 * saying something its configuration does not.
 */
function sameConfiguration(a: SessionConfigDto, b: SessionConfigDto): boolean {
  return a === b || JSON.stringify(a) === JSON.stringify(b);
}

/**
 * `sessions` with `config`'s row carrying it, or the same array when no row
 * has that id.
 */
function replaceConfig(sessions: SessionView[], config: SessionConfigDto): SessionView[] {
  const index = sessions.findIndex((session) => session.config.id === config.id);
  return index < 0 ? sessions : replaceConfigAt(sessions, index, config);
}

/**
 * `sessions` with its row at `index` carrying `config`.
 *
 * The row's runtime and runs are kept and its group is recomputed, because the
 * group is derived from the configuration: a row that was filed under
 * `Temporary` and is now saved has moved, and a stale group would leave the
 * saved terminal in the group the reference calls 用完即走的终端.
 */
function replaceConfigAt(
  sessions: SessionView[],
  index: number,
  config: SessionConfigDto,
): SessionView[] {
  const next = [...sessions];
  next[index] = { ...next[index], config, group: groupForConfig(config) };
  return next;
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
