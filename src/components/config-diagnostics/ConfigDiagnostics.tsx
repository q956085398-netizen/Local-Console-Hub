import type { ConfigReportDto, SessionConfigErrorDto } from "../../types/config";
import "./ConfigDiagnostics.css";

export interface ConfigDiagnosticsProps {
  report: ConfigReportDto | null;
  /** Failure to fetch the startup report itself, distinct from a config error. */
  error: string | null;
  sessionCount: number;
  /** Render a first-run or empty-config explanation when there are no sessions. */
  empty?: boolean;
}

function errorLocation(error: SessionConfigErrorDto): string {
  if (error.index === 0) {
    return error.field ? `配置文件 · ${error.field}` : "配置文件";
  }
  const id = error.sessionId ? ` · ${error.sessionId}` : "";
  const field = error.field ? ` · ${error.field}` : "";
  return `第 ${error.index} 项${id}${field}`;
}

export default function ConfigDiagnostics({
  report,
  error,
  sessionCount,
  empty = false,
}: ConfigDiagnosticsProps) {
  const errors = report?.errors ?? [];
  const hasProblems = errors.length > 0 || error !== null;
  if (!empty && !hasProblems) return null;

  const firstRun = empty && report?.fileStatus === "missing" && !hasProblems;
  const validEmpty = empty && report?.fileStatus === "loaded" && !hasProblems;
  const pending = empty && report === null && error === null;
  const title = error
    ? "无法读取配置诊断"
    : errors.length > 0
      ? `配置问题（${errors.length}）`
      : firstRun
        ? "首次运行"
        : validEmpty
          ? "配置文件中还没有定义会话"
          : "没有可显示的会话";
  const description = error
    ? error
    : errors.length > 0
      ? sessionCount > 0
        ? `其余 ${sessionCount} 个有效会话仍可使用。修复配置后重启应用以重新加载。`
        : "没有有效会话可显示；请修复下面的问题并重启应用。"
      : firstRun
        ? "首次使用，请点击下方“添加应用”保存常用应用，或点击“新建 PowerShell”直接打开终端。"
        : validEmpty
          ? "配置文件可正常读取，但当前没有会话条目。"
          : pending
            ? "正在读取配置状态…"
            : "请检查应用的配置文件后重启。";

  const tone = error || (errors.length > 0 && sessionCount === 0) ? "error" : "warning";

  return (
    <section
      className={`config-diagnostics config-diagnostics--${tone}${empty ? " config-diagnostics--empty" : ""}`}
      role={hasProblems ? "alert" : "status"}
      aria-label="配置诊断"
    >
      <div className="config-diagnostics__heading">
        <span className={`pip ${tone === "error" ? "pip--err" : "pip--warn"}`} aria-hidden="true" />
        <strong>{title}</strong>
        {report?.configPath && (
          <code className="config-diagnostics__path" title={report.configPath}>
            {report.configPath}
          </code>
        )}
      </div>
      <p className="config-diagnostics__description">{description}</p>
      {errors.length > 0 && (
        <ul className="config-diagnostics__errors">
          {errors.map((item, index) => (
            <li key={`${item.index}-${item.sessionId ?? "file"}-${item.field ?? index}`}>
              <span className="config-diagnostics__location">{errorLocation(item)}</span>
              <span>{item.message}</span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
