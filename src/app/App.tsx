import { useEffect, useMemo, useState } from "react";
import TitleBar from "../components/title-bar/TitleBar";
import Sidebar from "../components/sidebar/Sidebar";
import SessionHeader from "../components/session-header/SessionHeader";
import WorkspaceTabs from "../components/workspace/WorkspaceTabs";
import TerminalHost from "../components/terminal/TerminalHost";
import LogsPanel from "../components/logs/LogsPanel";
import DetailsPanel from "../components/details/DetailsPanel";
import StatusBar from "../components/status-bar/StatusBar";
import {
  filterSessions,
  groupSessions,
  initialSelectedSessionId,
  isReady,
  liveCounts,
  sidebarSummaryText,
  titlebarSummaryText,
} from "../state/derivations";
import { SESSION_ACTION_LABELS, type SessionAction } from "../state/actions";
import { DEFAULT_SELECTED_SESSION_ID, FIXTURE_GROUPS, FIXTURE_SESSIONS } from "../state/fixtures";
import { LIVE_GROUP } from "../state/session-view";
import type { WorkspaceTab } from "../state/view";
import { useBackendPing } from "./useBackendPing";
import { useSessionRegistry } from "./useSessionRegistry";
import { useMediaQuery } from "./useMediaQuery";
import "./App.css";

const NOTICE_TIMEOUT_MS = 4000;
/** Uptime text shows minutes and seconds; a slow tick keeps it fresh without
 * turning the whole shell into a per-second renderer (spec §14). */
const CLOCK_TICK_MS = 5000;

/**
 * The V2 workspace shell (T06 #7, wired to Session Core by T07 #8).
 *
 * Structure per docs/UI_STYLE_GUIDE.md: compact title bar, grouped session
 * sidebar, selected-session workspace (header + 终端/日志/详情 tabs, terminal
 * dominant), minimal status bar.
 *
 * Where the sessions come from is `useSessionRegistry`'s business: the backend's
 * registry when one is answering, the T06 fixture workspace otherwise. Either
 * way the shell renders the same `SessionView` shape, and the lifecycle
 * controls act on Session Core rather than on what the window happens to think.
 *
 * The Logs tab is the one surface that reads further than the view model: it
 * asks Session Core for the session's logging state and run history itself
 * (`useSessionLogs`), because nothing else renders them, and it labels the
 * values it had to fall back to the fixture for (T10).
 */
export default function App() {
  const connection = useBackendPing();
  const registry = useSessionRegistry(connection);
  const sessions = registry.sessions;

  // `#session=<id>` deep link (tray/restore surfaces can target a session).
  const [selectedId, setSelectedId] = useState(
    () =>
      initialSelectedSessionId(
        typeof window === "undefined" ? null : window.location.hash,
        FIXTURE_SESSIONS.map((session) => session.config.id),
      ) || DEFAULT_SELECTED_SESSION_ID,
  );
  const [tab, setTab] = useState<WorkspaceTab>("terminal");
  const [query, setQuery] = useState("");
  const [notice, setNotice] = useState<string | null>(null);
  const [now, setNow] = useState(() => new Date());
  const narrow = useMediaQuery("(max-width: 767px)");
  const [drawerOpen, setDrawerOpen] = useState(false);

  useEffect(() => {
    const timer = window.setInterval(() => setNow(new Date()), CLOCK_TICK_MS);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    if (notice === null) return;
    const timer = window.setTimeout(() => setNotice(null), NOTICE_TIMEOUT_MS);
    return () => window.clearTimeout(timer);
  }, [notice]);

  const filtered = useMemo(() => filterSessions(sessions, query), [sessions, query]);
  const groups = useMemo(
    () => groupSessions(filtered, registry.source === "backend" ? [LIVE_GROUP] : FIXTURE_GROUPS),
    [filtered, registry.source],
  );

  // The selection is derived rather than repaired. The workspace can change
  // under it — a deep link naming a fixture session, then a backend whose
  // config has no such id — and the honest rendering of "that session is not
  // here" is the first one that is, not a header for a session the rail cannot
  // show. `selectedId` keeps whatever the user last chose, so a session that
  // comes back is still selected.
  const selected = sessions.find((session) => session.config.id === selectedId) ?? sessions[0];
  const counts = useMemo(() => liveCounts(sessions), [sessions]);

  /** Preview mode: actions render from the real lifecycle rules but perform
   * nothing, because there is no run behind them to act on. */
  const onPreviewAction = (label: string) => {
    setNotice(`预览模式 ·「${label}」需要连接到后端`);
  };

  /**
   * What a session control was asked for.
   *
   * Every one of these is a named Session Core operation (T08 #9 added the two
   * "open" actions); the ones that are not yet — a new session, which needs a
   * config writer that does not exist — say so rather than being a button that
   * appears to work.
   */
  const onSessionAction = (action: SessionAction) => {
    const label = SESSION_ACTION_LABELS[action];
    if (!registry.live) {
      onPreviewAction(label);
      return;
    }
    switch (action) {
      case "start":
        registry.start(selected.config.id);
        break;
      case "stop":
        registry.stop(selected.config.id);
        break;
      case "restart":
        registry.restart(selected.config.id);
        break;
      case "force-stop":
        registry.forceStop(selected.config.id);
        break;
      case "open-url":
        registry.openUrl(selected.config.id);
        break;
      case "open-directory":
        registry.openDirectory(selected.config.id);
        break;
      default:
        setNotice(`「${label}」尚未接入`);
    }
  };

  if (selected === undefined) {
    // Nothing to render yet: the backend has not answered and the fixture
    // workspace is the only other source, so a shell with no sessions means a
    // config with no sessions in it.
    return (
      <div className="app-shell">
        <TitleBar
          summary={titlebarSummaryText({ total: 0, running: 0, busy: 0 })}
          pill={connection.state === "unavailable" ? "preview" : "connected"}
          narrow={narrow}
          drawerOpen={drawerOpen}
          onToggleDrawer={() => setDrawerOpen((open) => !open)}
        />
        <div className="app-main">
          <section className="workspace workspace--empty">
            <p className="workspace__empty-hint">
              没有可显示的会话。配置文件中还没有会话，或后端尚未就绪。
            </p>
          </section>
        </div>
        <StatusBar counts={counts} connection={connection} notice={registry.error ?? notice} />
      </div>
    );
  }

  return (
    <div className="app-shell">
      <TitleBar
        summary={titlebarSummaryText(counts)}
        pill={connection.state === "unavailable" ? "preview" : "connected"}
        narrow={narrow}
        drawerOpen={drawerOpen}
        onToggleDrawer={() => setDrawerOpen((open) => !open)}
      />
      <div className={`app-main${narrow ? " app-main--narrow" : ""}`}>
        {narrow && drawerOpen && (
          <div
            className="app-main__backdrop"
            aria-hidden="true"
            onClick={() => setDrawerOpen(false)}
          />
        )}
        <div className={`app-main__rail${narrow && drawerOpen ? " app-main__rail--open" : ""}`}>
          <Sidebar
            groups={groups}
            selectedId={selected.config.id}
            now={now}
            summary={sidebarSummaryText(counts)}
            query={query}
            onQueryChange={setQuery}
            onSelect={(sessionId) => {
              setSelectedId(sessionId);
              setDrawerOpen(false);
            }}
            onAdd={() => onSessionAction("new-session")}
          />
        </div>
        <section className="workspace">
          <SessionHeader
            config={selected.config}
            runtime={selected.runtime}
            busy={selected.busy ?? false}
            ready={isReady(selected.runtime)}
            now={now}
            onAction={onSessionAction}
            onFocusTerminal={() => setTab("terminal")}
            onOpenLogs={() => setTab("logs")}
          />
          <WorkspaceTabs active={tab} onChange={setTab} />
          <div className="workspace__content">
            {tab === "terminal" && (
              <TerminalHost
                session={selected}
                live={registry.live}
                onStart={() =>
                  registry.live
                    ? registry.start(selected.config.id)
                    : onPreviewAction(`启动 ${selected.config.name}`)
                }
              />
            )}
            {/* Keyed by session so the Logs tab's own state — the pending
                retention question above all — belongs to one session. */}
            {tab === "logs" && (
              <LogsPanel key={selected.config.id} session={selected} onNotice={setNotice} />
            )}
            {tab === "details" && <DetailsPanel session={selected} sessions={sessions} />}
          </div>
        </section>
      </div>
      <StatusBar counts={counts} connection={connection} notice={registry.error ?? notice} />
    </div>
  );
}
