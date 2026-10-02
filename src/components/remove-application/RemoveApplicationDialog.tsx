import { useEffect, useRef, useState } from "react";
import type { FormSaveOutcome, SessionConfigDto } from "../../types/config";
import "../dialog/dialog.css";

export default function RemoveApplicationDialog({
  config,
  onSubmit,
  onClose,
}: {
  config: SessionConfigDto;
  onSubmit: () => Promise<FormSaveOutcome>;
  onClose: () => void;
}) {
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const cancel = useRef<HTMLButtonElement>(null);
  const dialog = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const previous = document.activeElement;
    cancel.current?.focus();
    return () => {
      if (previous instanceof HTMLElement && previous.isConnected) previous.focus();
    };
  }, []);
  const submit = async () => {
    if (submitting) return;
    setSubmitting(true);
    setError(null);
    try {
      const result = await onSubmit();
      if (!result.ok) setError(result.message);
    } finally {
      setSubmitting(false);
    }
  };
  return (
    <div className="dialog__backdrop" role="presentation">
      <div
        className="dialog"
        ref={dialog}
        role="dialog"
        aria-modal="true"
        aria-labelledby="remove-application-title"
        aria-describedby="remove-application-description"
        onKeyDown={(event) => {
          if (event.key === "Escape" && !submitting) onClose();
          if (event.key === "Tab") {
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
          }
        }}
      >
        <header className="dialog__head">
          <h2 className="dialog__title" id="remove-application-title">
            移除「{config.name}」？
          </h2>
        </header>
        <p className="dialog__lead" id="remove-application-description">
          将从受管名单和已保存的启动配置中移除，下次打开 Hub 也不会再出现。
          应用文件和磁盘日志会保留；以后可以通过“添加应用”重新加入。
          运行中的应用请先停止，独立窗口应用请先在自己的窗口中退出。
        </p>
        {error && (
          <div className="dialog__body">
            <p className="dialog__error" role="alert">
              {error}
            </p>
          </div>
        )}
        <footer className="dialog__foot">
          <button
            ref={cancel}
            type="button"
            className="btn btn--secondary btn--sm"
            disabled={submitting}
            onClick={onClose}
          >
            取消
          </button>
          <button
            type="button"
            className="btn btn--secondary btn--sm"
            disabled={submitting}
            onClick={() => void submit()}
          >
            {submitting ? "正在移除…" : "确认移除"}
          </button>
        </footer>
      </div>
    </div>
  );
}
