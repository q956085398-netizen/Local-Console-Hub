import type { LiveCounts } from "../../state/derivations";
import type { BackendConnection } from "../../app/useBackendPing";
import "./StatusBar.css";

export interface StatusBarProps {
  counts: LiveCounts;
  connection: BackendConnection;
  /** Transient fixture-action notice; null when nothing to say. */
  notice: string | null;
}

/** The minimal bottom strip (UI_STYLE_GUIDE §3). */
export default function StatusBar({ counts, connection, notice }: StatusBarProps) {
  return (
    <footer className="status-bar">
      <span className="status-bar__left">
        Hub 常驻 · {counts.running}/{counts.total} 运行 · 输入默认不记录
      </span>
      <span className="status-bar__right">
        {notice ?? <span className="status-bar__hint">关闭窗口 ≠ 停止服务</span>}
        {connection.state === "unavailable" && (
          <span className="status-bar__preview" title="未连接到 Rust 后端">
            UI 预览
          </span>
        )}
      </span>
    </footer>
  );
}
