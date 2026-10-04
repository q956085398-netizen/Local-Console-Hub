import { useEffect, useRef, useState } from "react";
import type { OccupantLine } from "../../state/port-occupancy";
import { UDP_SOCKET_NOTE } from "../../state/ports";
import "../dialog/dialog.css";
import ConfirmEndDialog from "./ConfirmEndDialog";
import "./PortOccupancyDialog.css";
import { useConfirmedEnd } from "./useConfirmedEnd";

export interface PortOccupancyDialogProps {
  sessionName: string;
  port: number;
  /** `unreadable` is a failed collection: no listener is named, including no session. */
  mode: "occupied" | "unreadable";
  lines: readonly OccupantLine[];
  /** Calls the existing activate. Does not end the occupant. */
  onContinue: () => void;
  /** Leaves the session unstarted. */
  onClose: () => void;
}

/**
 * The configured port is already being listened on (#99).
 *
 * The user sees who holds it — process, PID, path, protocol, address, and
 * whether that process is a managed session, external, or unread — and then
 * cancels or continues. Continuing is the ordinary start. Cancelling starts
 * nothing. Ending an occupant is a separate confirm, and only that confirm
 * asks to end the process it names.
 */
export default function PortOccupancyDialog({
  sessionName,
  port,
  mode,
  lines,
  onContinue,
  onClose,
}: PortOccupancyDialogProps) {
  const [submitting, setSubmitting] = useState(false);
  const ending = useConfirmedEnd();
  const cancel = useRef<HTMLButtonElement>(null);
  const dialog = useRef<HTMLDivElement>(null);
  const locked = submitting || ending.pending !== null;

  useEffect(() => {
    const previous = document.activeElement;
    cancel.current?.focus();
    return () => {
      if (previous instanceof HTMLElement && previous.isConnected) previous.focus();
    };
  }, []);

  const choose = (choice: "cancel" | "continue") => {
    if (locked) return;
    setSubmitting(true);
    if (choice === "continue") onContinue();
    else onClose();
  };

  return (
    <div className="dialog__backdrop" role="presentation">
      <div
        className="dialog"
        ref={dialog}
        role="dialog"
        aria-modal="true"
        aria-labelledby="port-occupancy-title"
        aria-describedby="port-occupancy-description"
        onKeyDown={(event) => {
          if (event.key === "Escape" && !submitting) {
            if (ending.pending) ending.cancel();
            else onClose();
          }
          if (event.key !== "Tab") return;
          const buttons =
            dialog.current?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)");
          if (!buttons?.length) {
            event.preventDefault();
            return;
          }
          const first = buttons[0];
          const last = buttons[buttons.length - 1];
          if (event.shiftKey && document.activeElement === first) {
            event.preventDefault();
            last.focus();
          } else if (!event.shiftKey && document.activeElement === last) {
            event.preventDefault();
            first.focus();
          }
        }}
      >
        <header className="dialog__head">
          <h2 className="dialog__title" id="port-occupancy-title">
            「{sessionName}」的端口已被占用
          </h2>
        </header>
        <p className="dialog__lead" id="port-occupancy-description">
          {mode === "unreadable" ? (
            <>
              端口 {port}{" "}
              在启动前需要确认占用者，但这次没有读到（信息不可用）。这不是一次空的检查，也没有把它认成某个会话。Hub
              不会因此结束任何进程。你可以取消，或仍然启动。
            </>
          ) : (
            <>
              端口 {port} 上已经有程序在监听。下面是读到的占用者、PID、路径和归属。Hub
              不会因此结束占用者，也不会停止其他会话，更不会把外部程序认成当前会话。取消则不会启动。要结束其中某一个，需要再确认一次。
            </>
          )}
        </p>
        {mode === "occupied" && (
          <div className="dialog__body">
            <ul className="port-occupancy__list">
              {lines.map((line) => (
                <li key={line.key} className="port-occupancy__item">
                  <span
                    className={
                      line.processNeutral ? "port-occupancy__neutral" : "port-occupancy__process"
                    }
                  >
                    {line.processName}
                  </span>
                  <span className="port-occupancy__meta">PID {line.pid}</span>
                  <span className="port-occupancy__meta">路径 {line.path}</span>
                  <span className="port-occupancy__meta">
                    {line.protocol} · {line.address}
                  </span>
                  <span
                    className={
                      line.attributionNeutral ? "port-occupancy__neutral" : "port-occupancy__owner"
                    }
                  >
                    归属 {line.attribution}
                  </span>
                  {line.protocol === "UDP" && (
                    <span className="port-occupancy__neutral">{UDP_SOCKET_NOTE}</span>
                  )}
                  {line.endTarget && (
                    <button
                      type="button"
                      className="btn btn--secondary btn--sm port-occupancy__end"
                      disabled={locked}
                      onClick={() => {
                        if (line.endTarget) ending.ask(line.endTarget);
                      }}
                    >
                      结束此进程
                    </button>
                  )}
                </li>
              ))}
            </ul>
          </div>
        )}
        <footer className="dialog__foot">
          <button
            ref={cancel}
            type="button"
            className="btn btn--secondary btn--sm"
            disabled={locked}
            onClick={() => choose("cancel")}
          >
            取消
          </button>
          <button
            type="button"
            className="btn btn--primary btn--sm"
            disabled={locked}
            onClick={() => choose("continue")}
          >
            仍然启动
          </button>
        </footer>
      </div>
      {ending.pending && (
        <ConfirmEndDialog
          target={ending.pending}
          busy={ending.busy}
          message={ending.message}
          onCancel={ending.cancel}
          onConfirm={ending.confirm}
        />
      )}
    </div>
  );
}
