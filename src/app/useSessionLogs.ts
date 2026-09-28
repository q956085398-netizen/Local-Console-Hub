/**
 * The Logs tab's data source and actions (T10 #11).
 *
 * Reads one session's logging state and run history from Session Core, and
 * offers the actions `docs/LOGGING.md` §10 puts in the UI: open the log, open
 * its folder, copy its path, clean up old logs — plus the two the logging modes
 * own (§3: save an `on_error` buffer now, switch a `manual` run's recording).
 *
 * ## Live, or honest about not being live
 *
 * A session the backend knows answers `get_log_info` and `get_run_history`, and
 * the tab renders those. A session it does not know — the fixture shell T06
 * shipped, a session whose config the user has not written yet — answers with
 * an error, and the tab falls back to the fixture's own record and says so
 * (`provenance`). What it never does is blend the two: every value on screen
 * comes from one source, and the source is stated.
 *
 * ## Why the actions re-read instead of patching state
 *
 * `save_run_log`, `set_log_recording` and `cleanup_logs` each answer with the
 * post-operation truth. The hook re-reads rather than guessing at the new
 * state: logging truth lives in Core, and a second copy in the frontend is how
 * the two drift.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  isCleanupReportDto,
  isLogStatusDto,
  isRunHistoryDto,
  type CleanupReportDto,
  type LogErrorDto,
  type LogStatusDto,
} from "../types/logs";
import {
  isSessionErrorDto,
  RUN_RECORD_UPDATED,
  SESSION_STATE_CHANGED,
  type RunRecordDto,
} from "../types/runtime";
import type { FixtureSession } from "../state/fixtures";
import { cleanupOutcome, previewLogStatus } from "../state/logs";

/** Where the tab's values came from: Session Core, or the fixture record. */
export type LogsProvenance = "live" | "preview";

/** One session's logging, as the tab renders it. */
export interface SessionLogs {
  provenance: LogsProvenance;
  status: LogStatusDto;
  runs: RunRecordDto[];
  /** Run records that could not be read, so the list can say it is incomplete. */
  unreadable: LogErrorDto[];
  loading: boolean;
  actions: LogActions;
  cleanup: CleanupState;
}

/** The actions the tab can take, each keyed to the run it acts on. */
export interface LogActions {
  /** Open a run's log with its default handler (`runId` absent = current). */
  openFile: (runId?: string) => void;
  /** Open the folder that holds a run's log. */
  openFolder: (runId?: string) => void;
  /** Put a path on the clipboard. */
  copyPath: (path: string) => void;
  /** Commit an `on_error` run's buffer now. */
  saveRunLog: () => void;
  /** Start or stop recording a `manual` run. */
  setRecording: (recording: boolean) => void;
}

/** The confirm-then-sweep flow retention needs (`docs/LOGGING.md` §9). */
export interface CleanupState {
  /** What a sweep would remove, once asked. `null` when nothing is pending. */
  plan: CleanupReportDto | null;
  /** Whether a preview or a sweep is in flight. */
  busy: boolean;
  /** Ask what a sweep would remove. */
  preview: () => void;
  /** Run the sweep the user has been shown. */
  confirm: () => void;
  /** Drop the question, changing nothing. */
  cancel: () => void;
}

export function useSessionLogs(
  session: FixtureSession,
  onNotice: (message: string) => void,
): SessionLogs {
  const sessionId = session.config.id;
  const [provenance, setProvenance] = useState<LogsProvenance>("preview");
  const [status, setStatus] = useState<LogStatusDto>(() => previewLogStatus(session));
  const [runs, setRuns] = useState<RunRecordDto[]>(session.runs);
  const [unreadable, setUnreadable] = useState<LogErrorDto[]>([]);
  const [loading, setLoading] = useState(true);
  const [plan, setPlan] = useState<CleanupReportDto | null>(null);
  const [busy, setBusy] = useState(false);

  // A read that resolves after the selection changed must not overwrite the
  // session the user is now looking at.
  const generation = useRef(0);

  const fallBackToPreview = useCallback(() => {
    setStatus(previewLogStatus(session));
    setRuns(session.runs);
    setUnreadable([]);
    setProvenance("preview");
  }, [session]);

  // `loading` starts true and is only ever cleared: every read either resolves
  // with the backend's answer or falls back to the preview, and a refresh after
  // an action does not need to re-announce a read that is already on screen.
  const read = useCallback(() => {
    const mine = ++generation.current;
    Promise.all([invoke("get_log_info", { sessionId }), invoke("get_run_history", { sessionId })])
      .then(([info, history]) => {
        if (mine !== generation.current) return;
        if (!isLogStatusDto(info) || !isRunHistoryDto(history)) {
          // A payload the guards reject is a contract mismatch, not a session
          // fact. Falling back is right; falling back silently would hide the
          // mismatch, so it is said out loud as well.
          fallBackToPreview();
          onNotice("日志数据与 DTO 契约不符 · 已回退到预览数据");
          return;
        }
        setStatus(info);
        setRuns(history.runs);
        setUnreadable(history.unreadable);
        setProvenance("live");
      })
      .catch(() => {
        // Not registered in Core, or no host to ask (a browser preview): the
        // fixture's own record is the answer, labelled as such.
        if (mine === generation.current) fallBackToPreview();
      })
      .finally(() => {
        if (mine === generation.current) setLoading(false);
      });
  }, [fallBackToPreview, onNotice, sessionId]);

  // Read on mount. The tab is keyed by session (`App.tsx`), so a different
  // session is a different panel instance with its own state — including the
  // retention question below, which must never carry over to another session.
  useEffect(() => {
    read();
  }, [read]);

  // Run records and logging state arrive as events; the tab follows them while
  // it is mounted, so a run ending shows up in the history without the user
  // having to leave the tab and come back.
  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    let cancelled = false;
    for (const name of [SESSION_STATE_CHANGED, RUN_RECORD_UPDATED]) {
      listen<{ sessionId?: string }>(name, (event) => {
        if (event.payload?.sessionId === sessionId) {
          read();
        }
      })
        .then((unlisten) => {
          // A listener that resolves after the tab closed has to be undone.
          if (cancelled) unlisten();
          else unlisteners.push(unlisten);
        })
        .catch(() => {
          // No host to listen to (a browser preview): the tab still reads on
          // mount, it just does not follow updates.
        });
    }
    return () => {
      cancelled = true;
      for (const unlisten of unlisteners) unlisten();
    };
  }, [read, sessionId]);

  const report = useCallback(
    (error: unknown, fallback: string) => {
      onNotice(isSessionErrorDto(error) ? error.message : fallback);
    },
    [onNotice],
  );

  /** Run one backend action and re-read, so the tab shows what happened. */
  const act = useCallback(
    (command: string, args: Record<string, unknown>, onSuccess?: () => void) => {
      if (provenance === "preview") {
        onNotice("预览数据 · 该会话未在后台注册，操作不会生效");
        return;
      }
      invoke(command, { sessionId, ...args })
        .then(() => onSuccess?.())
        .catch((error: unknown) => {
          report(error, `「${command}」执行失败`);
          // A failed action usually means the disk moved under the list — a log
          // cleaned up outside the app, an application that has not written its
          // file yet — so the tab re-reads rather than keeping the row that led
          // to the failure.
          read();
        });
    },
    [onNotice, provenance, read, report, sessionId],
  );

  const copyPath = useCallback(
    (path: string) => {
      // The clipboard needs a secure context, which a hosted preview is not;
      // falling back to telling the user the path is honest and still useful.
      const written = navigator.clipboard?.writeText(path);
      if (written === undefined) {
        onNotice(`复制不可用，请手动复制：${path}`);
        return;
      }
      written
        .then(() => onNotice(`已复制路径：${path}`))
        .catch(() => onNotice(`复制失败，请手动复制：${path}`));
    },
    [onNotice],
  );

  const previewCleanup = useCallback(() => {
    if (provenance === "preview") {
      onNotice("预览数据 · 该会话未在后台注册，无法清理日志");
      return;
    }
    setBusy(true);
    invoke("preview_log_cleanup", { sessionId })
      .then((value: unknown) => {
        if (!isCleanupReportDto(value)) {
          onNotice("后端返回的清理计划无法识别");
          return;
        }
        // The plan is a question, not a notice: it stays on screen until the
        // user answers it.
        setPlan(value);
      })
      .catch((error: unknown) => report(error, "无法读取清理计划"))
      .finally(() => setBusy(false));
  }, [onNotice, provenance, report, sessionId]);

  const confirmCleanup = useCallback(() => {
    setBusy(true);
    invoke("cleanup_logs", { sessionId })
      .then((value: unknown) => {
        if (!isCleanupReportDto(value)) {
          onNotice("后端返回的清理结果无法识别");
          return;
        }
        setPlan(null);
        onNotice(cleanupOutcome(value));
        read();
      })
      .catch((error: unknown) => report(error, "清理日志失败"))
      .finally(() => setBusy(false));
  }, [onNotice, read, report, sessionId]);

  return {
    provenance,
    status,
    runs,
    unreadable,
    loading,
    actions: {
      openFile: (runId?: string) => act("open_log_file", { runId: runId ?? null }),
      openFolder: (runId?: string) => act("open_log_folder", { runId: runId ?? null }),
      copyPath,
      saveRunLog: () => act("save_run_log", {}, () => onNotice("本次运行的缓冲已写入日志文件")),
      setRecording: (recording: boolean) =>
        act("set_log_recording", { recording }, () =>
          onNotice(recording ? "已开始记录本次运行" : "已停止记录"),
        ),
    },
    cleanup: {
      plan,
      busy,
      preview: previewCleanup,
      confirm: confirmCleanup,
      cancel: () => setPlan(null),
    },
  };
}
