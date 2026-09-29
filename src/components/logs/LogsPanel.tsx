import { CirclePlay, CircleStop, Copy, FolderOpen, Save, ScrollText, Trash2 } from "lucide-react";
import { logSourceLabel, loggingHeadline, runOutcomeBadge } from "../../state/derivations";
import {
  bufferNote,
  cleanupPrompt,
  currentLogFile,
  fileActions,
  logActionAvailability,
  logPathEntries,
  logStateLabel,
  logStateTone,
  runFilePathNote,
  runLogFile,
  runsNewestFirst,
  showsSourceBadge,
} from "../../state/logs";
import type { SessionView } from "../../state/session-view";
import { isPresent } from "../../types/runtime";
import type { RunHistoryEntryDto } from "../../types/logs";
import { useSessionLogs } from "../../app/useSessionLogs";
import "./LogsPanel.css";

export interface LogsPanelProps {
  session: SessionView;
  /** Status-bar notice: what an action did, or why it could not. */
  onNotice: (message: string) => void;
}

/**
 * Logs tab: the effective policy first, then the run history (UI_STYLE_GUIDE §8).
 *
 * Three things this surface has to keep apart, because `docs/LOGGING.md` never
 * lets them merge: the in-memory terminal buffer, the Hub-owned captured log,
 * and an application-owned external log. The state badge answers "is this being
 * logged?" before anything else (T05's §1.4 requirement), and the path rows say
 * where.
 *
 * T06 rendered this from fixtures; T10 (#11) reads it back from Session Core
 * and wires the file and retention actions. The fixtures are still the answer
 * for a session the backend does not know, and then the panel says so rather
 * than presenting them as live.
 *
 * A run whose log retention has since swept stays on the list and says so
 * (`RunRow`): the record is the evidence the run happened, and a view that
 * dropped the row — or offered to open a file that is gone — would lose that.
 * The current run's card answers that question the same way, because both it
 * and the rows read their file actions from `fileActions` (D-022): a file that
 * is gone loses 打开日志 and 复制路径 in both places, and keeps 打开目录 in both.
 */
export default function LogsPanel({ session, onNotice }: LogsPanelProps) {
  const logs = useSessionLogs(session, onNotice);
  const { status, provenance, actions, cleanup, logActionBusy } = logs;
  const availability = logActionAvailability(status);
  const current = currentLogFile(status);
  const currentActions = fileActions(current);
  const ordered = runsNewestFirst(logs.runs);

  return (
    <div className="logs-panel">
      <div className="logs-panel__policy">
        <div className="logs-panel__badges">
          <span className={`badge badge--${logStateTone(status.state)}`}>
            {logStateLabel(status.state)}
          </span>
          {showsSourceBadge(status) && (
            <span className="badge badge--outline">{logSourceLabel(status.source)}</span>
          )}
          <span className="badge badge--outline">
            {status.recordsInput ? "输入会被记录" : "stdin 不记录"}
          </span>
          <span
            className={
              provenance === "preview"
                ? "badge badge--outline logs-panel__provenance--preview"
                : "badge badge--outline"
            }
            title={
              provenance === "live"
                ? "这些值来自 Session Core"
                : "该会话未在后台注册，显示的是预览配置"
            }
          >
            {logs.loading ? "读取中" : provenance === "live" ? "后端实时" : "预览数据"}
          </span>
        </div>
        <p className="logs-panel__headline">{loggingHeadline(status)}</p>
        <dl className="logs-panel__paths">
          {logPathEntries(status).map((entry) => (
            <div key={entry.label} className="logs-panel__path-row">
              <dt>{entry.label}</dt>
              <dd>{entry.value}</dd>
            </div>
          ))}
          <div className="logs-panel__path-row">
            <dt>滚动缓冲</dt>
            <dd>{bufferNote(status)}</dd>
          </div>
        </dl>

        {status.truncated && (
          <p className="logs-panel__warning">
            当前运行的日志已达到单文件上限并停止写入 · 之后的输出只留在内存缓冲里
          </p>
        )}
        {isPresent(status.lastError) && (
          <p className="logs-panel__warning">
            {status.lastError.operation} · {status.lastError.message}
          </p>
        )}

        <div className="logs-panel__actions">
          <button
            type="button"
            className="btn btn--secondary btn--sm"
            disabled={!currentActions.open}
            onClick={() => actions.openFile()}
          >
            <ScrollText size={14} />
            打开日志
          </button>
          <button
            type="button"
            className="btn btn--secondary btn--sm"
            disabled={!currentActions.folder}
            onClick={() => actions.openFolder()}
          >
            <FolderOpen size={14} />
            打开目录
          </button>
          <button
            type="button"
            className="btn btn--ghost btn--sm"
            disabled={!currentActions.copy}
            onClick={() => current.path !== undefined && actions.copyPath(current.path)}
          >
            <Copy size={14} />
            复制路径
          </button>
          {availability.saveRunLog && (
            <button
              type="button"
              className="btn btn--secondary btn--sm"
              disabled={logActionBusy}
              title="把当前运行的缓冲写进日志文件 · 会话停止时没有可保存的运行"
              onClick={() => actions.saveRunLog()}
            >
              <Save size={14} />
              保存本次日志
            </button>
          )}
          {availability.recording !== null && (
            <button
              type="button"
              className="btn btn--secondary btn--sm"
              disabled={logActionBusy}
              onClick={() => actions.setRecording(availability.recording === "start")}
            >
              {availability.recording === "start" ? (
                <>
                  <CirclePlay size={14} />
                  开始记录
                </>
              ) : (
                <>
                  <CircleStop size={14} />
                  停止记录
                </>
              )}
            </button>
          )}
        </div>
      </div>

      <div className="logs-panel__history">
        <div className="logs-panel__history-head">
          <p className="logs-panel__history-title">运行历史</p>
          <button
            type="button"
            className="btn btn--ghost btn--sm logs-panel__cleanup"
            disabled={cleanup.busy}
            onClick={() => cleanup.preview()}
          >
            <Trash2 size={14} />
            清理旧日志
          </button>
        </div>

        {cleanup.plan !== null && (
          <div className="logs-panel__confirm" role="alert">
            <p className="logs-panel__confirm-text">{cleanupPrompt(cleanup.plan)}</p>
            <div className="logs-panel__confirm-actions">
              <button
                type="button"
                className="btn btn--sm logs-panel__destructive"
                disabled={cleanup.busy}
                onClick={() => cleanup.confirm()}
              >
                确认清理
              </button>
              <button
                type="button"
                className="btn btn--ghost btn--sm"
                disabled={cleanup.busy}
                onClick={() => cleanup.cancel()}
              >
                取消
              </button>
            </div>
          </div>
        )}

        {logs.unreadable.length > 0 && (
          <p className="logs-panel__warning logs-panel__warning--list">
            {logs.unreadable.length} 条运行记录无法读取，列表可能不完整 ·{" "}
            {logs.unreadable[0].message}
          </p>
        )}

        <ul className="logs-panel__runs">
          {ordered.length === 0 ? (
            <li className="logs-panel__runs-empty">
              {status.source === "none" || status.mode === "off"
                ? "该会话不写磁盘日志（仅内存缓冲），因此没有伪造的空记录。"
                : "还没有运行记录。"}
            </li>
          ) : (
            ordered.map((run) => (
              <RunRow
                key={run.runId}
                run={run}
                onOpen={actions.openFile}
                onFolder={actions.openFolder}
                onCopy={actions.copyPath}
              />
            ))
          )}
        </ul>
      </div>
    </div>
  );
}

interface RunRowProps {
  run: RunHistoryEntryDto;
  onOpen: (runId?: string) => void;
  onFolder: (runId?: string) => void;
  onCopy: (path: string) => void;
}

/**
 * One run: what it was, how it ended, and where its log went.
 *
 * A run can outlive its log — retention deletes files and never the record of
 * the run that wrote them (`docs/LOGGING.md` §9) — so a row may point at a file
 * that is not on disk. It says so, and it offers only the folder action: the
 * folder survives a sweep, while "open log" and "copy path" would act on a file
 * that is gone. Nothing here is coloured — a swept log is retention working,
 * not a lifecycle failure (UI_STYLE_GUIDE §10).
 *
 * The wording states the fact and stops there. "不在磁盘上" rather than "已被
 * 清理", because this row cannot tell a swept log from one an `external`
 * application has not written yet, and naming a cause it did not observe would
 * be the kind of invention the rest of the tab avoids.
 */
function RunRow({ run, onOpen, onFolder, onCopy }: RunRowProps) {
  const outcome = runOutcomeBadge(run);
  const file = run.logFile;
  const available = fileActions(runLogFile(run));
  return (
    <li className="logs-panel__run">
      <div className="logs-panel__run-top">
        <div className="logs-panel__run-id">
          <span className="logs-panel__run-name">run-{run.runId}</span>
          <span className={`badge badge--${outcome.tone}`}>{outcome.label}</span>
        </div>
        <div className="logs-panel__run-tail">
          <span className="logs-panel__run-time">{formatClock(run.startedAt)}</span>
          {available.folder && (
            <div className="logs-panel__run-actions">
              {available.open && (
                <button
                  type="button"
                  className="btn btn--ghost btn--icon-sm"
                  title="打开日志"
                  aria-label={`打开 run-${run.runId} 的日志`}
                  onClick={() => onOpen(run.runId)}
                >
                  <ScrollText size={14} />
                </button>
              )}
              {available.copy && (
                <button
                  type="button"
                  className="btn btn--ghost btn--icon-sm"
                  title="复制路径"
                  aria-label={`复制 run-${run.runId} 的日志路径`}
                  onClick={() => isPresent(file) && onCopy(file)}
                >
                  <Copy size={14} />
                </button>
              )}
              <button
                type="button"
                className="btn btn--ghost btn--icon-sm"
                title={
                  available.open
                    ? "打开所在目录"
                    : "打开所在目录 · 日志文件不在磁盘上，目录仍然保留"
                }
                aria-label={`打开 run-${run.runId} 的目录`}
                onClick={() => onFolder(run.runId)}
              >
                <FolderOpen size={14} />
              </button>
            </div>
          )}
        </div>
      </div>
      <p className="logs-panel__run-detail">
        {runFilePathNote(run)}
        {isPresent(run.pid) ? ` · PID ${run.pid}` : ""}
        {isPresent(run.exitCode) && isPresent(run.endedAt) ? ` · exit ${run.exitCode}` : ""}
      </p>
    </li>
  );
}

/** `2026-09-28 06:00:15` — run history timestamps (UTC, per D-016). */
function formatClock(timestamp: string): string {
  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) return timestamp;
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${date.getUTCFullYear()}-${pad(date.getUTCMonth() + 1)}-${pad(date.getUTCDate())} ${pad(date.getUTCHours())}:${pad(date.getUTCMinutes())}:${pad(date.getUTCSeconds())}`;
}
