import { useState } from "react";
import { ChevronRight, FolderPlus, Plus, Search, Terminal } from "lucide-react";
import {
  formatDuration,
  isReady,
  sidebarRowMeta,
  statusLabel,
  statusTone,
  type SessionGroup,
} from "../../state/derivations";
import {
  ownerLabel,
  pathLabel,
  pidLabel,
  processLabel,
  UNAVAILABLE_LABEL,
  type PortGroup,
  type SidebarView,
} from "../../state/ports";
import type { SessionView } from "../../state/session-view";
import { isPresent } from "../../types/runtime";
import "./Sidebar.css";

/**
 * Whether a session group shows its rows (#113).
 *
 * `collapsed` is the groups the user folded. An id that is absent is expanded,
 * which is how the rail starts. Search is applied before these groups arrive,
 * so a group that still has rows under a non-empty query contains a match and
 * opens for that search only. The set is not rewritten; an empty query reads
 * it again, and a query that matches nothing in the group leaves it folded.
 */
export function sessionGroupExpanded(
  collapsed: ReadonlySet<string>,
  groupId: string,
  query: string,
  rowCount: number,
): boolean {
  if (query.trim() !== "" && rowCount > 0) return true;
  return !collapsed.has(groupId);
}

/**
 * The collapse set after the group title is activated.
 *
 * The choice follows the rows on screen. A search can hold a folded group
 * open; the title then already reads as expanded, so activating it records
 * collapsed instead of toggling the stored choice back to expanded.
 */
export function sessionGroupAfterTitleToggle(
  collapsed: ReadonlySet<string>,
  groupId: string,
  query: string,
  rowCount: number,
): Set<string> {
  const next = new Set(collapsed);
  if (sessionGroupExpanded(collapsed, groupId, query, rowCount)) next.add(groupId);
  else next.delete(groupId);
  return next;
}

/** Accessible name for a session-group title: the group, and whether it is open. */
export function sessionGroupToggleName(label: string, expanded: boolean): string {
  return `${label}，${expanded ? "已展开" : "已折叠"}`;
}

export interface SidebarProps {
  groups: SessionGroup[];
  selectedId: string;
  now: Date;
  summary: string;
  query: string;
  onQueryChange: (query: string) => void;
  onSelect: (sessionId: string) => void;
  /** The quick entry (#62): one click, one interactive terminal. */
  onAdd: () => void;
  /** The secondary entry (#64): a form, then a saved application. */
  onAddApplication: () => void;
  /** Which list the rail is showing. Defaults to the managed-session list. */
  view?: SidebarView;
  onViewChange?: (view: SidebarView) => void;
  portSummary?: string;
  portQuery?: string;
  onPortQueryChange?: (query: string) => void;
  portGroups?: PortGroup[];
  selectedPortKey?: string;
  onSelectPort?: (key: string) => void;
  portEmpty?: string | null;
}

/** The compact grouped session rail (UI_STYLE_GUIDE §4). */
export default function Sidebar({
  groups,
  selectedId,
  now,
  summary,
  query,
  onQueryChange,
  onSelect,
  onAdd,
  onAddApplication,
  view = "sessions",
  onViewChange,
  portSummary = "",
  portQuery = "",
  onPortQueryChange,
  portGroups = [],
  selectedPortKey = "",
  onSelectPort,
  portEmpty = null,
}: SidebarProps) {
  const ports = view === "ports";
  const [collapsedGroups, setCollapsedGroups] = useState<ReadonlySet<string>>(() => new Set());
  return (
    <div className="sidebar">
      <div className="sidebar__head">
        <div>
          <div className="sidebar__switch" role="group" aria-label="侧栏视图">
            <button
              type="button"
              className={`sidebar__switch-btn${ports ? "" : " sidebar__switch-btn--current"}`}
              aria-pressed={!ports}
              onClick={() => onViewChange?.("sessions")}
            >
              受管会话
            </button>
            <button
              type="button"
              className={`sidebar__switch-btn${ports ? " sidebar__switch-btn--current" : ""}`}
              aria-pressed={ports}
              onClick={() => onViewChange?.("ports")}
            >
              端口
            </button>
          </div>
          <p className="sidebar__counts">{ports ? portSummary : summary}</p>
        </div>
        {!ports && (
          <button
            type="button"
            className="sidebar__add-icon"
            title="新建 PowerShell"
            aria-label="新建 PowerShell"
            onClick={onAdd}
          >
            <Plus size={14} />
          </button>
        )}
      </div>
      <div className="sidebar__search-wrap">
        <Search size={14} className="sidebar__search-icon" aria-hidden="true" />
        <input
          className="sidebar__search"
          type="search"
          placeholder={ports ? "搜索端口、进程、会话" : "搜索名称、端口、用途"}
          aria-label={ports ? "搜索端口" : "搜索会话"}
          value={ports ? portQuery : query}
          onChange={(event) =>
            ports ? onPortQueryChange?.(event.target.value) : onQueryChange(event.target.value)
          }
        />
      </div>
      {ports ? (
        <div className="sidebar__groups">
          {portGroups.map((group) => (
            <section key={group.id} className="sidebar__group">
              <header className="sidebar__group-head">
                <h2 className="sidebar__group-title">{group.title}</h2>
                <span className="sidebar__group-hint">{group.hint}</span>
              </header>
              <ul className="sidebar__rows">
                {group.rows.map((row) => {
                  const process = processLabel(row.processName);
                  const pid = pidLabel(row.pid);
                  const path = pathLabel(row.programPath);
                  const owner = ownerLabel(row);
                  return (
                    <li key={row.key}>
                      <button
                        type="button"
                        className={`session-row port-row${row.key === selectedPortKey ? " session-row--selected" : ""}`}
                        aria-current={row.key === selectedPortKey ? "true" : undefined}
                        onClick={() => onSelectPort?.(row.key)}
                      >
                        <span className="session-row__main">
                          <span className="session-row__top">
                            <span className="session-row__name">{row.port}</span>
                            <span className="session-row__tail">{row.protocol}</span>
                          </span>
                          <span className="session-row__meta">
                            <span
                              className={
                                process === UNAVAILABLE_LABEL ? "port-row__muted" : undefined
                              }
                            >
                              {process}
                            </span>
                            <span
                              className={`session-row__meta-part${
                                pid === UNAVAILABLE_LABEL ? " port-row__muted" : ""
                              }`}
                            >
                              PID {pid}
                            </span>
                            <span
                              className={`session-row__meta-part ${
                                row.attribution === "session"
                                  ? "port-row__session"
                                  : "port-row__muted"
                              }`}
                            >
                              {owner}
                            </span>
                          </span>
                          <span className="port-row__path">{path}</span>
                        </span>
                      </button>
                    </li>
                  );
                })}
              </ul>
            </section>
          ))}
          {portEmpty && <p className="sidebar__empty">{portEmpty}</p>}
        </div>
      ) : (
        <div className="sidebar__groups">
          {groups.map((group) => {
            const expanded = sessionGroupExpanded(
              collapsedGroups,
              group.group.id,
              query,
              group.items.length,
            );
            return (
              <section key={group.group.id} className="sidebar__group">
                <header className="sidebar__group-head">
                  <h2 className="sidebar__group-title">
                    <button
                      type="button"
                      className="sidebar__group-toggle"
                      aria-expanded={expanded}
                      aria-label={sessionGroupToggleName(group.group.label, expanded)}
                      onClick={() =>
                        setCollapsedGroups((current) =>
                          sessionGroupAfterTitleToggle(
                            current,
                            group.group.id,
                            query,
                            group.items.length,
                          ),
                        )
                      }
                    >
                      <ChevronRight
                        size={12}
                        className={`sidebar__group-chevron${
                          expanded ? " sidebar__group-chevron--open" : ""
                        }`}
                        aria-hidden="true"
                      />
                      {group.group.label}
                    </button>
                  </h2>
                  <span className="sidebar__group-hint">{group.group.hint}</span>
                </header>
                {expanded && (
                  <ul className="sidebar__rows">
                    {group.items.map((item) => (
                      <SessionRow
                        key={item.config.id}
                        item={item}
                        selected={item.config.id === selectedId}
                        now={now}
                        onSelect={onSelect}
                      />
                    ))}
                  </ul>
                )}
              </section>
            );
          })}
          {groups.length === 0 && (
            <p className="sidebar__empty">{query.trim() ? "没有匹配的受管会话。" : "还没有会话"}</p>
          )}
        </div>
      )}
      <div className="sidebar__foot">
        {ports ? (
          <p className="sidebar__foot-note">只查看占用，不结束进程。</p>
        ) : (
          <>
            {/* Spec #59 decision 7 keeps the two entries apart, and this is where
            the difference is visible: the first creates a terminal on the
            click and asks nothing, the second opens a form. The reference's
            mixed wording is what that decision cancels, so the labels are the
            plain ones. */}
            <button type="button" className="sidebar__add" onClick={onAdd}>
              <Terminal size={14} />
              新建 PowerShell
            </button>
            <button
              type="button"
              className="sidebar__add sidebar__add--secondary"
              onClick={onAddApplication}
            >
              <FolderPlus size={14} />
              添加应用
            </button>
          </>
        )}
      </div>
    </div>
  );
}

interface SessionRowProps {
  item: SessionView;
  selected: boolean;
  now: Date;
  onSelect: (sessionId: string) => void;
}

/** One session row: status pip and name, never a decorative app icon. */
function SessionRow({ item, selected, now, onSelect }: SessionRowProps) {
  const { config, runtime } = item;
  const live = runtime.status === "running";
  const tone = statusTone(runtime.status, item.busy ?? false);
  const pulse =
    runtime.status === "starting" ||
    runtime.status === "stopping" ||
    (runtime.status === "running" && (item.busy ?? false));
  const tail =
    live && isPresent(runtime.startedAt)
      ? formatDuration(runtime.startedAt, now)
      : statusLabel(runtime.status, item.busy ?? false, isReady(runtime));
  const meta = sidebarRowMeta(item);
  return (
    <li>
      <button
        type="button"
        className={`session-row${selected ? " session-row--selected" : ""}`}
        aria-current={selected ? "true" : undefined}
        onClick={() => onSelect(config.id)}
      >
        <span className={`pip pip--${tone}${pulse ? " pip--pulse" : ""}`} aria-hidden="true" />
        <span className="session-row__main">
          <span className="session-row__top">
            <span className="session-row__name">{config.name}</span>
            <span className="session-row__tail">{tail}</span>
          </span>
          <span className="session-row__meta">
            {meta.map((chip, index) => (
              <span
                key={index}
                className={[
                  index > 0 ? "session-row__meta-part" : "",
                  chip.tone ? `session-row__chip--${chip.tone}` : "",
                ]
                  .filter(Boolean)
                  .join(" ")}
              >
                {chip.text}
              </span>
            ))}
          </span>
        </span>
      </button>
    </li>
  );
}
