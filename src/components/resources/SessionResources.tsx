import { useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  UNCONFIRMED_NOTE,
  applyResourceCheck,
  formatCpu,
  formatMemory,
  idleResourceView,
  resourceCaption,
  resourceTotals,
  roleLabel,
  showsResourceTable,
  treeNote,
  type ResourceView,
} from "../../state/resources";
import { SESSION_RESOURCES_COMMAND, isSessionResourcesDto } from "../../types/resources";
import "./SessionResources.css";

export interface SessionResourcesProps {
  sessionId: string;
}

/**
 * CPU and memory for the selected managed session (#108).
 *
 * Shown only while that session is running. The user asks; nothing here polls,
 * and nothing here ends a process. A check that cannot confirm the session's
 * process clears the numbers instead of leaving them up as a fresh reading.
 */
export default function SessionResources({ sessionId }: SessionResourcesProps) {
  const [view, setView] = useState<ResourceView>(idleResourceView);
  const [pending, setPending] = useState(false);
  const request = useRef(0);

  const refresh = () => {
    const id = request.current + 1;
    request.current = id;
    setPending(true);
    void invoke<unknown>(SESSION_RESOURCES_COMMAND, { sessionId })
      .then((raw) => {
        if (request.current !== id) return;
        if (!isSessionResourcesDto(raw) || raw.sessionId !== sessionId) {
          setView(applyResourceCheck(null));
          return;
        }
        setView(applyResourceCheck(raw));
      })
      .catch(() => {
        if (request.current !== id) return;
        setView(applyResourceCheck(null));
      })
      .finally(() => {
        if (request.current === id) setPending(false);
      });
  };

  const caption = resourceCaption(view, pending);
  const table = showsResourceTable(view);
  const totals = table ? resourceTotals(view.members) : null;
  const note = view.phase === "unconfirmed" && !pending ? UNCONFIRMED_NOTE : treeNote(view);

  return (
    <section className="session-resources" aria-label="会话资源">
      <div className="session-resources__bar">
        <p className="session-resources__summary">
          <span className="session-resources__label">资源</span>
          <span className="session-resources__caption">{caption.text}</span>
        </p>
        <button
          type="button"
          className="session-resources__button"
          onClick={refresh}
          disabled={pending}
        >
          {pending ? "正在读取" : "查看"}
        </button>
      </div>
      {note !== null && <p className="session-resources__note">{note}</p>}
      {table && (
        <table className="session-resources__table">
          <caption className="session-resources__meaning">
            CPU 是占这台电脑处理器的比例，内存是工作集。读不到的是信息不可用。不会结束进程。
          </caption>
          <thead>
            <tr>
              <th>进程</th>
              <th>CPU</th>
              <th>内存</th>
            </tr>
          </thead>
          <tbody>
            {view.members.map((member) => (
              <tr key={`${member.role}-${member.pid}`}>
                <td>
                  {roleLabel(member.role)}
                  <span className="session-resources__pid">{member.pid}</span>
                </td>
                <td className="session-resources__mono">
                  {formatCpu(member.cpuPercentHundredths)}
                </td>
                <td className="session-resources__mono">{formatMemory(member.memoryBytes)}</td>
              </tr>
            ))}
            {totals !== null && (
              <tr>
                <td>已读到的合计</td>
                <td className="session-resources__mono">{totals.cpu}</td>
                <td className="session-resources__mono">{totals.memory}</td>
              </tr>
            )}
          </tbody>
        </table>
      )}
    </section>
  );
}
