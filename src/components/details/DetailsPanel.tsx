import { Save } from "lucide-react";
import {
  closeMechanics,
  dependenciesOf,
  healthReading,
  isReady,
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
  onSaveTerminal: () => void;
  closing: boolean;
}

/**
 * Details tab: identity, close impact, dependencies and the *low-frequency*
 * technical fields (UI_STYLE_GUIDE §6).
 *
 * Deliberately does not repeat the header's live metadata line — PID, port,
 * uptime, cwd and effective log policy are already visible one tab across
 * (§13 forbids excessive duplication). What lives here is what the header
 * does not carry: the launch command, run identity, run outcome, PTY state,
 * the scrollback summary, and since T08 the health reading — the one place the
 * TCP facts behind the header's badge are spelled out, with the HTTP probe
 * beside them when that probe was issued (`derivations.healthReading`).
 */
export default function DetailsPanel({
  session,
  sessions,
  onSaveTerminal,
  closing,
}: DetailsPanelProps) {
  const { config, runtime } = session;
  const deps = dependenciesOf(session, sessions);
  const health = healthReading(runtime);

  const rows: Array<[string, string]> = [
    ["类型", typeLabel(config.sessionType)],
    ["状态", statusLabel(runtime.status, session.busy ?? false, isReady(runtime))],
    // Only while there is a reading: the row says nothing rather than claiming
    // a port is closed when nothing has been probed (spec §12). When both
    // exist, the same row shows TCP reachability and the HTTP result together.
    ...(health !== undefined ? ([["健康", health]] as Array<[string, string]>) : []),
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
        <p className="details-panel__impact-note">{closeMechanics(config)}</p>
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
                  {statusLabel(dep.runtime.status, dep.busy ?? false, isReady(dep.runtime))}
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
      {config.temporary && (
        <button
          type="button"
          className="btn btn--secondary btn--sm details-panel__save"
          disabled={closing}
          onClick={onSaveTerminal}
          title="保存这个终端的 shell 和工作目录，下次打开 Hub 仍可用"
        >
          <Save size={14} />
          保存启动配置
        </button>
      )}
    </div>
  );
}
