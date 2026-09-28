import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { openTerminalSession, type TerminalSession } from "../state/terminal-attach";
import type { TerminalAttachmentDto } from "../types/terminal";
import { tauriTerminalBackend } from "./tauriTerminalBackend";

/** What the terminal view does with the stream it is given. */
export interface TerminalStreamHandlers {
  /**
   * Called once per attachment, before any live bytes: reset the surface and
   * render the retained scrollback. The chunks are base64, exactly as the
   * backend holds them, so a replay cut mid-character stays intact.
   */
  onAttach(attachment: TerminalAttachmentDto): void;
  /** Live bytes, in stream order, after `onAttach`. */
  onData(bytes: Uint8Array): void;
}

/** The terminal data path one view is using. */
export interface TerminalStream {
  /** Whether this view is attached to a backend session. */
  attached: boolean;
  /** Whether part of the stream was missed — the view says so rather than
   * showing a history with an invisible hole in it. */
  gapped: boolean;
  /** Why the view is not attached, or what last went wrong with it. */
  error: string | null;
  /** Send keystrokes (a paste included). No-op when not attached. */
  send(text: string): void;
  /** Report the view's size in cells. Safe before a session is running: the
   * backend remembers it so the shell starts at the right geometry. */
  resize(cols: number, rows: number): void;
}

/** What the view knows about the attachment it is (or is not) holding. */
interface AttachmentState {
  /** The session and run this state belongs to. */
  key: string;
  attached: boolean;
  gapped: boolean;
  error: string | null;
}

/**
 * Attach one terminal view to a session's stream (T07 #8).
 *
 * This is the React edge of a protocol that lives in
 * `src/state/terminal-attach.ts`: it decides *when* a view exists — one per
 * session and run — and holds the few facts the terminal's chrome renders.
 * Everything else (subscribing before attaching, replaying the scrollback,
 * dropping what the replay covered, settling a resize) belongs to that module,
 * where it is tested without a DOM.
 *
 * The view's position in the stream is deliberately not React state: it moves
 * once per output batch, and re-rendering the shell per batch is the freeze
 * spec §14 rules out. The chrome's facts are derived for the attachment they
 * belong to rather than reset by a new one, so selecting another session can
 * never briefly report the previous session's stream.
 */
export function useTerminalStream(
  sessionId: string | null,
  /**
   * The run the view is showing. A new run is a new stream: the backend bumps
   * the generation on every start, and this view has to attach to it rather
   * than keep rendering the run that ended. `undefined` covers a session that
   * has not run since the app opened.
   */
  runId: string | undefined,
  live: boolean,
  handlers: TerminalStreamHandlers,
): TerminalStream {
  const key = live && sessionId !== null ? `${sessionId}\u0000${runId ?? "never"}` : null;

  const [reported, setReported] = useState<AttachmentState | null>(null);
  const state = reported !== null && reported.key === key ? reported : null;
  const attached = state?.attached ?? false;
  const gapped = state?.gapped ?? false;
  const error = state?.error ?? null;

  // The handlers are inline closures at the call site, so they change identity
  // every render; the ref is how the session reads the current ones without
  // being torn down and rebuilt on every render.
  const handlersRef = useRef(handlers);
  useEffect(() => {
    handlersRef.current = handlers;
  }, [handlers]);

  const sessionRef = useRef<TerminalSession | null>(null);

  useEffect(() => {
    if (key === null || sessionId === null) {
      return;
    }
    const session = openTerminalSession(sessionId, tauriTerminalBackend, {
      onAttach: (attachment) => {
        setReported({ key, attached: true, gapped: false, error: null });
        handlersRef.current.onAttach(attachment);
      },
      onData: (bytes) => handlersRef.current.onData(bytes),
      onGap: () =>
        setReported((previous) =>
          previous !== null && previous.key === key ? { ...previous, gapped: true } : previous,
        ),
      onError: (message) =>
        setReported((previous) =>
          // An error while attached does not detach the view — a refused
          // keystroke or an oversized resize leaves the terminal on screen and
          // explainable.
          previous !== null && previous.key === key
            ? { ...previous, error: message }
            : { key, attached: false, gapped: false, error: message },
        ),
    });
    sessionRef.current = session;
    return () => {
      sessionRef.current = null;
      session.close();
    };
  }, [key, sessionId]);

  const send = useCallback((text: string) => sessionRef.current?.send(text), []);
  const resize = useCallback(
    (cols: number, rows: number) => sessionRef.current?.resize(cols, rows),
    [],
  );

  return useMemo(
    () => ({ attached, gapped, error, send, resize }),
    [attached, gapped, error, send, resize],
  );
}
