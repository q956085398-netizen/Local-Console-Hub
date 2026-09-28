/**
 * Pure view derivations for the V2 workspace (T06 #7).
 *
 * Every rule the shell renders by — status tones and labels, action
 * availability, callout wording, metadata pairs, sidebar rows — lives here
 * as a pure function of the DTOs (plus the documented fixture extras), so
 * components stay thin and the rules are unit-tested without a DOM. The
 * wording mirrors the approved V2 prototype (docs/DESIGN_SPEC_EXTRACTED.md);
 * when T07–T10 replace fixtures with live snapshots, these functions keep
 * operating on the same shapes.
 */

import type { EffectiveLoggingDto, SessionConfigDto } from "../types/config";
import type {
  RunRecordDto,
  RuntimeEffectiveLoggingDto,
  SessionRuntimeDto,
  SessionStatusValue,
} from "../types/runtime";
import type { FixtureGroup, FixtureSession } from "./fixtures";

/** Pip/badge tone semantics (UI_STYLE_GUIDE §10). */
export type StatusTone = "run" | "busy" | "warn" | "err" | "idle";

/** Is the session mid-lifecycle (anything but fully stopped)? */
export function isLive(status: SessionStatusValue): boolean {
  return status === "running" || status === "starting" || status === "stopping";
}

/** Map lifecycle state (plus the busy runtime flag, spec §4) to a tone. */
export function statusTone(status: SessionStatusValue, busy = false): StatusTone {
  if (status === "error") return "err";
  if (status === "starting" || status === "stopping") return "warn";
  if (status === "running" && busy) return "busy";
  if (status === "running") return "run";
  return "idle";
}

/** Status badge / row-tail label; `Ready`/`Busy` supplement, never replace. */
export function statusLabel(status: SessionStatusValue, busy = false, ready = false): string {
  if (status === "running" && busy) return "Busy";
  if (status === "running" && ready) return "Ready";
  switch (status) {
    case "stopped":
      return "Stopped";
    case "starting":
      return "Starting";
    case "running":
      return "Running";
    case "stopping":
      return "Stopping";
    case "exited":
      return "Exited";
    case "error":
      return "Error";
  }
}

/** `Service` / `Terminal`. */
export function typeLabel(type: SessionConfigDto["sessionType"]): string {
  return type === "service" ? "Service" : "Terminal";
}

/** Elapsed run duration in the reference's vocabulary. */
export function formatDuration(startedAt: string, now: Date): string {
  const started = Date.parse(startedAt);
  if (Number.isNaN(started)) {
    return "—";
  }
  const sec = Math.max(0, Math.floor((now.getTime() - started) / 1000));
  const h = Math.floor(sec / 3600);
  const m = Math.floor((sec % 3600) / 60);
  const s = sec % 60;
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m ${s}s`;
  return `${s}s`;
}

/** App-wide counts: running, busy (running + busy flag) and total. */
export interface LiveCounts {
  total: number;
  running: number;
  busy: number;
}

/** Count the live summary over sessions carrying runtime + busy flag. */
export function liveCounts(sessions: FixtureSession[]): LiveCounts {
  let running = 0;
  let busy = 0;
  for (const session of sessions) {
    if (session.runtime.status === "running") {
      running += 1;
      if (session.busy) busy += 1;
    }
  }
  return { total: sessions.length, running, busy };
}

/** Sidebar-header summary: `2 运行 · 1 忙碌 · 6 会话`. */
export function sidebarSummaryText(counts: LiveCounts): string {
  const busy = counts.busy ? ` · ${counts.busy} 忙碌` : "";
  return `${counts.running} 运行${busy} · ${counts.total} 会话`;
}

/** Title-bar summary: `2 运行 · 1 busy`. */
export function titlebarSummaryText(counts: LiveCounts): string {
  const busy = counts.busy ? ` · ${counts.busy} busy` : "";
  return `${counts.running} 运行${busy}`;
}

/**
 * Which header actions exist, derived from lifecycle truth (spec §5).
 *
 * Components render this verbatim — the lifecycle rules live here, once, so
 * a button's enablement cannot drift from the state machine.
 */
export interface ActionAvailability {
  /** Start is offered while idle; Stop while live. Never both at once. */
  start: boolean;
  stop: boolean;
  /** Stop stays visible but inert while the session is already unwinding. */
  stopDisabled: boolean;
  /** Restart cannot launch a replacement until the previous run is gone
   * (spec §5 rule 2), so it is offered only from a settled state. */
  restart: boolean;
  /** Open the configured URL (service with a URL). */
  openUrl?: string;
  /** Open the working directory. */
  directory: boolean;
  /** Force-kill the managed tree — a separate, explicit action (D-007).
   * Available for the whole live window, including Stopping: skipping the
   * grace period is exactly what this action is for. */
  forceStop: boolean;
}

/** Derive the header action set for the current lifecycle state. */
export function availableActions(
  config: SessionConfigDto,
  runtime: SessionRuntimeDto,
): ActionAvailability {
  const idle = !isLive(runtime.status);
  const settled = runtime.status === "running" || idle;
  return {
    start: idle,
    stop: isLive(runtime.status),
    stopDisabled: runtime.status === "stopping",
    restart: settled,
    openUrl: config.sessionType === "service" ? config.url : undefined,
    directory: true,
    forceStop: isLive(runtime.status),
  };
}

/**
 * The terminal panel's connection strip.
 *
 * Reads `ptyAttached` rather than the session type: UI_STYLE_GUIDE §7 calls
 * the terminal "a real PTY view, not a read-only log box", so the surface
 * must not claim an attachment the snapshot does not report.
 */
export function ptyChromeLabel(config: SessionConfigDto, runtime: SessionRuntimeDto): string {
  if (!runtime.ptyAttached) {
    return config.sessionType === "terminal" ? "ConPTY · 未连接" : "PTY 未连接 · 只读缓冲";
  }
  return config.sessionType === "terminal" ? "ConPTY · interactive" : "PTY attached · stdin 可用";
}

/**
 * Scrollback-loss notice, or null when nothing was discarded
 * (`docs/LOGGING.md` §8: discarded output is counted and surfaced so the UI
 * can say "older output was dropped" instead of showing a gapped history).
 */
export function bufferDiscardNotice(runtime: SessionRuntimeDto): string | null {
  if (runtime.buffer.droppedBytes <= 0) {
    return null;
  }
  return `更早的输出已被丢弃（${runtime.buffer.droppedBytes} B）`;
}

/** Sessions this one depends on, resolved from the fixture workspace. */
export function dependenciesOf(session: FixtureSession, all: FixtureSession[]): FixtureSession[] {
  return (session.dependsOn ?? [])
    .map((id) => all.find((candidate) => candidate.config.id === id))
    .filter((candidate): candidate is FixtureSession => candidate !== undefined);
}

/** Badge label + tone for a run record's outcome. */
export function runOutcomeBadge(run: RunRecordDto): { label: string; tone: StatusTone } {
  const outcome = runOutcome(run);
  return {
    label: outcome,
    tone: outcome === "running" ? "run" : outcome === "error" ? "err" : "idle",
  };
}

/** Badge label + tone for the effective logging policy headline. */
export function logPolicyBadge(
  logging: RuntimeEffectiveLoggingDto | EffectiveLoggingDto,
  status: SessionStatusValue,
): { label: string; tone: StatusTone } {
  const capturing =
    status === "running" &&
    logging.source === "captured" &&
    (logging.mode === "always" || logging.mode === "manual");
  if (capturing) {
    return { label: "Capturing", tone: "run" };
  }
  return { label: logModeLabel(logging.mode), tone: logging.mode === "off" ? "idle" : "warn" };
}

/** The header callout: close impact while live, last error otherwise. */
export interface HeaderCallout {
  kind: "impact" | "error";
  title: string;
  text: string;
}

/** Pick and word the header callout for the current state. */
export function headerCallout(
  config: SessionConfigDto,
  runtime: SessionRuntimeDto,
): HeaderCallout | null {
  if (isLive(runtime.status)) {
    return { kind: "impact", title: "关闭影响", text: config.closeImpact ?? "—" };
  }
  if (runtime.lastError != null) {
    return { kind: "error", title: "上次错误", text: runtime.lastError.message };
  }
  return null;
}

/** Human label of an effective log mode. */
export function logModeLabel(mode: RuntimeEffectiveLoggingDto["mode"]): string {
  switch (mode) {
    case "off":
      return "Off";
    case "always":
      return "Always";
    case "on_error":
      return "On error";
    case "manual":
      return "Manual";
  }
}

/** Human label of a log source. */
export function logSourceLabel(source: RuntimeEffectiveLoggingDto["source"]): string {
  switch (source) {
    case "none":
      return "None";
    case "captured":
      return "Hub captured";
    case "external":
      return "External";
  }
}

/** One-line answer to "is this being logged?" (Logs pane headline). */
export function loggingHeadline(logging: RuntimeEffectiveLoggingDto | EffectiveLoggingDto): string {
  if (logging.source === "external") return "关联应用日志，不重复捕获";
  if (logging.mode === "off") return "仅内存缓冲，不写磁盘";
  if (logging.mode === "always") return "每次运行都保存 stdout/stderr";
  if (logging.mode === "on_error") return "异常退出时才落盘";
  return "需手动开始记录";
}

/** Compact header metadata: label/value pairs in the reference's shape. */
export function metadataPairs(
  config: SessionConfigDto,
  runtime: SessionRuntimeDto,
  now: Date,
): Array<{ label: string; value: string }> {
  const logging = runtime.logging ?? config.logging;
  const logValue =
    logging.source === "none"
      ? "buffer only"
      : logging.source === "external"
        ? "external"
        : logModeLabel(logging.mode);
  const pairs: Array<{ label: string; value: string }> = [
    { label: "PID", value: runtime.pid != null ? String(runtime.pid) : "—" },
  ];
  if (config.port !== undefined) pairs.push({ label: "port", value: `:${config.port}` });
  if (runtime.startedAt != null && runtime.status === "running") {
    pairs.push({ label: "up", value: formatDuration(runtime.startedAt, now) });
  }
  pairs.push({ label: "cwd", value: config.cwd ?? "—" });
  pairs.push({ label: "log", value: logValue });
  return pairs;
}

/** One chip in a sidebar row's meta line. */
export interface RowMetaChip {
  text: string;
  tone?: "busy" | "err";
}

/** Sidebar row meta: `:8000 · svc` or `interactive · tty`, plus flags.
 *
 * The logging chip reports the *effective* policy: the mode when one is in
 * effect, otherwise `External` for an application-owned log. The prototype's
 * `Auto` label cannot appear here — `auto` resolves before the frontend
 * (src/types/config.ts), so the row states what is actually happening. */
export function sidebarRowMeta(session: FixtureSession): RowMetaChip[] {
  const chips: RowMetaChip[] = [];
  if (session.config.sessionType === "service") {
    chips.push({ text: session.config.port !== undefined ? `:${session.config.port}` : "service" });
    chips.push({ text: "svc" });
  } else {
    chips.push({ text: "interactive" });
    chips.push({ text: "tty" });
  }
  if (session.runtime.status === "running" && session.busy) {
    chips.push({ text: "busy", tone: "busy" });
  }
  if (session.runtime.status === "error") {
    chips.push({ text: "error", tone: "err" });
  }
  // Live state wins once a run exists (LOGGING.md §1.4): a `manual` run that
  // has started recording must show as such in the rail, not as its config.
  const logging = session.runtime.logging ?? session.config.logging;
  if (logging.mode !== "off") {
    chips.push({ text: logModeLabel(logging.mode) });
  } else if (logging.source === "external") {
    chips.push({ text: "External" });
  }
  return chips;
}

/**
 * Outcome of a run record for the history list.
 *
 * `== null`, not `=== undefined`: an unfinished run arrives with `endedAt` as
 * `null` (see the note in `types/runtime.ts`), and reading that as an ended run
 * would file every live run under "error".
 */
export function runOutcome(run: RunRecordDto): "running" | "ok" | "error" {
  if (run.endedAt == null) return "running";
  return run.exitCode === 0 ? "ok" : "error";
}

/** A sidebar workload group. */
export interface SessionGroup {
  group: FixtureGroup;
  items: FixtureSession[];
}

/** Group sessions by workload in the fixture groups' declared order. */
export function groupSessions(
  sessions: FixtureSession[],
  groups: readonly FixtureGroup[],
): SessionGroup[] {
  return groups
    .map((group) => ({
      group,
      items: sessions.filter((session) => session.group === group.id),
    }))
    .filter((entry) => entry.items.length > 0);
}

/** Filter sessions by name, purpose, port, id or type, case-insensitively. */
export function filterSessions(sessions: FixtureSession[], rawQuery: string): FixtureSession[] {
  const query = rawQuery.trim().toLowerCase();
  if (!query) return sessions;
  return sessions.filter((session) => {
    const { config } = session;
    return (
      config.name.toLowerCase().includes(query) ||
      (config.purpose?.toLowerCase().includes(query) ?? false) ||
      (config.port !== undefined && String(config.port).includes(query)) ||
      config.id.toLowerCase().includes(query) ||
      config.sessionType.includes(query)
    );
  });
}

/**
 * Resolve the initially selected session from a `#session=<id>` deep link,
 * falling back to the first known session. Unknown ids fall back rather than
 * render an empty workspace; `null` hash input keeps this testable.
 */
export function initialSelectedSessionId(hash: string | null, ids: readonly string[]): string {
  const match = /^#session=([A-Za-z0-9_-]+)$/.exec(hash ?? "");
  const wanted = match?.[1];
  if (wanted !== undefined && ids.includes(wanted)) {
    return wanted;
  }
  return ids[0] ?? "";
}
