import { Copy, FolderOpen, ScrollText, Trash2 } from "lucide-react";
import {
  logPolicyBadge,
  logSourceLabel,
  loggingHeadline,
  runOutcome,
  runOutcomeBadge,
} from "../../state/derivations";
import type { FixtureSession } from "../../state/fixtures";
import "./LogsPanel.css";

export interface LogsPanelProps {
  fixture: FixtureSession;
  /** Fixture mode: file actions are visible but wired to a notice (T10). */
  onAction: (label: string) => void;
}

/**
 * Logs tab: effective policy first, then run history (UI_STYLE_GUIDE §8).
 *
 * T06 renders the fixture run records; T10 (#11) replaces them with
 * `get_run_history` / `get_log_info` and adds retention controls. The
 * no-fabrication rule is already live: an off/none session shows no invented
 * disk-log records.
 */
export default function LogsPanel({ fixture, onAction }: LogsPanelProps) {
  const { config, runtime, runs } = fixture;
  const logging = runtime.logging ?? config.logging;
  const visibleRuns = runs.filter(
    (run) => run.logFile !== undefined || runOutcome(run) === "error" || logging.source !== "none",
  );
  const current = runs.find((run) => run.runId === runtime.runId) ?? runs[runs.length - 1];
  const policy = logPolicyBadge(logging, runtime.status);

  return (
    <div className="logs-panel">
      <div className="logs-panel__policy">
        <div className="logs-panel__badges">
          <span className={`badge badge--${policy.tone}`}>{policy.label}</span>
          <span className="badge badge--outline">{logSourceLabel(logging.source)}</span>
          <span className="badge badge--outline">stdin 不记录</span>
        </div>
        <p className="logs-panel__headline">{loggingHeadline(logging)}</p>
        <p className="logs-panel__path">
          {current?.logFile ??
            (logging.source === "none" ? "无持久化文件 · 仅内存滚动缓冲" : "本次运行尚未生成文件")}
        </p>
        <div className="logs-panel__actions">
          <button
            type="button"
            className="btn btn--secondary btn--sm"
            disabled={current?.logFile === undefined}
            onClick={() => onAction("打开日志")}
          >
            <ScrollText size={14} />
            打开日志
          </button>
          <button
            type="button"
            className="btn btn--secondary btn--sm"
            disabled={current?.logFile === undefined}
            onClick={() => onAction("打开目录")}
          >
            <FolderOpen size={14} />
            打开目录
          </button>
          <button
            type="button"
            className="btn btn--ghost btn--sm"
            disabled={current?.logFile === undefined}
            onClick={() => onAction("复制路径")}
          >
            <Copy size={14} />
            复制路径
          </button>
        </div>
      </div>

      <div className="logs-panel__history">
        <div className="logs-panel__history-head">
          <p className="logs-panel__history-title">运行历史</p>
          <button
            type="button"
            className="btn btn--ghost btn--sm logs-panel__cleanup"
            onClick={() => onAction("清理旧日志")}
          >
            <Trash2 size={14} />
            清理旧日志
          </button>
        </div>
        <ul className="logs-panel__runs">
          {visibleRuns.length === 0 ? (
            <li className="logs-panel__runs-empty">
              {logging.mode === "off"
                ? "交互终端默认不产生磁盘日志，因此没有伪造的空记录。"
                : "还没有运行记录。"}
            </li>
          ) : (
            [...visibleRuns].reverse().map((run) => {
              const outcome = runOutcomeBadge(run);
              return (
                <li key={run.runId} className="logs-panel__run">
                  <div className="logs-panel__run-top">
                    <div className="logs-panel__run-id">
                      <span className="logs-panel__run-name">run-{run.runId}</span>
                      <span className={`badge badge--${outcome.tone}`}>{outcome.label}</span>
                    </div>
                    <span className="logs-panel__run-time">{formatClock(run.startedAt)}</span>
                  </div>
                  <p className="logs-panel__run-detail">
                    {run.logFile ?? "未落盘"}
                    {run.pid !== undefined ? ` · PID ${run.pid}` : ""}
                    {run.exitCode !== undefined && run.endedAt !== undefined
                      ? ` · exit ${run.exitCode}`
                      : ""}
                  </p>
                </li>
              );
            })
          )}
        </ul>
      </div>
    </div>
  );
}

/** `2026-09-28 06:00:15` — run history timestamps (UTC, per D-016). */
function formatClock(timestamp: string): string {
  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) return timestamp;
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${date.getUTCFullYear()}-${pad(date.getUTCMonth() + 1)}-${pad(date.getUTCDate())} ${pad(date.getUTCHours())}:${pad(date.getUTCMinutes())}:${pad(date.getUTCSeconds())}`;
}
