import { RefreshCw } from "lucide-react";
import {
  ownerLabel,
  pathLabel,
  pidLabel,
  processLabel,
  udpNote,
  UNAVAILABLE_LABEL,
  type ListedPort,
} from "../../state/ports";
import "./PortsWorkspace.css";

export interface PortsWorkspaceProps {
  connected: boolean;
  rows: ListedPort[];
  selected: ListedPort | null;
  caption: string;
  empty: string | null;
  inProgress: boolean;
  onSelect: (key: string) => void;
  onRefresh: () => void;
  onOpenSession: (sessionId: string) => void;
  openableSessionId: (row: ListedPort) => string | null;
}

/**
 * The ports workspace: protocol, address, owner, pid, and attribution for the
 * real list. External and unavailable stay neutral, and nothing here ends a
 * process.
 */
export default function PortsWorkspace({
  connected,
  rows,
  selected,
  caption,
  empty,
  inProgress,
  onSelect,
  onRefresh,
  onOpenSession,
  openableSessionId,
}: PortsWorkspaceProps) {
  return (
    <div className="ports-workspace">
      <header className="ports-workspace__header">
        <div className="ports-workspace__heading">
          <h2 className="ports-workspace__title">正在监听</h2>
          <p className="ports-workspace__status">{connected ? caption : "尚未连接后端"}</p>
        </div>
        <button
          type="button"
          className="ports-workspace__refresh"
          onClick={onRefresh}
          disabled={!connected || inProgress}
        >
          <RefreshCw size={14} />
          {inProgress ? "正在检查" : "刷新"}
        </button>
      </header>
      <div className="ports-workspace__body">
        <section className="ports-card">
          <p className="ports-card__eyebrow">只查看</p>
          <p className="ports-card__text">
            正在监听的端口，包括 Hub
            以外的进程。对上了另一个受管会话就写那个会话的名字，对上了进程但不是受管会话就写外部，读不到的字段保持信息不可用。不会结束任何进程，也不会把外部程序收成当前会话。
          </p>
        </section>
        {!connected ? (
          <p className="ports-workspace__note">连接到后端后，这里显示这台电脑正在监听的端口。</p>
        ) : (
          <>
            <div className="ports-table-card">
              {rows.length === 0 ? (
                <p className="ports-workspace__note">{empty ?? "没有可显示的端口。"}</p>
              ) : (
                <table className="ports-table">
                  <thead>
                    <tr>
                      <th>端口</th>
                      <th>协议</th>
                      <th>地址</th>
                      <th>占用者</th>
                      <th>PID</th>
                      <th>路径</th>
                      <th>归属</th>
                    </tr>
                  </thead>
                  <tbody>
                    {rows.map((row) => (
                      <tr
                        key={row.key}
                        aria-selected={selected?.key === row.key}
                        onClick={() => onSelect(row.key)}
                      >
                        <td className="ports-table__mono">{row.port}</td>
                        <td className="ports-table__mono">{row.protocol}</td>
                        <td className="ports-table__mono">{row.address}</td>
                        <td className={fieldClass(processLabel(row.processName))}>
                          {processLabel(row.processName)}
                        </td>
                        <td className={fieldClass(pidLabel(row.pid))}>{pidLabel(row.pid)}</td>
                        <td
                          className={`${fieldClass(pathLabel(row.programPath))} ports-table__path`}
                        >
                          {pathLabel(row.programPath)}
                        </td>
                        <td>
                          <Owner row={row} />
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </div>
            {selected && (
              <OwnerCard
                row={selected}
                sessionId={openableSessionId(selected)}
                onOpenSession={onOpenSession}
              />
            )}
          </>
        )}
      </div>
    </div>
  );
}

function fieldClass(label: string): string {
  return label === UNAVAILABLE_LABEL ? "ports-table__neutral" : "ports-table__mono";
}

function Owner({ row }: { row: ListedPort }) {
  const label = ownerLabel(row);
  if (row.attribution === "session") {
    return <span className="ports-owner">{label}</span>;
  }
  return <span className="ports-owner ports-owner--neutral">{label}</span>;
}

function OwnerCard({
  row,
  sessionId,
  onOpenSession,
}: {
  row: ListedPort;
  sessionId: string | null;
  onOpenSession: (sessionId: string) => void;
}) {
  const note = udpNote(row.protocol);
  const who = ownerLabel(row);
  const process = processLabel(row.processName);
  const pid = pidLabel(row.pid);
  const path = pathLabel(row.programPath);
  return (
    <article className="ports-card">
      <p className="ports-card__eyebrow">占用者</p>
      <h3
        className={
          process === UNAVAILABLE_LABEL ? "ports-card__who ports-owner--neutral" : "ports-card__who"
        }
      >
        {process}
      </h3>
      <p className="ports-card__identity">
        {row.address}:{row.port} · {row.protocol}
      </p>
      <p
        className={
          pid === UNAVAILABLE_LABEL
            ? "ports-card__identity ports-owner--neutral"
            : "ports-card__identity"
        }
      >
        PID {pid}
      </p>
      <p
        className={
          path === UNAVAILABLE_LABEL
            ? "ports-card__identity ports-owner--neutral"
            : "ports-card__identity"
        }
      >
        路径 {path}
      </p>
      <div className="ports-card__who-row">
        {sessionId !== null ? (
          <>
            <span>受管会话 {who}</span>
            <button
              type="button"
              className="ports-workspace__refresh"
              onClick={() => onOpenSession(sessionId)}
            >
              打开会话
            </button>
          </>
        ) : (
          <span className="ports-owner--neutral">{who}</span>
        )}
      </div>
      {note && <p className="ports-card__note">{note}</p>}
      {row.attribution !== "session" && (
        <p className="ports-card__note">
          {row.attribution === "external"
            ? "没有对上受管会话。这是外部进程，不是当前会话，只展示，不会被结束。"
            : "这一行的归属读不到，不把它当成外部，也不把它当成某个会话。"}
        </p>
      )}
    </article>
  );
}
