import { Plus, Search, Terminal } from "lucide-react";
import {
  formatDuration,
  isReady,
  sidebarRowMeta,
  statusLabel,
  statusTone,
  type SessionGroup,
} from "../../state/derivations";
import type { SessionView } from "../../state/session-view";
import { isPresent } from "../../types/runtime";
import "./Sidebar.css";

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
}: SidebarProps) {
  return (
    <div className="sidebar">
      <div className="sidebar__head">
        <div>
          <p className="sidebar__title">受管会话</p>
          <p className="sidebar__counts">{summary}</p>
        </div>
        <button
          type="button"
          className="sidebar__add-icon"
          title="新建 PowerShell"
          aria-label="新建 PowerShell"
          onClick={onAdd}
        >
          <Plus size={14} />
        </button>
      </div>
      <div className="sidebar__search-wrap">
        <Search size={14} className="sidebar__search-icon" aria-hidden="true" />
        <input
          className="sidebar__search"
          type="search"
          placeholder="搜索名称、端口、用途"
          aria-label="搜索会话"
          value={query}
          onChange={(event) => onQueryChange(event.target.value)}
        />
      </div>
      <div className="sidebar__groups">
        {groups.map((group) => (
          <section key={group.group.id} className="sidebar__group">
            <header className="sidebar__group-head">
              <h2 className="sidebar__group-title">{group.group.label}</h2>
              <span className="sidebar__group-hint">{group.group.hint}</span>
            </header>
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
          </section>
        ))}
        {groups.length === 0 && <p className="sidebar__empty">没有匹配的受管会话。</p>}
      </div>
      <div className="sidebar__foot">
        {/* Spec #59 decision 7 keeps the two entries apart: this one creates a
            terminal immediately, and the form-based "添加应用" entry is a
            different control. The reference's mixed wording is what that
            decision cancels, so the label is the plain one. */}
        <button type="button" className="sidebar__add" onClick={onAdd}>
          <Terminal size={14} />
          新建 PowerShell
        </button>
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
