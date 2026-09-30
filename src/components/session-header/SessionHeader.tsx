import { useEffect, useRef, useState } from "react";
import { AppWindow, ExternalLink, FolderOpen, MoreHorizontal, Play, RotateCw, Square } from "lucide-react";
import type { SessionConfigDto } from "../../types/config";
import type { SessionRuntimeDto } from "../../types/runtime";
import type { SessionAction } from "../../state/actions";
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
  /** What the control was asked for; the caller decides what it means. */
  onAction: (action: SessionAction) => void;
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
          {/* The primary control of a run the Hub owns is 启动/停止. For a
              standalone application it is neither: stopping is not the Hub's
              to do (#66), so the control that remains is the one that works —
              opening the application, which is also what brings its own window
              forward when it is already running. */}
          {actions.start ? (
            <button
              type="button"
              className="btn btn--primary btn--sm"
              disabled={actions.stopDisabled}
              onClick={() => onAction("start")}
            >
              <Play size={14} />
              启动
            </button>
          ) : actions.stop ? (
            <button
              type="button"
              className="btn btn--secondary btn--sm"
              disabled={actions.stopDisabled}
              onClick={() => onAction("stop")}
            >
              <Square size={14} />
              停止
            </button>
          ) : (
            <button
              type="button"
              className="btn btn--primary btn--sm"
              disabled={actions.stopDisabled}
              title="唤起应用的窗口"
              onClick={() => onAction("start")}
            >
              <AppWindow size={14} />
              打开
            </button>
          )}
          <button
            type="button"
            className="btn btn--secondary btn--sm"
            disabled={!actions.restart}
            title={
              actions.managed
                ? actions.restart
                  ? undefined
                  : "需等待上一次运行结束"
                : "此应用由自己管理生命周期；在配置中启用 Hub 生命周期管理后可从这里重启"
            }
            onClick={() => onAction("restart")}
          >
            <RotateCw size={14} />
            重启
          </button>
          {actions.openUrl !== undefined && (
            <button
              type="button"
              className="btn btn--secondary btn--sm"
              title={actions.openUrl}
              onClick={() => onAction("open-url")}
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
              onClick={() => onAction("open-directory")}
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
                      onAction("open-url");
                    }}
                  >
                    打开网页
                  </MenuItem>
                )}
                {actions.directory && (
                  <MenuItem
                    title={config.cwd}
                    onSelect={() => {
                      setMenuOpen(false);
                      onAction("open-directory");
                    }}
                  >
                    打开目录
                  </MenuItem>
                )}
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
                {actions.copyPath && (
                  <MenuItem
                    title={config.cwd}
                    onSelect={() => {
                      setMenuOpen(false);
                      onAction("copy-path");
                    }}
                  >
                    复制路径
                  </MenuItem>
                )}
                <div className="more-menu__separator" role="separator" />
                {config.temporary === true && (
                  <MenuItem
                    destructive
                    disabled={!actions.remove}
                    title={
                      actions.remove ? "从列表去掉这个临时终端及它保留的输出" : "先结束终端再移除"
                    }
                    onSelect={() => {
                      setMenuOpen(false);
                      onAction("remove-session");
                    }}
                  >
                    移除临时终端
                  </MenuItem>
                )}
                <MenuItem
                  destructive
                  disabled={!actions.forceStop}
                  title={actions.forceStop ? "只作用于本会话的受管进程树" : "仅运行中的会话可用"}
                  onSelect={() => {
                    setMenuOpen(false);
                    onAction("force-stop");
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
          {callout.note && <p className="callout__note">{callout.note}</p>}
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
