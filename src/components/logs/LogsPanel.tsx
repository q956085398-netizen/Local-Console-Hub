import { CirclePlay, CircleStop, Copy, FolderOpen, Save, ScrollText, Trash2 } from "lucide-react";
import { logSourceLabel, loggingHeadline, runOutcomeBadge } from "../../state/derivations";
import {
  bufferNote,
  cleanupPrompt,
  currentLogPath,
  logActionAvailability,
  logPathEntries,
  logStateLabel,
  logStateTone,
  runHasLog,
  runsNewestFirst,
  showsSourceBadge,
} from "../../state/logs";
import type { FixtureSession } from "../../state/fixtures";
import type { RunRecordDto } from "../../types/runtime";
import { useSessionLogs } from "../../app/useSessionLogs";
import "./LogsPanel.css";

export interface LogsPanelProps {
  session: FixtureSession;
  /** Status-bar notice: what an action did, or why it could not. */
  onNotice: (message: string) => void;
}

/**
 * Logs tab: the effective policy first, then the run history (UI_STYLE_GUIDE §8).
 *
 * Three things this surface has to keep apart, because `docs/LOGGING.md` never
 * lets them merge: the in-memory terminal buffer, the Hub-owned captured log,
 * and an application-owned external log. The state badge answers "is this being
 * logged?" before anything else (T05's §1.4 requirement), the path rows say
 * where, and the history lists only runs that actually left something — no
 * fabricated empty records for a session that writes nothing.
 *
 * T06 rendered this from fixtures; T10 (#11) reads it back from Session Core
 * and wires the file and retention actions. The fixtures are still the answer
 * for a session the backend does not know, and then the panel says so rather
 * than presenting them as live.
 */
export default function LogsPanel({ session, onNotice }: LogsPanelProps) {
  const logs = useSessionLogs(session, onNotice);
  const { status, provenance, actions, cleanup } = logs;
  const availability = logActionAvailability(status);
  const current = currentLogPath(status);
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
        {status.lastError != null && (
          <p className="logs-panel__warning">
            {status.lastError.operation} · {status.lastError.message}
          </p>
        )}

        <div className="logs-panel__actions">
          <button
            type="button"
            className="btn btn--secondary btn--sm"
            disabled={!availability.openCurrent}
            onClick={() => actions.openFile()}
          >
            <ScrollText size={14} />
            打开日志
          </button>
          <button
            type="button"
            className="btn btn--secondary btn--sm"
            disabled={!availability.openCurrent}
            onClick={() => actions.openFolder()}
          >
            <FolderOpen size={14} />
            打开目录
          </button>
          <button
            type="button"
            className="btn btn--ghost btn--sm"
            disabled={current === undefined}
            onClick={() => current !== undefined && actions.copyPath(current)}
          >
            <Copy size={14} />
            复制路径
          </button>
          {availability.saveRunLog && (
            <button
              type="button"
              className="btn btn--secondary btn--sm"
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
  run: RunRecordDto;
  onOpen: (runId?: string) => void;
  onFolder: (runId?: string) => void;
  onCopy: (path: string) => void;
}

/** One run: what it was, how it ended, and where its log went. */
function RunRow({ run, onOpen, onFolder, onCopy }: RunRowProps) {
  const outcome = runOutcomeBadge(run);
  const file = run.logFile;
  return (
    <li className="logs-panel__run">
      <div className="logs-panel__run-top">
        <div className="logs-panel__run-id">
          <span className="logs-panel__run-name">run-{run.runId}</span>
          <span className={`badge badge--${outcome.tone}`}>{outcome.label}</span>
        </div>
        <div className="logs-panel__run-tail">
          <span className="logs-panel__run-time">{formatClock(run.startedAt)}</span>
          {runHasLog(run) && (
            <div className="logs-panel__run-actions">
              <button
                type="button"
                className="btn btn--ghost btn--icon-sm"
                title="打开日志"
                aria-label={`打开 run-${run.runId} 的日志`}
                onClick={() => onOpen(run.runId)}
              >
                <ScrollText size={14} />
              </button>
              <button
                type="button"
                className="btn btn--ghost btn--icon-sm"
                title="打开所在目录"
                aria-label={`打开 run-${run.runId} 的目录`}
                onClick={() => onFolder(run.runId)}
              >
                <FolderOpen size={14} />
              </button>
              <button
                type="button"
                className="btn btn--ghost btn--icon-sm"
                title="复制路径"
                aria-label={`复制 run-${run.runId} 的日志路径`}
                onClick={() => file != null && onCopy(file)}
              >
                <Copy size={14} />
              </button>
            </div>
          )}
        </div>
      </div>
      <p className="logs-panel__run-detail">
        {file ?? "未落盘"}
        {run.pid != null ? ` · PID ${run.pid}` : ""}
        {run.exitCode != null && run.endedAt != null ? ` · exit ${run.exitCode}` : ""}
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
