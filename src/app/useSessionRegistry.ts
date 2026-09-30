import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { ConfigReportDto } from "../types/config";
import {
  SESSION_CREATED,
  SESSION_REMOVED,
  SESSION_STATE_CHANGED,
  isCreatedSessionDto,
  sessionErrorMessage,
} from "../types/runtime";
import { FIXTURE_SESSIONS } from "../state/fixtures";
import type { SessionView } from "../state/session-view";
import type { BackendConnection } from "../state/backend-connection";
import {
  watchSessionRegistry,
  type SessionRegistryController,
  type SessionRegistrySnapshot,
} from "../state/session-registry";
import {
  installVerificationReload,
  readVerificationSnapshot,
} from "./session-initialization-verification";

/** The workspace the shell renders, and the actions its controls call. */
export interface SessionRegistry {
  /** The sessions to render, in the registry's order. */
  sessions: SessionView[];
  /** The list source, including the period before the first live snapshot. */
  source: SessionSource;
  /** The backend is connected, but its session list has not loaded yet. */
  loading: boolean;
  /** Whether a backend is answering; this is separate from session lifecycle. */
  live: boolean;
  /** The most recent failed action, for the status bar. */
  error: string | null;
  /** A failed session initialization; the registry retries with capped delay. */
  initializationError: string | null;
  /** Startup config report, including file-level and per-entry problems. */
  configReport: ConfigReportDto | null;
  /** Failure to retrieve the report IPC payload itself. */
  configReportError: string | null;
  start(sessionId: string): void;
  stop(sessionId: string): void;
  restart(sessionId: string): void;
  forceStop(sessionId: string): void;
  /**
   * Hand this session's configured URL or working directory to the OS (T08
   * #9). A session id, never a URL or path, keeps resolution inside Session
   * Core (`docs/DECISIONS.md` D-021).
   */
  openUrl(sessionId: string): void;
  openDirectory(sessionId: string): void;
  /**
   * Create a temporary terminal (#62) — the "新建 PowerShell" entry.
   *
   * Answers with the new session's id once the backend has created, started
   * and announced it, or `null` when it could not: the failure's message
   * lands in `error`, which the status bar shows, because a shell that is not
   * on the machine is the user's to fix (H06).
   */
  createTerminal(cwd?: string): Promise<string | null>;
  /** Remove a temporary session that has ended (#62). */
  removeSession(sessionId: string): void;
}

/** Where the rendered sessions came from. */
export type SessionSource = "preview" | "loading" | "backend";

const EMPTY_LIVE_SNAPSHOT: SessionRegistrySnapshot = {
  phase: "loading",
  sessions: [],
  error: null,
  sessionRevision: 0,
  configReport: null,
  configReportError: null,
};
const EMPTY_SESSIONS: SessionView[] = [];

/**
 * Session Core remains the lifecycle source of truth. The registry
 * coordinator subscribes before reading `list_session_configs` and
 * `list_sessions`, then follows full state events. It reconciles those events
 * with the initial snapshot and only renders rows with validated config.
 *
 * The hook supplies typed Tauri operations to that injected backend boundary.
 * It renders an empty loading state until the first live snapshot is ready;
 * preview fixtures are used only when no backend has answered.
 */
export function useSessionRegistry(connection: BackendConnection): SessionRegistry {
  const live = connection.state === "connected";
  const [registryForConnection, setRegistryForConnection] = useState<{
    connection: BackendConnection;
    snapshot: SessionRegistrySnapshot;
  } | null>(null);
  const [error, setError] = useState<string | null>(null);
  /**
   * The running watch, for the two commands whose answers the event stream may
   * not have delivered yet (`createTerminal`, `removeSession`).
   */
  const controller = useRef<SessionRegistryController | null>(null);

  useEffect(installVerificationReload, []);

  useEffect(() => {
    if (!live) {
      controller.current = null;
      return;
    }
    const activeConnection = connection;
    let lastSessionRevision = 0;
    const watching = watchSessionRegistry(
      {
        // Three event names, one listener: a session's state, its arrival and
        // its departure are all "what the registry looks like now" (#62), and
        // a second listener keyed to the same lifecycle would only be a second
        // chance to get the ordering wrong.
        subscribe: (receive) =>
          Promise.all([
            listen<unknown>(SESSION_STATE_CHANGED, (event) => receive(event.payload)),
            listen<unknown>(SESSION_CREATED, (event) => receive(event.payload)),
            listen<unknown>(SESSION_REMOVED, (event) => receive(event.payload)),
          ]).then((unlisten) => () => {
            for (const stop of unlisten) stop();
          }),
        listConfigs: () =>
          readVerificationSnapshot("configs", () => invoke<unknown>("list_session_configs")),
        listSessions: () =>
          readVerificationSnapshot("runtimes", () => invoke<unknown>("list_sessions")),
        getConfigReport: () => invoke<unknown>("get_config_report"),
      },
      (snapshot) => {
        setRegistryForConnection({ connection: activeConnection, snapshot });
        if (snapshot.sessionRevision > lastSessionRevision) {
          lastSessionRevision = snapshot.sessionRevision;
          setError(null);
        }
      },
    );
    controller.current = watching;
    return () => {
      controller.current = null;
      watching.stop();
    };
  }, [connection, live]);

  // A new connection starts empty immediately, without briefly showing fixture
  // rows or a snapshot from an earlier connection. If the transport later
  // goes away, keep its last known session snapshot; transport availability
  // does not rewrite lifecycle truth.
  const snapshot = live
    ? registryForConnection?.connection === connection
      ? registryForConnection.snapshot
      : EMPTY_LIVE_SNAPSHOT
    : registryForConnection?.snapshot.phase === "ready"
      ? registryForConnection.snapshot
      : null;
  const source: SessionSource =
    snapshot === null ? "preview" : snapshot.phase === "ready" ? "backend" : "loading";
  const sessions: SessionView[] =
    source === "preview" ? FIXTURE_SESSIONS : (snapshot?.sessions ?? EMPTY_SESSIONS);
  const loading = source === "loading";
  const initializationError = loading ? (snapshot?.error ?? null) : null;
  const configReport = snapshot?.configReport ?? null;
  const configReportError = snapshot?.configReportError ?? null;

  const run = useCallback((command: string, sessionId: string) => {
    // The result is deliberately ignored: a lifecycle command answers with
    // the post-operation snapshot, and the event that follows publishes the
    // same state to everyone (the window, the tray, a future scheduler).
    // Rendering from the event alone is what keeps them from disagreeing. The
    // two "open" actions return no state, so they share this path and its
    // error handling instead of growing another one.
    invoke(command, { sessionId }).catch((cause) => setError(sessionErrorMessage(cause)));
  }, []);

  /**
   * The one command whose answer the window *does* read (#62).
   *
   * Everything else renders from the event, because the event is the same
   * fact published to everyone. A creation is different in one respect: the
   * caller needs the new session's id to select it, and only the answer has
   * it — the event carries the configuration but no notion of "this is the one
   * you asked for". The pair is adopted into the watch as well, which is what
   * makes the row exist even if the event is still in flight.
   */
  const createTerminal = useCallback(async (cwd?: string): Promise<string | null> => {
    try {
      const raw = await invoke<unknown>("create_temporary_terminal", { cwd });
      if (!isCreatedSessionDto(raw)) {
        throw new Error("create_temporary_terminal 返回了无法识别的载荷");
      }
      controller.current?.adopt(raw);
      return raw.config.id;
    } catch (cause) {
      // A shell or directory that is not there is a real answer, not a
      // silent failure: it is the message the user needs (H06).
      setError(sessionErrorMessage(cause));
      return null;
    }
  }, []);

  const removeSession = useCallback((sessionId: string) => {
    invoke("remove_session", { sessionId })
      .then(() => controller.current?.forget(sessionId))
      .catch((cause) => setError(sessionErrorMessage(cause)));
  }, []);

  return useMemo<SessionRegistry>(
    () => ({
      sessions,
      source,
      loading,
      live,
      error,
      initializationError,
      configReport,
      configReportError,
      start: (sessionId) => run("start_session", sessionId),
      stop: (sessionId) => run("stop_session", sessionId),
      restart: (sessionId) => run("restart_session", sessionId),
      forceStop: (sessionId) => run("force_stop_session", sessionId),
      openUrl: (sessionId) => run("open_session_url", sessionId),
      openDirectory: (sessionId) => run("open_session_cwd", sessionId),
      createTerminal,
      removeSession,
    }),
    [
      sessions,
      source,
      loading,
      live,
      error,
      initializationError,
      configReport,
      configReportError,
      run,
      createTerminal,
      removeSession,
    ],
  );
}
