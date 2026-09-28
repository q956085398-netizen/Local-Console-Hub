import {
  formatDuration,
  logModeLabel,
  logSourceLabel,
  statusTone,
  typeLabel,
} from "../../state/derivations";
import { FIXTURE_SESSIONS, type FixtureSession } from "../../state/fixtures";
import "./DetailsPanel.css";

export interface DetailsPanelProps {
  fixture: FixtureSession;
  now: Date;
}

/**
 * Details tab: identity, close impact, dependencies and low-frequency
 * technical metadata (UI_STYLE_GUIDE §6) — the information that must stay off
 * the terminal surface without earning a permanent side panel. Dependency
 * status is read from the same fixture the sidebar renders (T08 owns the
 * real dependency model).
 */
export default function DetailsPanel({ fixture, now }: DetailsPanelProps) {
  const { config, runtime } = fixture;
  const deps = (fixture.dependsOn ?? [])
    .map((id) => FIXTURE_SESSIONS.find((session) => session.config.id === id))
    .filter((session): session is FixtureSession => session !== undefined);

  const rows: Array<[string, string]> = [
    ["类型", typeLabel(config.sessionType)],
    ["状态", runtime.status + (fixture.busy ? " · busy" : fixture.ready ? " · ready" : "")],
    ["PID", runtime.pid !== undefined ? String(runtime.pid) : "—"],
    ["Run", runtime.runId !== undefined ? `run-${runtime.runId}` : "—"],
    ["端口", config.port !== undefined ? String(config.port) : "—"],
    ["URL", config.url ?? "—"],
    ["工作目录", config.cwd ?? "—"],
    ["启动命令", config.command ?? config.shell ?? "—"],
    [
      "日志模式",
      `${logModeLabel(runtime.logging.mode)} / ${logSourceLabel(runtime.logging.source)}`,
    ],
    [
      "运行时长",
      runtime.startedAt !== undefined && runtime.status === "running"
        ? formatDuration(runtime.startedAt, now)
        : "—",
    ],
  ];

  return (
    <div className="details-panel">
      <div className="details-panel__card">
        <p className="details-panel__eyebrow">它是谁</p>
        <h3 className="details-panel__name">{config.name}</h3>
        <p className="details-panel__purpose">{config.purpose ?? "—"}</p>
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
                  {dep.runtime.status}
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
