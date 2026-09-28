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
  liveCounts,
  sidebarSummaryText,
  titlebarSummaryText,
} from "../state/derivations";
import {
  DEFAULT_SELECTED_SESSION_ID,
  FIXTURE_GROUPS,
  FIXTURE_SESSIONS,
  type FixtureSession,
} from "../state/fixtures";
import type { WorkspaceTab } from "../state/view";
import { useBackendPing } from "./useBackendPing";
import { useMediaQuery } from "./useMediaQuery";
import "./App.css";

const NOTICE_TIMEOUT_MS = 4000;
/** Uptime text shows minutes and seconds; a slow tick keeps it fresh without
 * turning the whole shell into a per-second renderer (spec §14). */
const CLOCK_TICK_MS = 5000;

/**
 * The V2 workspace shell (T06 #7).
 *
 * Structure per docs/UI_STYLE_GUIDE.md: compact title bar, grouped session
 * sidebar, selected-session workspace (header + 终端/日志/详情 tabs, terminal
 * dominant), minimal status bar. Data is fixture-driven until the runtime
 * tickets land; every fixture value already passes the landed DTO guards, so
 * swapping in live snapshots is a data change only.
 */
export default function App() {
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
  const connection = useBackendPing();

  useEffect(() => {
    const timer = window.setInterval(() => setNow(new Date()), CLOCK_TICK_MS);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    if (notice === null) return;
    const timer = window.setTimeout(() => setNotice(null), NOTICE_TIMEOUT_MS);
    return () => window.clearTimeout(timer);
  }, [notice]);

  const filtered = useMemo(() => filterSessions(FIXTURE_SESSIONS, query), [query]);
  const groups = useMemo(() => groupSessions(filtered, FIXTURE_GROUPS), [filtered]);
  const selected: FixtureSession =
    FIXTURE_SESSIONS.find((session) => session.config.id === selectedId) ?? FIXTURE_SESSIONS[0];
  const counts = useMemo(() => liveCounts(FIXTURE_SESSIONS), []);

  /** Fixture mode: actions render from real lifecycle rules but perform
   * nothing — the runtime tickets (T07/T08) wire them to Session Core. */
  const onFixtureAction = (label: string) => {
    setNotice(`Fixture 预览 ·「${label}」将在运行时接入后生效`);
  };

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
            onAdd={() => onFixtureAction("新建会话")}
          />
        </div>
        <section className="workspace">
          <SessionHeader
            config={selected.config}
            runtime={selected.runtime}
            busy={selected.busy ?? false}
            ready={selected.ready ?? false}
            now={now}
            onAction={onFixtureAction}
            onFocusTerminal={() => setTab("terminal")}
            onOpenLogs={() => setTab("logs")}
          />
          <WorkspaceTabs active={tab} onChange={setTab} />
          <div className="workspace__content">
            {tab === "terminal" && <TerminalHost fixture={selected} onAction={onFixtureAction} />}
            {tab === "logs" && <LogsPanel fixture={selected} onAction={onFixtureAction} />}
            {tab === "details" && <DetailsPanel fixture={selected} />}
          </div>
        </section>
      </div>
      <StatusBar counts={counts} connection={connection} notice={notice} />
    </div>
  );
}
