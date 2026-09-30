import { useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { FolderPlus, Terminal } from "lucide-react";
import TitleBar from "../components/title-bar/TitleBar";
import Sidebar from "../components/sidebar/Sidebar";
import SessionHeader from "../components/session-header/SessionHeader";
import WorkspaceTabs from "../components/workspace/WorkspaceTabs";
import TerminalHost from "../components/terminal/TerminalHost";
import StandalonePanel from "../components/workspace/StandalonePanel";
import LogsPanel from "../components/logs/LogsPanel";
import DetailsPanel from "../components/details/DetailsPanel";
import StatusBar from "../components/status-bar/StatusBar";
import ConfigDiagnostics from "../components/config-diagnostics/ConfigDiagnostics";
import AddApplicationDialog from "../components/add-application/AddApplicationDialog";
import type { NewApplicationFormDto } from "../types/config";
import {
  filterSessions,
  groupSessions,
  initialSelectedSessionId,
  isReady,
  isStandalone,
  liveCounts,
  sidebarSummaryText,
  titlebarSummaryText,
} from "../state/derivations";
import { SESSION_ACTION_LABELS, type SessionAction } from "../state/actions";
import { DEFAULT_SELECTED_SESSION_ID, FIXTURE_GROUPS, FIXTURE_SESSIONS } from "../state/fixtures";
import { LIVE_GROUPS } from "../state/session-view";
import type { WorkspaceTab } from "../state/view";
import { SESSION_FOCUS_REQUESTED, isSessionFocusRequestedDto } from "../types/tray";
import { copyPathToClipboard } from "./clipboard";
import { useBackendPing } from "./useBackendPing";
import { useDisplayAdvice } from "./useDisplayAdvice";
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
  // The form's own read-only question (#66), kept apart from the session
  // registry because a launch method that has not been saved is not a session.
  const recommendDisplay = useDisplayAdvice();
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
  /** Bumped for each "新建 PowerShell" the workspace carried out (#62). */
  const [focusRequest, setFocusRequest] = useState(0);
  /** Whether the "添加应用" form is open (#64). */
  const [addApplicationOpen, setAddApplicationOpen] = useState(false);

  useEffect(() => {
    const timer = window.setInterval(() => setNow(new Date()), CLOCK_TICK_MS);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    if (notice === null) return;
    const timer = window.setTimeout(() => setNotice(null), NOTICE_TIMEOUT_MS);
    return () => window.clearTimeout(timer);
  }, [notice]);

  /**
   * The tray's "show me this session" request (T09 #10).
   *
   * A click on a tray row is a request to *look at* a session, not an operation
   * on it, so it arrives as a selection and nothing else: no lifecycle command
   * is implied, and a session name the workspace cannot resolve falls back to
   * the derived selection exactly as a stale deep link does.
   *
   * Only registered once a backend answers. A tray exists only in the desktop
   * app, and in the browser preview there is no IPC channel to listen on — the
   * same reason `useSessionRegistry` registers its own listener there and
   * nowhere else.
   */
  useEffect(() => {
    if (connection.state !== "connected") return;
    const subscription = listen<unknown>(SESSION_FOCUS_REQUESTED, (event) => {
      if (isSessionFocusRequestedDto(event.payload)) {
        setSelectedId(event.payload.sessionId);
      }
    });
    return () => {
      void subscription.then((unlisten) => unlisten());
    };
  }, [connection.state]);

  const filtered = useMemo(() => filterSessions(sessions, query), [sessions, query]);
  const groups = useMemo(
    () => groupSessions(filtered, registry.source === "backend" ? LIVE_GROUPS : FIXTURE_GROUPS),
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
  const diagnosticSessionCount = registry.configReport?.sessions.length ?? sessions.length;
  const statusNotice =
    registry.error ??
    (registry.loading && !registry.initializationError ? "正在同步会话…" : notice);

  /** Preview mode: actions render from the real lifecycle rules but perform
   * nothing, because there is no run behind them to act on. */
  const onPreviewAction = (label: string) => {
    setNotice(`预览模式 ·「${label}」需要连接到后端`);
  };

  /**
   * "新建 PowerShell" (#62): create, select and focus, in one gesture.
   *
   * Nothing is asked for first — no form, no dialog, no default session to
   * copy. The backend resolves the shell (`pwsh`, else Windows PowerShell),
   * opens it in the user's home directory and answers with the new session's
   * id; the workspace selects it and the terminal takes the keyboard, so the
   * next keystroke is already a command (stories 6–8).
   *
   * A failure is a message, not a row: the backend withdraws a terminal it
   * could not start, and what is left is the reason — a shell that is not on
   * this machine, or a directory that is not there (story 17).
   */
  const onCreateTerminal = () => {
    if (!registry.live) {
      onPreviewAction(SESSION_ACTION_LABELS["new-session"]);
      return;
    }
    void registry.createTerminal().then((sessionId) => {
      if (sessionId === null) return;
      setSelectedId(sessionId);
      setTab("terminal");
      setFocusRequest((request) => request + 1);
      setDrawerOpen(false);
    });
  };

  /**
   * "添加应用" (#64): a form, then a saved launch configuration.
   *
   * A preview workspace has no backend to save to and no file to save into, so
   * the entry says that instead of opening a form whose save could only fail.
   * The dialog itself stays open on a refusal — the user's input is the thing
   * they need to fix, and closing would throw it away (story 31).
   */
  const onOpenAddApplication = () => {
    if (!registry.live) {
      onPreviewAction("添加应用");
      return;
    }
    setAddApplicationOpen(true);
    setDrawerOpen(false);
  };

  const onAddApplication = async (form: NewApplicationFormDto) => {
    // The answer goes back untouched on a refusal, so the dialog places the
    // message on the field the backend named; a success closes the form and
    // selects what was added.
    const result = await registry.addApplication(form);
    if (result.ok) {
      setAddApplicationOpen(false);
      setSelectedId(result.sessionId);
      setTab("terminal");
      setNotice(`已保存「${form.name}」，下次打开 Hub 仍然可用`);
    }
    return result;
  };

  /**
   * What a session control was asked for.
   *
   * Every one of these but two is a named Session Core operation (T08 #9 added
   * the two "open" actions); a new session needs a config writer that does not
   * exist yet, so it says so rather than being a button that appears to work.
   *
   * 复制路径 is the other exception, and the only action that reads nothing from
   * the backend: the path is already in the config this header renders. It is
   * answered before the connection check so the preview workspace can serve it
   * too — what it copies is exactly what the metadata line is showing. The gate
   * that matters is `availableActions`': a session with no `cwd` is never
   * offered the control.
   */
  /**
   * Open a session, and say what happened to an application's own window.
   *
   * `activate` answers with the window step as well as the lifecycle one
   * (#66), and the step is the only thing here the events do not repeat: an
   * application that is running with no window to bring forward, or a Windows
   * refusal, is reported once and nowhere else. Everything else about the open
   * still arrives as a session event.
   */
  const activate = (sessionId: string) => {
    void registry.activate(sessionId).then((outcome) => {
      const notice = outcome?.window?.notice;
      if (notice) setNotice(notice);
    });
  };

  const onSessionAction = (action: SessionAction) => {
    const label = SESSION_ACTION_LABELS[action];
    if (action === "copy-path") {
      if (selected.config.cwd === undefined) {
        setNotice("该会话未配置工作目录，没有可复制的路径");
        return;
      }
      copyPathToClipboard(selected.config.cwd, setNotice);
      return;
    }
    if (!registry.live) {
      onPreviewAction(label);
      return;
    }
    switch (action) {
      // Opening, not starting (#64): the activation semantics are what keep a
      // second click from creating a second run of the same application.
      case "start":
        activate(selected.config.id);
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
      case "remove-session":
        registry.removeSession(selected.config.id);
        break;
      default:
        setNotice(`「${label}」尚未接入`);
    }
  };

  /** The form, in whichever branch of the shell is on screen. */
  const addApplicationDialog = addApplicationOpen ? (
    <AddApplicationDialog
      onSubmit={onAddApplication}
      onRecommendDisplay={recommendDisplay}
      onClose={() => setAddApplicationOpen(false)}
    />
  ) : null;

  if (selected === undefined) {
    // There is no selected session while the live registry is initializing,
    // or when the validated workspace is empty. Initialization has its own
    // status so this is not mistaken for an empty config.
    return (
      <div className="app-shell">
        <TitleBar
          summary={titlebarSummaryText({ total: 0, running: 0, busy: 0 })}
          pill={connection.state === "unavailable" ? "preview" : "connected"}
          narrow={narrow}
          drawerOpen={drawerOpen}
          onToggleDrawer={() => setDrawerOpen((open) => !open)}
          onNotice={setNotice}
        />
        <div className="app-main">
          <section className="workspace workspace--empty">
            {registry.loading && (
              <div
                className={`workspace__registry-status${registry.initializationError ? " workspace__registry-status--error" : ""}`}
                role={registry.initializationError ? "alert" : "status"}
                aria-label="会话同步状态"
              >
                <div className="workspace__registry-status-heading">
                  <span
                    className={`pip ${registry.initializationError ? "pip--err" : "pip--warn"}`}
                    aria-hidden="true"
                  />
                  <strong>
                    {registry.initializationError ? "暂时无法读取会话状态" : "正在同步会话…"}
                  </strong>
                </div>
                <p>
                  {registry.initializationError
                    ? `${registry.initializationError} · 正在自动重试。`
                    : "正在连接到后台并读取已配置会话。"}
                </p>
              </div>
            )}
            <ConfigDiagnostics
              report={registry.configReport}
              error={registry.configReportError}
              sessionCount={diagnosticSessionCount}
              empty={!registry.loading}
            />
            {/* An empty workspace is a workspace (#62, story 7): the quick
                entry is offered here too, so a first run with no config file
                can still open a terminal. It is deliberately not rendered
                while the registry is still syncing — a session may be about to
                arrive, and a button that claimed there was nothing yet would
                be answering a question the window cannot. */}
            {!registry.loading && (
              <div className="workspace__quick-entry">
                <p className="workspace__quick-entry-title">
                  {registry.live ? "还没有会话" : "预览工作区"}
                </p>
                <p className="workspace__quick-entry-hint">
                  点击“新建 PowerShell”立即在 Hub 内打开一个临时终端，不需要填写配置；
                  长期使用的服务用“添加应用”保存启动方式。
                </p>
                <div className="workspace__quick-entry-actions">
                  <button
                    type="button"
                    className="btn btn--primary btn--sm"
                    onClick={onCreateTerminal}
                  >
                    <Terminal size={14} />
                    新建 PowerShell
                  </button>
                  <button
                    type="button"
                    className="btn btn--secondary btn--sm"
                    onClick={onOpenAddApplication}
                  >
                    <FolderPlus size={14} />
                    添加应用
                  </button>
                </div>
              </div>
            )}
          </section>
        </div>
        <StatusBar counts={counts} connection={connection} notice={statusNotice} />
        {addApplicationDialog}
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
        onNotice={setNotice}
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
            onAdd={onCreateTerminal}
            onAddApplication={onOpenAddApplication}
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
          <ConfigDiagnostics
            report={registry.configReport}
            error={registry.configReportError}
            sessionCount={diagnosticSessionCount}
          />
          <WorkspaceTabs active={tab} onChange={setTab} />
          <div className="workspace__content">
            {tab === "terminal" &&
              // A standalone-window application has no Hub-side stream to
              // render (#66): the pane says where its console is instead of
              // drawing an empty terminal for a console the Hub does not own.
              (isStandalone(selected.config) ? (
                <StandalonePanel
                  session={selected}
                  onActivate={() =>
                    registry.live
                      ? activate(selected.config.id)
                      : onPreviewAction(`打开 ${selected.config.name}`)
                  }
                />
              ) : (
                <TerminalHost
                  session={selected}
                  live={registry.live}
                  focusRequest={focusRequest}
                  onStart={() =>
                    registry.live
                      ? activate(selected.config.id)
                      : onPreviewAction(`启动 ${selected.config.name}`)
                  }
                />
              ))}
            {/* Keyed by session so the Logs tab's own state — the pending
                retention question above all — belongs to one session. */}
            {tab === "logs" && (
              <LogsPanel key={selected.config.id} session={selected} onNotice={setNotice} />
            )}
            {tab === "details" && <DetailsPanel session={selected} sessions={sessions} />}
          </div>
        </section>
      </div>
      <StatusBar counts={counts} connection={connection} notice={statusNotice} />
      {addApplicationDialog}
    </div>
  );
}
