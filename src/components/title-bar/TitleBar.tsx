import { Menu } from "lucide-react";
import "./TitleBar.css";

/** What the title-bar identity pill can say about the backend. */
export type TitlebarPill = "preview" | "connected";

export interface TitleBarProps {
  /** "2 运行 · 1 busy"-style run summary. */
  summary: string;
  /** Backend connectivity: the T00 ping contract, quietly surfaced. */
  pill: TitlebarPill;
  /** Narrow layout: offer a sidebar-drawer toggle instead of the static rail. */
  narrow: boolean;
  drawerOpen: boolean;
  onToggleDrawer: () => void;
}

/** The compact title bar: app identity and the global run summary. */
export default function TitleBar({
  summary,
  pill,
  narrow,
  drawerOpen,
  onToggleDrawer,
}: TitleBarProps) {
  return (
    <header className="title-bar">
      {narrow && (
        <button
          type="button"
          className="title-bar__menu"
          aria-label={drawerOpen ? "关闭会话列表" : "打开会话列表"}
          aria-expanded={drawerOpen}
          onClick={onToggleDrawer}
        >
          <Menu size={16} />
        </button>
      )}
      <HubMark className="title-bar__mark" />
      <div className="title-bar__name-wrap">
        <p className="title-bar__name">Local Console Hub</p>
        {pill === "preview" && <span className="title-bar__pill">UI 预览</span>}
      </div>
      <p className="title-bar__summary">{summary}</p>
    </header>
  );
}

/** Application mark, transcribed from the approved V2 prototype source. */
export function HubMark({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" className={className} aria-hidden="true">
      <rect x="3" y="3" width="18" height="18" rx="5" fill="#1a1c24" />
      <rect
        x="3.5"
        y="3.5"
        width="17"
        height="17"
        rx="4.5"
        fill="none"
        stroke="rgb(236 236 232 / 0.16)"
      />
      <circle cx="7.5" cy="8" r="1.15" fill="#6fba8a" />
      <rect x="10.2" y="7.15" width="8.2" height="1.5" rx="0.6" fill="#c5ccd6" opacity="0.9" />
      <rect x="6.4" y="11.2" width="11.2" height="1.4" rx="0.6" fill="#c5ccd6" opacity="0.45" />
      <rect x="6.4" y="14.6" width="8.4" height="1.4" rx="0.6" fill="#c5ccd6" opacity="0.28" />
    </svg>
  );
}
