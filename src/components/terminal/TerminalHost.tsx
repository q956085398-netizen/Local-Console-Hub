import { useEffect, useRef } from "react";
import { Play } from "lucide-react";
import type { FixtureSession } from "../../state/fixtures";
import "./TerminalHost.css";

export interface TerminalHostProps {
  fixture: FixtureSession;
  /** Fixture mode: the start button surfaces a notice (T07/T08 wire it). */
  onAction: (label: string) => void;
}

/**
 * The dominant terminal/output host (UI_STYLE_GUIDE §7).
 *
 * T06 renders the fixture preview lines; T07 (#8) replaces this body with an
 * xterm.js view attached to the live PTY. No fake command execution ships —
 * the caret is presentation of fixture state, and there is no input path
 * until the real one exists.
 */
export default function TerminalHost({ fixture, onAction }: TerminalHostProps) {
  const bodyRef = useRef<HTMLDivElement>(null);
  const lines = fixture.lines ?? [];
  const live = fixture.runtime.status === "running";

  useEffect(() => {
    const el = bodyRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [fixture.config.id, fixture.lines]);

  return (
    <div className="terminal-host">
      <div className="terminal-host__chrome">
        <p className="terminal-host__mode">
          {fixture.config.sessionType === "terminal"
            ? "ConPTY · interactive"
            : "PTY attached · stdin 可用"}
        </p>
        <p className="terminal-host__state">{live ? "connected" : fixture.runtime.status}</p>
      </div>
      <div className="terminal-host__body" ref={bodyRef}>
        {lines.map((line, index) => (
          <div key={index} className={`terminal-host__line terminal-host__line--${line.kind}`}>
            {line.text}
          </div>
        ))}
        {live && (
          <div className="terminal-host__prompt-line">
            <span className="terminal-host__prompt">{promptFor(fixture)}</span>
            <span className="terminal-host__caret" aria-hidden="true" />
          </div>
        )}
      </div>
      {!live && (
        <div className="terminal-host__overlay">
          <div className="terminal-host__overlay-card">
            <p className="terminal-host__overlay-title">会话未运行</p>
            <p className="terminal-host__overlay-hint">
              交互终端必须先启动进程。这不是只读日志面板。
            </p>
            <button
              type="button"
              className="btn btn--primary btn--sm terminal-host__overlay-button"
              onClick={() => onAction(`启动 ${fixture.config.name}`)}
            >
              <Play size={14} />
              启动 {fixture.config.name}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

/** Shell prompt for the fixture preview (`PS D:\Work>` / `sillytavern $`). */
function promptFor(fixture: FixtureSession): string {
  if (fixture.config.sessionType === "terminal") {
    return `PS ${fixture.config.cwd ?? ""}>`;
  }
  return `${fixture.config.id} $`;
}
