import { useEffect, useRef } from "react";
import type { EndTarget } from "../../types/confirm-end";
import "../dialog/dialog.css";

export interface ConfirmEndDialogProps {
  target: EndTarget;
  busy: boolean;
  message: string | null;
  onCancel: () => void;
  onConfirm: () => void;
}

/**
 * The explicit confirm (#110).
 *
 * Cancel leaves the process running and does not change session state.
 * Confirm is the only control that asks to end this pid.
 */
export default function ConfirmEndDialog({
  target,
  busy,
  message,
  onCancel,
  onConfirm,
}: ConfirmEndDialogProps) {
  const cancel = useRef<HTMLButtonElement>(null);
  const dialog = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const previous = document.activeElement;
    cancel.current?.focus();
    return () => {
      if (previous instanceof HTMLElement && previous.isConnected) previous.focus();
    };
  }, []);

  return (
    <div className="dialog__backdrop" role="presentation">
      <div
        className="dialog"
        ref={dialog}
        role="dialog"
        aria-modal="true"
        aria-labelledby="confirm-end-title"
        aria-describedby="confirm-end-description"
        onKeyDown={(event) => {
          event.stopPropagation();
          if (event.key === "Escape" && !busy) onCancel();
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
          <h2 className="dialog__title" id="confirm-end-title">
            确认结束这个进程
          </h2>
        </header>
        <p className="dialog__lead" id="confirm-end-description">
          将结束 PID {target.pid}（创建时间 {target.createdAt}）以及它此刻仍是同一进程的子进程。
          取消则这个进程继续运行，会话状态不变。不会按端口或进程名结束，也不会停止其他受管会话，也不会把外部程序收成当前会话。
        </p>
        {message && <p className="dialog__lead">{message}</p>}
        <footer className="dialog__foot">
          <button
            ref={cancel}
            type="button"
            className="btn btn--secondary btn--sm"
            disabled={busy}
            onClick={onCancel}
          >
            取消
          </button>
          <button
            type="button"
            className="btn btn--primary btn--sm"
            disabled={busy}
            onClick={onConfirm}
          >
            确认结束
          </button>
        </footer>
      </div>
    </div>
  );
}
