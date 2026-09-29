import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { isSessionConfigDto } from "../types/config";
import {
  isSessionRuntimeDto,
  isSessionStateChangedDto,
  sessionErrorMessage,
  type SessionRuntimeDto,
} from "../types/runtime";
import { FIXTURE_SESSIONS } from "../state/fixtures";
import { sessionsFromLive, type SessionView } from "../state/session-view";
import type { BackendConnection } from "../state/backend-connection";

/** The workspace the shell renders, and the actions its controls call. */
export interface SessionRegistry {
  /** The sessions to render, in the registry's order. */
  sessions: SessionView[];
  /**
   * Where `sessions` came from. `"preview"` is the T06 fixture workspace: no
   * backend is answering yet, or none ever will (the browser).
   */
  source: SessionSource;
  /**
   * Whether a backend is answering at all — a different question from where
   * the list came from, and both are asked. The terminal attaches to whatever
   * session is selected as soon as there is a backend, while the rail only
   * calls itself the backend's once the listing has landed.
   */
  live: boolean;
  /** The most recent failed action, for the status bar. */
  error: string | null;
  start(sessionId: string): void;
  stop(sessionId: string): void;
  restart(sessionId: string): void;
  forceStop(sessionId: string): void;
  /**
   * Hand this session's configured URL — or its working directory — to the OS
   * (T08 #9).
   *
   * A session id, never a URL or a path: Session Core resolves both from the
   * session's own configuration, so the window cannot ask the machine to open
   * something the user did not configure (`docs/DECISIONS.md` D-021).
   */
  openUrl(sessionId: string): void;
  openDirectory(sessionId: string): void;
}

/** Where the rendered sessions came from. */
export type SessionSource = "preview" | "backend";

/**
 * The session workspace, from Session Core when there is one to ask (T07 #8).
 *
 * Session Core is the single source of lifecycle truth (spec §3), so the window
 * does not keep its own copy of it: this reads the registry once
 * (`list_session_configs` + `list_sessions`) and then follows the
 * `session-state-changed` events it publishes, replacing the snapshot for the
 * session each event names. Nothing here decides what a state *is* — an action
 * sends a command and the next event is what the window renders.
 *
 * Before the backend answers — and in the browser preview, where there is none
 * — the workspace is the T06 fixture data. That is a data source, not a second
 * model: `SessionView` is one shape either way, and the shell renders both.
 *
 * Actions on a fixture workspace do nothing: a fixture session has no run to
 * start, and inventing one would be exactly the fake the ticket forbids. The
 * caller surfaces the preview notice instead (`App.tsx`).
 */
export function useSessionRegistry(connection: BackendConnection): SessionRegistry {
  const live = connection.state === "connected";
  const [sessions, setSessions] = useState<SessionView[]>(() => FIXTURE_SESSIONS);
  const [source, setSource] = useState<SessionSource>("preview");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!live) {
      return;
    }
    let cancelled = false;

    const applySnapshot = (runtime: SessionRuntimeDto) => {
      // The session moved, so whatever went wrong with it has been answered by
      // something newer than the message. Leaving it up would have the status
      // bar report a failure the app has already moved past.
      setError(null);
      setSessions((current) => {
        const index = current.findIndex((view) => view.config.id === runtime.sessionId);
        if (index < 0) {
          // An event for a session the configuration list did not carry. It
          // cannot be rendered, and inventing a row for it would show a session
          // with no name or purpose; the next `list_session_configs` is what
          // would explain it.
          return current;
        }
        const next = current.slice();
        next[index] = { ...current[index], runtime };
        return next;
      });
    };

    const subscription = listen<unknown>("session-state-changed", (event) => {
      if (isSessionStateChangedDto(event.payload)) {
        applySnapshot(event.payload.runtime);
      }
    });

    void (async () => {
      try {
        const [configs, runtimes] = await Promise.all([
          invoke<unknown>("list_session_configs"),
          invoke<unknown>("list_sessions"),
        ]);
        if (cancelled) {
          return;
        }
        if (!Array.isArray(configs) || !configs.every(isSessionConfigDto)) {
          throw new Error("list_session_configs 返回了无法识别的载荷");
        }
        if (!Array.isArray(runtimes) || !runtimes.every(isSessionRuntimeDto)) {
          throw new Error("list_sessions 返回了无法识别的载荷");
        }
        setSource("backend");
        setSessions(sessionsFromLive(configs, runtimes));
      } catch (cause) {
        if (!cancelled) {
          setError(sessionErrorMessage(cause));
        }
      }
    })();

    return () => {
      cancelled = true;
      void subscription.then((unlisten) => unlisten());
    };
  }, [live]);

  const run = useCallback((command: string, sessionId: string) => {
    // The result is deliberately ignored: a lifecycle command answers with the
    // post-operation snapshot, and the event that follows publishes the same
    // state to everyone (the window, the tray, a future scheduler). Rendering
    // from the event alone is what keeps them from disagreeing. The two
    // "open" actions (T08 #9) answer with nothing at all — what they produce
    // is outside the Hub — so they share this path and its error handling
    // rather than growing a second one.
    invoke(command, { sessionId }).catch((cause) => setError(sessionErrorMessage(cause)));
  }, []);

  return useMemo<SessionRegistry>(
    () => ({
      sessions,
      source,
      live,
      error,
      start: (sessionId) => run("start_session", sessionId),
      stop: (sessionId) => run("stop_session", sessionId),
      restart: (sessionId) => run("restart_session", sessionId),
      forceStop: (sessionId) => run("force_stop_session", sessionId),
      openUrl: (sessionId) => run("open_session_url", sessionId),
      openDirectory: (sessionId) => run("open_session_cwd", sessionId),
    }),
    [sessions, source, live, error, run],
  );
}
