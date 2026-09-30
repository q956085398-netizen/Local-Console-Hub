import { AppWindow, Play } from "lucide-react";
import { isLive, standaloneNote } from "../../state/derivations";
import type { SessionView } from "../../state/session-view";
import "./StandalonePanel.css";

export interface StandalonePanelProps {
  session: SessionView;
  /**
   * Open the application: start it, or bring its own window forward.
   *
   * The same call the header's 启动 button makes, so the two cannot disagree
   * about what opening means — and in preview mode the caller turns it into a
   * notice, exactly as the terminal pane's start does.
   */
  onActivate: () => void;
}

/**
 * The pane a standalone-window application gets instead of a terminal (#66).
 *
 * An entry configured as `display: window` keeps the window *and* the console
 * its application provides, so there is no Hub-side stream to render. Drawing
 * an empty terminal for it would be the "假内嵌" spec #59 decision 16 rules out:
 * a surface that looks like the application's console but is not one, staying
 * empty forever while the user's real console sits behind the Hub window.
 *
 * What is left is honest and useful: where the console actually is, and the one
 * action that makes sense — open it.
 */
export default function StandalonePanel({ session, onActivate }: StandalonePanelProps) {
  const running = isLive(session.runtime.status);

  return (
    <div className="standalone-panel">
      <div className="standalone-panel__chrome">
        <p className="standalone-panel__mode">独立窗口 · 应用自己的控制台</p>
        <p className="standalone-panel__state">{running ? "运行中" : session.runtime.status}</p>
      </div>
      <div className="standalone-panel__body">
        <div className="standalone-panel__card">
          <AppWindow size={20} aria-hidden="true" />
          <p className="standalone-panel__title">{session.config.name} 使用独立窗口</p>
          <p className="standalone-panel__hint">{standaloneNote(session.runtime)}</p>
          <button
            type="button"
            className="btn btn--primary btn--sm standalone-panel__button"
            onClick={onActivate}
          >
            <Play size={14} />
            {running ? "唤起应用窗口" : `启动 ${session.config.name}`}
          </button>
        </div>
      </div>
    </div>
  );
}
