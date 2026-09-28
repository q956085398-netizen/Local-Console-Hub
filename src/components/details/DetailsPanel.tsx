import {
  dependenciesOf,
  logModeLabel,
  logSourceLabel,
  statusLabel,
  statusTone,
  typeLabel,
} from "../../state/derivations";
import type { SessionView } from "../../state/session-view";
import { isPresent } from "../../types/runtime";
import "./DetailsPanel.css";

export interface DetailsPanelProps {
  session: SessionView;
  /** Every rendered session, for the dependency lookup — the same list the
   * rail is showing, so "depends on X" can never name a session the user
   * cannot see. */
  sessions: readonly SessionView[];
}

/**
 * Details tab: identity, close impact, dependencies and the *low-frequency*
 * technical fields (UI_STYLE_GUIDE §6).
 *
 * Deliberately does not repeat the header's live metadata line — PID, port,
 * uptime, cwd and effective log policy are already visible one tab across
 * (§13 forbids excessive duplication). What lives here is what the header
 * does not carry: the launch command, run identity, run outcome, PTY state
 * and the scrollback summary.
 */
export default function DetailsPanel({ session, sessions }: DetailsPanelProps) {
  const { config, runtime } = session;
  const deps = dependenciesOf(session, sessions);

  const rows: Array<[string, string]> = [
    ["类型", typeLabel(config.sessionType)],
    ["状态", statusLabel(runtime.status, session.busy ?? false, session.ready ?? false)],
    ["启动命令", config.command ?? config.shell ?? "—"],
    ["Run", isPresent(runtime.runId) ? `run-${runtime.runId}` : "—"],
    ["退出码", isPresent(runtime.exitCode) ? String(runtime.exitCode) : "—"],
    ["PTY", runtime.ptyAttached ? "attached" : "未附加"],
    [
      "日志模式",
      `${logModeLabel(runtime.logging.mode)} / ${logSourceLabel(runtime.logging.source)}`,
    ],
    [
      "内存缓冲",
      `${runtime.buffer.lines} 行 · ${runtime.buffer.bytes} B${
        runtime.buffer.droppedBytes > 0 ? ` · 已丢弃 ${runtime.buffer.droppedBytes} B` : ""
      }`,
    ],
    ...(isPresent(runtime.lastError)
      ? ([[`最近错误（${runtime.lastError.operation}）`, runtime.lastError.message]] as Array<
          [string, string]
        >)
      : []),
  ];

  return (
    <div className="details-panel">
      <div className="details-panel__card">
        <p className="details-panel__eyebrow">它是谁</p>
        <h3 className="details-panel__name">{config.name}</h3>
        <p className="details-panel__purpose">{config.purpose ?? "—"}</p>
        <p className="details-panel__identity">
          <span className="details-panel__mono">{config.cwd ?? "—"}</span>
          {config.url !== undefined && (
            <>
              {" · "}
              <span className="details-panel__mono">{config.url}</span>
            </>
          )}
        </p>
      </div>

      <div className="details-panel__impact">
        <p className="details-panel__eyebrow details-panel__eyebrow--busy">能不能关</p>
        <p className="details-panel__impact-text">{config.closeImpact ?? "—"}</p>
        <p className="details-panel__impact-note">
          停止会尝试优雅结束；强制结束是单独动作，且只作用于本会话进程树。
        </p>
      </div>

      {deps.length > 0 && (
        <div className="details-panel__card">
          <p className="details-panel__eyebrow">依赖</p>
          <ul className="details-panel__deps">
            {deps.map((dep) => (
              <li key={dep.config.id} className="details-panel__dep">
                <span>{dep.config.name}</span>
                <span
                  className={`badge badge--${statusTone(dep.runtime.status, dep.busy ?? false)}`}
                >
                  {statusLabel(dep.runtime.status, dep.busy ?? false, dep.ready ?? false)}
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}

      <dl className="details-panel__rows">
        {rows.map(([label, value]) => (
          <div key={label} className="details-panel__row">
            <dt>{label}</dt>
            <dd title={value}>{value}</dd>
          </div>
        ))}
      </dl>
    </div>
  );
}
