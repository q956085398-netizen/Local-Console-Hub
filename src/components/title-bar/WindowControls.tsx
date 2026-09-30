import { useWindowControls } from "../../app/useWindowControls";
import "./WindowControls.css";

export interface WindowControlsProps {
  /** Where an operation the OS refused is reported (the shell's notice line). */
  onNotice?: (message: string) => void;
}

/**
 * Minimize, maximize/restore and close, in the dark title bar (issue #68).
 *
 * The window is undecorated (`src-tauri/tauri.conf.json`), so these are the
 * window's only controls and the bar they sit in is also its drag handle and
 * its resize-border host. What the buttons *mean* is decided in
 * `src/app/window-controls.ts`; this file is their markup and their glyphs.
 *
 * Close is the same gesture the system's X was: the app's close handler hides
 * the window to the tray and keeps every managed session running (D-006).
 *
 * ## The glyphs are drawn here, not taken from the icon set
 *
 * Everything else in the UI uses `lucide-react`. These three are not app
 * iconography but the window's own chrome, and they are drawn at the sizes and
 * stroke weights Windows draws them at — a 24-unit icon library scaled down to
 * a 10px control reads as a picture of a button rather than as the button.
 */
export default function WindowControls({ onNotice }: WindowControlsProps) {
  const { available, maximized, run } = useWindowControls(onNotice);
  const maximizeLabel = maximized ? "还原窗口" : "最大化窗口";

  return (
    <div className="window-controls">
      <button
        type="button"
        className="window-control"
        aria-label="最小化窗口"
        title="最小化窗口"
        disabled={!available}
        onClick={() => run("minimize")}
      >
        <svg viewBox="0 0 10 10" aria-hidden="true">
          <path d="M0 5h10" />
        </svg>
      </button>
      <button
        type="button"
        className="window-control"
        aria-label={maximizeLabel}
        title={maximizeLabel}
        disabled={!available}
        onClick={() => run("toggle-maximize")}
      >
        <svg viewBox="0 0 10 10" aria-hidden="true">
          {maximized ? (
            // Two offset squares: the back one peeking above the front one.
            <>
              <path d="M2.5 2.5V0.5h7v7h-2" />
              <rect x="0.5" y="2.5" width="7" height="7" />
            </>
          ) : (
            <rect x="0.5" y="0.5" width="9" height="9" />
          )}
        </svg>
      </button>
      <button
        type="button"
        className="window-control"
        aria-label="关闭窗口"
        title="关闭窗口（会话继续运行）"
        disabled={!available}
        onClick={() => run("close")}
      >
        <svg viewBox="0 0 10 10" aria-hidden="true">
          <path d="M0 0l10 10M10 0L0 10" />
        </svg>
      </button>
    </div>
  );
}
