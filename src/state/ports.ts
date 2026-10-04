/**
 * Port-page rules that do not need a window (#98).
 *
 * Search, the empty result, and whether a check may be described as just
 * finished all live here so the node tests can hold them. The React view only
 * renders what this module already decided. Nothing here is a sample listener.
 */

import type { ListenerAttribution, ListenerRowDto } from "../types/listen";

/** The words the backend uses for the two readings that are not a session. */
export const EXTERNAL_LABEL = "外部";
export const UNAVAILABLE_LABEL = "信息不可用";

/**
 * What a UDP row has to say. A bound UDP socket is not evidence that traffic
 * is flowing.
 */
export const UDP_SOCKET_NOTE = "UDP 只说明有这个套接字，不说明正在收发数据。";

export type SidebarView = "sessions" | "ports";

export interface SessionName {
  id: string;
  name: string;
}

/** One listener after the live session list has supplied its name. */
export interface ListedPort {
  key: string;
  protocol: "TCP" | "UDP";
  address: string;
  port: number;
  pid: number | null;
  processName: string | null;
  programPath: string | null;
  attribution: ListenerAttribution;
  sessionId: string | null;
  sessionName: string | null;
}

/** Last successful list, and whether the latest attempt may be called current. */
export interface ListenRefreshState {
  rows: ListenerRowDto[];
  checkedAtMs: number | null;
  failure: string | null;
  inProgress: boolean;
  /**
   * A result arrived while this view was not allowed to treat it as a check
   * that just finished — the window was hidden, or the ports page was not
   * showing. The rows and `checkedAtMs` are still the previous accepted check.
   */
  stale: boolean;
}

export type ListenAttempt =
  { ok: true; checkedAtMs: number | null; rows: ListenerRowDto[] } | { ok: false; failure: string };

export function initialListenRefresh(): ListenRefreshState {
  return { rows: [], checkedAtMs: null, failure: null, inProgress: false, stale: false };
}

export function beginListenRefresh(state: ListenRefreshState): ListenRefreshState {
  return { ...state, inProgress: true };
}

/**
 * Fold one finished attempt the way `listen::RefreshState` does.
 *
 * Success replaces the list and its time, and clears the failure. Failure
 * records the error and leaves the previous success and its time alone, so
 * that list is not a check that just finished. A success that is not accepted
 * — the window was hidden, or the ports page was not showing — does not become
 * the current check either.
 */
export function completeListenRefresh(
  state: ListenRefreshState,
  attempt: ListenAttempt,
  acceptAsCurrent: boolean,
): ListenRefreshState {
  if (!attempt.ok) {
    return { ...state, inProgress: false, failure: attempt.failure };
  }
  if (!acceptAsCurrent || attempt.checkedAtMs === null) {
    return {
      ...state,
      inProgress: false,
      stale: true,
      failure:
        attempt.checkedAtMs === null && acceptAsCurrent ? "检查没有带上完成时间" : state.failure,
    };
  }
  return {
    rows: attempt.rows,
    checkedAtMs: attempt.checkedAtMs,
    failure: null,
    inProgress: false,
    stale: false,
  };
}

/**
 * The window went away. The list and its time stay the last accepted check.
 *
 * An attempt still in flight is not a check that finished while the window
 * was hidden, so it must not remain "in progress" either. The next accepted
 * success is what clears `stale`.
 */
export function noteWindowHidden(state: ListenRefreshState): ListenRefreshState {
  if (state.stale && !state.inProgress) return state;
  return { ...state, inProgress: false, stale: true };
}

/**
 * Automatic polling runs only while the ports page is showing, the window is
 * visible, and a backend is answering. A manual refresh is a separate call.
 */
export function shouldPollPorts(
  view: SidebarView,
  windowVisible: boolean,
  connected: boolean,
): boolean {
  return view === "ports" && windowVisible && connected;
}

/**
 * Whether a finished attempt may replace the current check.
 *
 * A manual refresh still runs when the page is open. It becomes the current
 * check only if the ports page is still showing and the window is still
 * visible when the attempt returns. A result from the hidden period is not.
 */
export function acceptListenResult(input: {
  manual: boolean;
  portsView: boolean;
  visibleAtStart: boolean;
  visibleNow: boolean;
}): boolean {
  if (!input.portsView || !input.visibleNow) return false;
  return input.manual || input.visibleAtStart;
}

export function portKey(
  row: Pick<ListenerRowDto, "protocol" | "address" | "port" | "pid" | "processName">,
): string {
  return [row.protocol, row.address, String(row.port), row.pid ?? "", row.processName ?? ""].join(
    "\u0000",
  );
}

/** Attach each managed row's session name from the live session list. */
export function nameListeners(
  rows: readonly ListenerRowDto[],
  sessions: readonly SessionName[],
): ListedPort[] {
  const names = new Map(sessions.map((session) => [session.id, session.name]));
  return rows.map((row) => ({
    key: portKey(row),
    protocol: row.protocol,
    address: row.address,
    port: row.port,
    pid: row.pid,
    processName: row.processName,
    programPath: row.programPath,
    attribution: row.attribution,
    sessionId: row.attribution === "session" ? row.sessionId : null,
    sessionName:
      row.attribution === "session" && row.sessionId !== null
        ? (names.get(row.sessionId) ?? null)
        : null,
  }));
}

/**
 * Filter by port, process name, and session name.
 *
 * A query that matches nothing returns an empty list. It does not return the
 * rows from the previous query.
 */
export function filterPorts(rows: readonly ListedPort[], query: string): ListedPort[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return [...rows];
  return rows.filter((row) => {
    const processName = row.processName ?? "";
    const sessionName = row.sessionName ?? "";
    return (
      String(row.port).includes(needle) ||
      processName.toLowerCase().includes(needle) ||
      sessionName.toLowerCase().includes(needle)
    );
  });
}

/** A text field from the listener. Blank is the same as not read. */
export function readableText(value: string | null | undefined): string {
  const trimmed = value?.trim() ?? "";
  return trimmed ? trimmed : UNAVAILABLE_LABEL;
}

export function processLabel(name: string | null): string {
  return readableText(name);
}

export function pathLabel(path: string | null): string {
  return readableText(path);
}

export function pidLabel(pid: number | null): string {
  return pid === null ? UNAVAILABLE_LABEL : String(pid);
}

export function ownerLabel(
  row: Pick<ListedPort, "attribution" | "sessionId" | "sessionName">,
): string {
  if (row.attribution === "session") {
    return row.sessionName ?? row.sessionId ?? UNAVAILABLE_LABEL;
  }
  if (row.attribution === "external") return EXTERNAL_LABEL;
  return UNAVAILABLE_LABEL;
}

/**
 * The session a row may open. External and unavailable never qualify, and a
 * session id that is not in the live list has nothing to select.
 */
export function openableSessionId(
  row: Pick<ListedPort, "attribution" | "sessionId">,
  sessions: readonly SessionName[],
): string | null {
  if (row.attribution !== "session" || row.sessionId === null) return null;
  return sessions.some((session) => session.id === row.sessionId) ? row.sessionId : null;
}

export function udpNote(protocol: ListedPort["protocol"]): string | null {
  return protocol === "UDP" ? UDP_SOCKET_NOTE : null;
}

export interface PortGroup {
  id: "managed" | "external" | "unavailable";
  title: string;
  hint: string;
  rows: ListedPort[];
}

/**
 * Three readings, never folded together. An unavailable row is not external,
 * and a group with no rows is left out.
 */
export function groupListedPorts(rows: readonly ListedPort[]): PortGroup[] {
  const managed = rows.filter((row) => row.attribution === "session");
  const external = rows.filter((row) => row.attribution === "external");
  const unavailable = rows.filter((row) => row.attribution === "unavailable");
  const groups: PortGroup[] = [];
  if (managed.length > 0) {
    groups.push({ id: "managed", title: "受管", hint: "对上了会话", rows: managed });
  }
  if (external.length > 0) {
    groups.push({ id: "external", title: "外部", hint: "Hub 以外", rows: external });
  }
  if (unavailable.length > 0) {
    groups.push({
      id: "unavailable",
      title: UNAVAILABLE_LABEL,
      hint: "不猜测归属",
      rows: unavailable,
    });
  }
  return groups;
}

export function portSummary(rows: readonly ListedPort[]): string {
  const managed = rows.filter((row) => row.attribution === "session").length;
  const external = rows.filter((row) => row.attribution === "external").length;
  return `${rows.length} 监听 · ${managed} 受管 · ${external} 外部`;
}

/**
 * What the sidebar says when the filtered list is empty.
 *
 * A search with no match says so even when an unfiltered list exists. A failed
 * check that never succeeded does not claim that nothing is listening.
 */
export function portListMessage(
  filtered: readonly ListedPort[],
  query: string,
  refresh: ListenRefreshState,
): string | null {
  if (filtered.length > 0) return null;
  if (query.trim()) return "没有匹配的端口。";
  if (refresh.inProgress && refresh.rows.length === 0) return "正在检查…";
  if (refresh.failure && refresh.rows.length === 0) return "这次检查没有得到列表。";
  if (refresh.checkedAtMs !== null && refresh.rows.length === 0 && refresh.failure === null) {
    return "没有正在监听的端口。";
  }
  if (refresh.failure === null && refresh.checkedAtMs === null) return "尚未检查。";
  return null;
}

export function selectedPort(rows: readonly ListedPort[], key: string | null): ListedPort | null {
  if (rows.length === 0) return null;
  return rows.find((row) => row.key === key) ?? rows[0];
}

export function formatCheckTime(ms: number): string {
  const date = new Date(ms);
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}

export interface CheckCaption {
  text: string;
  /** True only for an accepted success that is still the current check. */
  justChecked: boolean;
}

/**
 * How the header describes the last attempt.
 *
 * A failure keeps the previous success time and says the check failed.
 * A hidden-window or otherwise stale result is not a check that just finished.
 */
export function checkCaption(state: ListenRefreshState, now: number): CheckCaption {
  if (state.inProgress) {
    return { text: "正在检查…", justChecked: false };
  }
  if (state.failure) {
    const previous =
      state.checkedAtMs === null
        ? "还没有成功的检查"
        : `上次成功 ${formatCheckTime(state.checkedAtMs)}`;
    return { text: `检查失败 · ${state.failure} · ${previous}`, justChecked: false };
  }
  if (state.stale) {
    const previous =
      state.checkedAtMs === null
        ? "窗口隐藏期间的结果没有记成一次完成的检查"
        : `窗口隐藏期间暂停 · 上次检查 ${formatCheckTime(state.checkedAtMs)}`;
    return { text: previous, justChecked: false };
  }
  if (state.checkedAtMs === null) {
    return { text: "尚未检查", justChecked: false };
  }
  const age = now - state.checkedAtMs;
  return {
    text: `最近检查 ${formatCheckTime(state.checkedAtMs)}`,
    justChecked: age >= 0 && age < 5_000,
  };
}
