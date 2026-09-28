import { WORKSPACE_TABS, type WorkspaceTab } from "../../state/view";
import "./WorkspaceTabs.css";

export interface WorkspaceTabsProps {
  active: WorkspaceTab;
  onChange: (tab: WorkspaceTab) => void;
}

/** 终端 / 日志 / 详情 — the normative content switch. */
export default function WorkspaceTabs({ active, onChange }: WorkspaceTabsProps) {
  return (
    <nav className="workspace-tabs" role="tablist" aria-label="会话内容视图">
      {WORKSPACE_TABS.map((tab) => (
        <button
          key={tab.key}
          type="button"
          role="tab"
          aria-selected={active === tab.key}
          className={`workspace-tabs__tab${active === tab.key ? " workspace-tabs__tab--active" : ""}`}
          onClick={() => onChange(tab.key)}
        >
          {tab.label}
        </button>
      ))}
    </nav>
  );
}
