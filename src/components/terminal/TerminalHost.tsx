import { useEffect, useRef } from "react";
import { Play } from "lucide-react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal, type ITheme } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { bufferDiscardNotice, ptyChromeLabel } from "../../state/derivations";
import type { SessionView } from "../../state/session-view";
import { decodeBase64 } from "../../types/terminal";
import { useTerminalStream } from "../../app/useTerminalStream";
import "./TerminalHost.css";

/**
 * Scrollback the renderer keeps, in lines.
 *
 * The backend keeps its own bounded scrollback (`docs/LOGGING.md` §8, 5 000
 * lines) and hands it over on attach; this is the renderer's copy of it, sized
 * to match so the two cannot disagree about how far back "scrollback" reaches.
 * One is memory in Rust, the other in the WebView, and a terminal that grows
 * either without a bound is the leak §14 rules out.
 */
const SCROLLBACK_LINES = 5_000;

/**
 * The V2 terminal palette (`docs/DESIGN_SPEC_EXTRACTED.md` §1.2).
 *
 * Only the surfaces, the default text and the cursor are set: a shell's own
 * ANSI colours (what `ls`, `git` and a build tool paint with) come from the
 * terminal emulator's standard palette, and inventing a bespoke one would make
 * a familiar tool look unfamiliar.
 */
const THEME: ITheme = {
  background: "#090a0d",
  foreground: "#d7d8d4",
  cursor: "#c5ccd6",
  cursorAccent: "#090a0d",
  selectionBackground: "rgba(197, 204, 214, 0.25)",
};

export interface TerminalHostProps {
  session: SessionView;
  /** Whether a backend is answering this workspace at all. */
  live: boolean;
  /** Start the selected session. In preview mode the caller surfaces a notice. */
  onStart: () => void;
}

/**
 * The dominant terminal host (UI_STYLE_GUIDE §7).
 *
 * With a backend this is a real terminal: xterm.js renders the session's PTY
 * stream, keystrokes travel back through `terminal_write`, and the view's size
 * is forwarded once it has settled. xterm.js owns no process and no PTY — the
 * session does, in the backend — which is what makes selecting another session,
 * switching tabs or hiding the window a *view* operation that cannot disturb a
 * running shell.
 *
 * Without a backend (the browser preview, and the moment before the first
 * listing returns) it renders the fixture preview stream. That is the only
 * remaining fake, it is unreachable while a backend is answering, and it is
 * what keeps the shell comparable to the reference images.
 */
export default function TerminalHost({ session, live, onStart }: TerminalHostProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const previewRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);

  const running = session.runtime.status === "running";
  const stream = useTerminalStream(live ? session.config.id : null, session.runtime.runId, live, {
    onAttach: (attachment) => {
      const term = termRef.current;
      if (term === null) {
        return;
      }
      // Attaching means "here is the stream from as far back as it is
      // retained": the surface is reset and the scrollback replayed, so the
      // view never keeps showing bytes from a run it is no longer attached
      // to.
      term.reset();
      for (const chunk of attachment.chunks) {
        term.write(decodeBase64(chunk.data));
      }
    },
    onData: (bytes) => termRef.current?.write(bytes),
  });

  // The handlers xterm installs are set up once per backend, not once per
  // session: the same emulator re-attaches to whatever session is selected, so
  // selecting another session cannot leave a listener behind pointing at the
  // previous one. The ref is how that listener reaches the current stream, and
  // it is written in an effect rather than during render — a ref written while
  // rendering is a value React may not have settled.
  const streamRef = useRef(stream);
  useEffect(() => {
    streamRef.current = stream;
  }, [stream]);

  useEffect(() => {
    if (!live) {
      return;
    }
    const container = containerRef.current;
    if (container === null) {
      return;
    }

    const term = new Terminal({
      scrollback: SCROLLBACK_LINES,
      fontFamily: '"IBM Plex Mono", "Cascadia Mono", ui-monospace, Consolas, monospace',
      fontSize: 12.5,
      lineHeight: 1.55,
      cursorBlink: true,
      theme: THEME,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(container);
    termRef.current = term;

    // Ctrl+C needs no special path here: xterm sends the byte a terminal sends
    // (0x03) through the same route as any other key, and the backend raises it
    // as an interrupt for the shell's process group. Interrupting is not
    // closing the session (spec §7) — that is the Stop action.
    const input = term.onData((data) => streamRef.current.send(data));

    const sync = () => {
      if (container.clientWidth === 0 || container.clientHeight === 0) {
        // A hidden or not-yet-laid-out container has no geometry to fit to;
        // fitting anyway would report a size the view does not have.
        return;
      }
      fit.fit();
      streamRef.current.resize(term.cols, term.rows);
    };
    const observer = new ResizeObserver(sync);
    observer.observe(container);
    sync();

    return () => {
      observer.disconnect();
      input.dispose();
      term.dispose();
      termRef.current = null;
    };
  }, [live]);

  useEffect(() => {
    const el = previewRef.current;
    if (el) {
      el.scrollTop = el.scrollHeight;
    }
  }, [session.config.id, session.lines]);

  const discardNotice = bufferDiscardNotice(session.runtime);
  const stateText =
    discardNotice ??
    (stream.gapped ? "部分输出缺失" : null) ??
    stream.error ??
    (running ? "connected" : session.runtime.status);

  return (
    <div className="terminal-host">
      <div className="terminal-host__chrome">
        <p className="terminal-host__mode">{ptyChromeLabel(session.config, session.runtime)}</p>
        <p className="terminal-host__state" title={stream.error ?? undefined}>
          {stateText}
        </p>
      </div>

      {live ? (
        <div
          className="terminal-host__body terminal-host__body--xterm"
          ref={containerRef}
          data-testid="terminal-surface"
        />
      ) : (
        <div className="terminal-host__body" ref={previewRef}>
          {(session.lines ?? []).map((line, index) => (
            <div key={index} className={`terminal-host__line terminal-host__line--${line.kind}`}>
              {line.text}
            </div>
          ))}
          {running && (
            <div className="terminal-host__prompt-line">
              <span className="terminal-host__prompt">{promptFor(session)}</span>
              <span className="terminal-host__caret" aria-hidden="true" />
            </div>
          )}
        </div>
      )}

      {!running && (
        <div className="terminal-host__overlay">
          <div className="terminal-host__overlay-card">
            <p className="terminal-host__overlay-title">会话未运行</p>
            <p className="terminal-host__overlay-hint">
              交互终端必须先启动进程。这不是只读日志面板。
            </p>
            <button
              type="button"
              className="btn btn--primary btn--sm terminal-host__overlay-button"
              onClick={onStart}
            >
              <Play size={14} />
              启动 {session.config.name}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

/** Shell prompt for the preview stream (`PS D:\Work>` / `sillytavern $`). */
function promptFor(session: SessionView): string {
  if (session.config.sessionType === "terminal") {
    return `PS ${session.config.cwd ?? ""}>`;
  }
  return `${session.config.id} $`;
}
