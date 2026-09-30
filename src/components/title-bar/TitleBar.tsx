import { Menu } from "lucide-react";
import hubMarkUrl from "../../../assets/brand/hub-mark.svg";
import WindowControls from "./WindowControls";
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
  /** Where a window control that the OS refused is reported. */
  onNotice?: (message: string) => void;
}

/**
 * The window's only title bar: app identity, the global run summary, and the
 * window controls (issue #68).
 *
 * The main window is undecorated (`src-tauri/tauri.conf.json`), so this bar
 * carries what the system's title bar used to: it is the drag handle
 * (`data-tauri-drag-region="deep"` — Tauri walks up from the click, so the
 * glyph and the text drag the window while the buttons, being `<button>`s, do
 * not), the double-click-to-maximize target, and the home of
 * minimize / maximize / restore / close. Resizing still comes from the window's
 * own edges: Tauri attaches a native hit-test border to undecorated resizable
 * windows, which is why the bar keeps its padding clear of the frame.
 *
 * That V2's other title-bar items are still absent is deliberate — `Ctrl K`,
 * the global settings/logs entries and an Exit button remain out of scope here
 * (`docs/DESIGN_SPEC_EXTRACTED.md` §5.1). Closing hides to the tray and the
 * tray's own Exit is still the way out (D-006).
 */
export default function TitleBar({
  summary,
  pill,
  narrow,
  drawerOpen,
  onToggleDrawer,
  onNotice,
}: TitleBarProps) {
  return (
    <header className="title-bar" data-tauri-drag-region="deep">
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
      <WindowControls onNotice={onNotice} />
    </header>
  );
}

/**
 * Application mark: the Hub icon, rendered from the file the native icon set
 * is generated from (`assets/brand/hub-mark.svg`, D-028).
 *
 * It is that file rather than a copy of it because a second drawing of the same
 * mark is exactly the drift this ticket removed: one drawing, rasterized for
 * the taskbar, the tray and the installers, and rendered here as it is.
 */
function HubMark({ className }: { className?: string }) {
  return <img className={className} src={hubMarkUrl} alt="" draggable={false} />;
}
