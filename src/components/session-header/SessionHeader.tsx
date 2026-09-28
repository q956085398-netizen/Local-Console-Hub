import { useEffect, useRef, useState } from "react";
import { ExternalLink, FolderOpen, MoreHorizontal, Play, RotateCw, Square } from "lucide-react";
import type { SessionConfigDto } from "../../types/config";
import type { SessionRuntimeDto } from "../../types/runtime";
import {
  availableActions,
  headerCallout,
  metadataPairs,
  statusLabel,
  statusTone,
  typeLabel,
} from "../../state/derivations";
import "./SessionHeader.css";

export interface SessionHeaderProps {
  config: SessionConfigDto;
  runtime: SessionRuntimeDto;
  busy: boolean;
  ready: boolean;
  now: Date;
  /** Fixture mode: actions are visible but wired to a notice (T07/T08). */
  onAction: (label: string) => void;
  onFocusTerminal: () => void;
  onOpenLogs: () => void;
}

/** Selected-session header: identity, state, close impact, metadata, actions. */
export default function SessionHeader({
  config,
  runtime,
  busy,
  ready,
  now,
  onAction,
  onFocusTerminal,
  onOpenLogs,
}: SessionHeaderProps) {
  const actions = availableActions(config, runtime);
  const callout = headerCallout(config, runtime);
  const metadata = metadataPairs(config, runtime, now);
  const tone = statusTone(runtime.status, busy);
  const [menuOpen, setMenuOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!menuOpen) return;
    const onPointerDown = (event: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(event.target as Node)) {
        setMenuOpen(false);
      }
    };
    document.addEventListener("mousedown", onPointerDown);
    return () => document.removeEventListener("mousedown", onPointerDown);
  }, [menuOpen]);

  return (
    <header className="session-header">
      <div className="session-header__top">
        <div className="session-header__identity">
          <div className="session-header__title-row">
            <h1 className="session-header__name">{config.name}</h1>
            <span className="badge badge--outline">{typeLabel(config.sessionType)}</span>
            <span className={`badge badge--${tone}`}>
              <span className={`pip pip--${tone} badge__pip`} aria-hidden="true" />
              {statusLabel(runtime.status, busy, ready)}
            </span>
          </div>
          <p className="session-header__purpose">{config.purpose}</p>
        </div>
        <div className="session-header__actions">
          {actions.start ? (
            <button
              type="button"
              className="btn btn--primary btn--sm"
              onClick={() => onAction("启动")}
            >
              <Play size={14} />
              启动
            </button>
          ) : (
            <button
              type="button"
              className="btn btn--secondary btn--sm"
              disabled={actions.stopDisabled}
              onClick={() => onAction("停止")}
            >
              <Square size={14} />
              停止
            </button>
          )}
          <button
            type="button"
            className="btn btn--secondary btn--sm"
            disabled={!actions.restart}
            title={actions.restart ? undefined : "需等待上一次运行结束"}
            onClick={() => onAction("重启")}
          >
            <RotateCw size={14} />
            重启
          </button>
          {actions.openUrl !== undefined && (
            <button
              type="button"
              className="btn btn--secondary btn--sm"
              title={actions.openUrl}
              onClick={() => onAction(`打开网页 ${actions.openUrl}`)}
            >
              <ExternalLink size={14} />
              打开网页
            </button>
          )}
          {actions.directory && (
            <button
              type="button"
              className="btn btn--secondary btn--sm"
              title={config.cwd}
              onClick={() => onAction("打开目录")}
            >
              <FolderOpen size={14} />
              目录
            </button>
          )}
          <div className="session-header__more" ref={menuRef}>
            <button
              type="button"
              className="btn btn--ghost btn--icon-sm"
              aria-label="更多操作"
              aria-haspopup="menu"
              aria-expanded={menuOpen}
              onClick={() => setMenuOpen((open) => !open)}
            >
              <MoreHorizontal size={16} />
            </button>
            {menuOpen && (
              <div className="more-menu" role="menu">
                {actions.openUrl !== undefined && (
                  <MenuItem
                    onSelect={() => {
                      setMenuOpen(false);
                      onAction(`打开网页 ${actions.openUrl}`);
                    }}
                  >
                    打开网页
                  </MenuItem>
                )}
                <MenuItem
                  onSelect={() => {
                    setMenuOpen(false);
                    onAction("打开目录");
                  }}
                >
                  打开目录
                </MenuItem>
                <MenuItem
                  onSelect={() => {
                    setMenuOpen(false);
                    onFocusTerminal();
                  }}
                >
                  聚焦终端
                </MenuItem>
                <MenuItem
                  onSelect={() => {
                    setMenuOpen(false);
                    onOpenLogs();
                  }}
                >
                  查看日志策略
                </MenuItem>
                <MenuItem
                  onSelect={() => {
                    setMenuOpen(false);
                    onAction("复制路径");
                  }}
                >
                  复制路径
                </MenuItem>
                <div className="more-menu__separator" role="separator" />
                <MenuItem
                  destructive
                  disabled={!actions.forceStop}
                  title={actions.forceStop ? "只作用于本会话的受管进程树" : "仅运行中的会话可用"}
                  onSelect={() => {
                    setMenuOpen(false);
                    onAction("强制结束进程树");
                  }}
                >
                  强制结束进程树
                </MenuItem>
              </div>
            )}
          </div>
        </div>
      </div>

      {callout && (
        <div className={`callout callout--${callout.kind}`} role="note">
          <p className="callout__title">{callout.title}</p>
          <p className="callout__text">{callout.text}</p>
        </div>
      )}

      <dl className="session-header__meta">
        {metadata.map((pair) => (
          <div key={pair.label} className="session-header__meta-pair">
            <dt>{pair.label}</dt>
            <dd title={pair.value}>{pair.value}</dd>
          </div>
        ))}
      </dl>
    </header>
  );
}

function MenuItem({
  children,
  onSelect,
  destructive,
  disabled,
  title,
}: {
  children: React.ReactNode;
  onSelect: () => void;
  destructive?: boolean;
  disabled?: boolean;
  title?: string;
}) {
  return (
    <button
      type="button"
      role="menuitem"
      className={`more-menu__item${destructive ? " more-menu__item--destructive" : ""}`}
      disabled={disabled}
      title={title}
      onClick={onSelect}
    >
      {children}
    </button>
  );
}
