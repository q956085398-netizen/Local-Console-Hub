import { useEffect, useState } from "react";
import { X } from "lucide-react";
import type {
  AddApplicationErrorDto,
  AddApplicationOutcome,
  NewApplicationFormDto,
} from "../../types/config";
import "./AddApplicationDialog.css";

export interface AddApplicationDialogProps {
  /**
   * Save the form. The dialog stays open while this is in flight and reports
   * what came back — including a refusal, which is why it never assumes the
   * save succeeded (spec #59 decision 15: 只在持久保存确认后反馈成功).
   */
  onSubmit: (form: NewApplicationFormDto) => Promise<AddApplicationOutcome>;
  onClose: () => void;
}

/**
 * The config-layer field names this form has an input for.
 *
 * The backend's refusals name the *config* field (`close_impact`, not
 * `closeImpact`), which is what lets the two sides share one vocabulary
 * without the form re-mapping anything.
 */
const RENDERED_FIELDS: ReadonlySet<string> = new Set([
  "name",
  "cwd",
  "command",
  "port",
  "url",
  "purpose",
  "close_impact",
  "logging.path",
]);

/**
 * Where a refusal belongs, or `null` for "in the banner".
 *
 * A refusal is shown beside its input only when that input is on screen:
 * `logging.path` exists only for the external policy, and a field this form
 * does not render — a future one, or one the config layer adds — has nowhere
 * to go. Both fall through to the banner rather than disappearing, which is
 * the failure mode worth designing out (story 31: 保存失败有明确反馈).
 */
export function errorPlacement(
  error: AddApplicationErrorDto | null,
  policy: string,
): string | null {
  const field = error?.field;
  if (field === undefined || !RENDERED_FIELDS.has(field)) return null;
  if (field === "logging.path" && policy !== "external") return null;
  return field;
}

/**
 * The logging policies the form offers.
 *
 * Each one is a combination the config layer already accepts, stated in its
 * own vocabulary (`docs/LOGGING.md` §3) — the dialog does not invent a policy,
 * it picks between the ones the file format has. The empty value is "not
 * specified", which is what lets the config layer's defaults apply.
 */
const LOG_POLICIES: ReadonlyArray<{ value: string; label: string; hint: string }> = [
  { value: "", label: "未指定（服务默认：出错时记录）", hint: "不写 `logging:` 块，沿用默认" },
  { value: "off", label: "关闭", hint: "只保留内存滚动缓冲，不落盘" },
  { value: "on_error", label: "出错时记录", hint: "运行失败时写一份 Hub 日志" },
  { value: "always", label: "始终记录", hint: "运行期间持续写入 Hub 日志" },
  { value: "manual", label: "仅手动记录", hint: "只有手动保存时才写日志" },
  { value: "external", label: "关联应用自有日志", hint: "Hub 只指向应用自己写的日志文件" },
];

/** The logging block for a chosen policy, or `undefined` for "not specified". */
export function loggingFor(
  policy: string,
  path: string,
): NewApplicationFormDto["logging"] | undefined {
  switch (policy) {
    case "off":
      return { mode: "off", source: "none" };
    case "on_error":
      return { mode: "on_error", source: "captured" };
    case "always":
      return { mode: "always", source: "captured" };
    case "manual":
      return { mode: "manual", source: "captured" };
    case "external":
      return { mode: "always", source: "external", path: path.trim() };
    default:
      return undefined;
  }
}

/**
 * The "添加应用" form (#64, spec #59 decision 7).
 *
 * The secondary entry, deliberately apart from "新建 PowerShell": that one
 * creates a terminal immediately and asks nothing, this one saves a launch
 * configuration the Hub will list and reuse. Name, working directory and
 * command are required; purpose, close impact, port, web address and the log
 * policy are optional, and a blank optional box is left out of the file rather
 * than written as an empty value.
 *
 * ## Why there is no display-mode choice here
 *
 * The spec has two display modes — inside the Hub, or in the application's own
 * window — and only the first one exists until #66 lands. The form therefore
 * states the mode instead of offering a choice that would not work (spec #59
 * decision 8: 只提供可用的模式，不显示不能工作的选项).
 */
export default function AddApplicationDialog({ onSubmit, onClose }: AddApplicationDialogProps) {
  const [name, setName] = useState("");
  const [cwd, setCwd] = useState("");
  const [command, setCommand] = useState("");
  const [purpose, setPurpose] = useState("");
  const [closeImpact, setCloseImpact] = useState("");
  const [port, setPort] = useState("");
  const [url, setUrl] = useState("");
  const [policy, setPolicy] = useState(LOG_POLICIES[0].value);
  const [logPath, setLogPath] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<AddApplicationErrorDto | null>(null);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (submitting) return;

    const form: NewApplicationFormDto = {
      name: name.trim(),
      cwd: cwd.trim(),
      command: command.trim(),
    };
    const optional = [
      ["purpose", purpose],
      ["closeImpact", closeImpact],
      ["url", url],
    ] as const;
    for (const [key, value] of optional) {
      const trimmed = value.trim();
      if (trimmed !== "") form[key] = trimmed;
    }
    if (port.trim() !== "") {
      const value = Number(port.trim());
      // Checked here rather than sent: the wire type is a 16-bit integer, so a
      // typo would come back as a deserialization failure instead of the
      // field's own message.
      if (!Number.isInteger(value) || value < 1 || value > 65535) {
        setError({ field: "port", message: "端口必须是 1-65535 之间的整数。" });
        return;
      }
      form.port = value;
    }
    const logging = loggingFor(policy, logPath);
    if (logging !== undefined) form.logging = logging;

    setError(null);
    setSubmitting(true);
    try {
      const result = await onSubmit(form);
      if (!result.ok) setError({ field: result.field, message: result.message });
    } finally {
      setSubmitting(false);
    }
  };

  const placedField = errorPlacement(error, policy);

  /** The message for one input, when the refusal has been placed on it. */
  const errorFor = (field: string) => (placedField === field ? error?.message : null);

  return (
    <div className="add-app__backdrop" role="presentation">
      <form
        className="add-app"
        role="dialog"
        aria-modal="true"
        aria-labelledby="add-application-title"
        onSubmit={submit}
      >
        <header className="add-app__head">
          <h2 className="add-app__title" id="add-application-title">
            添加应用
          </h2>
          <button type="button" className="add-app__close" aria-label="关闭" onClick={onClose}>
            <X size={14} />
          </button>
        </header>

        <p className="add-app__lead">
          保存一个启动配置；保存后会立即出现在会话列表里，下次打开 Hub 仍然可用。
        </p>

        <div className="add-app__body">
          <label className="add-app__field">
            <span className="add-app__label">
              名称 <span className="add-app__required">必填</span>
            </span>
            <input
              className="add-app__input"
              value={name}
              autoFocus
              onChange={(event) => setName(event.target.value)}
              placeholder="ComfyUI"
            />
            {errorFor("name") && <span className="add-app__field-error">{errorFor("name")}</span>}
          </label>

          <label className="add-app__field">
            <span className="add-app__label">
              工作目录 <span className="add-app__required">必填</span>
            </span>
            <input
              className="add-app__input add-app__input--mono"
              value={cwd}
              onChange={(event) => setCwd(event.target.value)}
              placeholder="D:\Tools\ComfyUI_windows_portable"
            />
            {errorFor("cwd") && <span className="add-app__field-error">{errorFor("cwd")}</span>}
          </label>

          <label className="add-app__field">
            <span className="add-app__label">
              启动命令 <span className="add-app__required">必填</span>
            </span>
            <input
              className="add-app__input add-app__input--mono"
              value={command}
              onChange={(event) => setCommand(event.target.value)}
              placeholder="python main.py"
            />
            {errorFor("command") && (
              <span className="add-app__field-error">{errorFor("command")}</span>
            )}
          </label>

          <div className="add-app__row">
            <label className="add-app__field">
              <span className="add-app__label">端口</span>
              <input
                className="add-app__input add-app__input--mono"
                value={port}
                inputMode="numeric"
                onChange={(event) => setPort(event.target.value)}
                placeholder="8188"
              />
              {errorFor("port") && <span className="add-app__field-error">{errorFor("port")}</span>}
            </label>
            <label className="add-app__field">
              <span className="add-app__label">网页地址</span>
              <input
                className="add-app__input add-app__input--mono"
                value={url}
                onChange={(event) => setUrl(event.target.value)}
                placeholder="http://127.0.0.1:8188"
              />
              {errorFor("url") && <span className="add-app__field-error">{errorFor("url")}</span>}
            </label>
          </div>

          <label className="add-app__field">
            <span className="add-app__label">用途</span>
            <input
              className="add-app__input"
              value={purpose}
              onChange={(event) => setPurpose(event.target.value)}
              placeholder="图像生成后端，队列里经常会有长时间任务。"
            />
            {errorFor("purpose") && (
              <span className="add-app__field-error">{errorFor("purpose")}</span>
            )}
          </label>

          <label className="add-app__field">
            <span className="add-app__label">关闭影响</span>
            <input
              className="add-app__input"
              value={closeImpact}
              onChange={(event) => setCloseImpact(event.target.value)}
              placeholder="停止会中断当前生成；队列中的任务会丢失。"
            />
            {errorFor("close_impact") && (
              <span className="add-app__field-error">{errorFor("close_impact")}</span>
            )}
          </label>

          <label className="add-app__field">
            <span className="add-app__label">日志策略</span>
            <select
              className="add-app__input"
              value={policy}
              onChange={(event) => setPolicy(event.target.value)}
            >
              {LOG_POLICIES.map((option) => (
                <option key={option.value} value={option.value}>
                  {option.label}
                </option>
              ))}
            </select>
            <span className="add-app__hint">
              {LOG_POLICIES.find((option) => option.value === policy)?.hint}
            </span>
          </label>

          {policy === "external" && (
            <label className="add-app__field">
              <span className="add-app__label">应用日志文件</span>
              <input
                className="add-app__input add-app__input--mono"
                value={logPath}
                onChange={(event) => setLogPath(event.target.value)}
                placeholder="D:\Tools\ComfyUI\logs\app.log"
              />
              {errorFor("logging.path") && (
                <span className="add-app__field-error">{errorFor("logging.path")}</span>
              )}
            </label>
          )}

          {/* Stated, not offered: the other display mode is #66's to add, and
              a choice that did not work would be worse than no choice — so
              this says what will happen rather than asking. */}
          <p className="add-app__mode">显示方式：Hub 内显示</p>

          {error && placedField === null && (
            <p className="add-app__error" role="alert">
              {error.message}
            </p>
          )}
        </div>

        <footer className="add-app__foot">
          <button type="button" className="btn btn--secondary btn--sm" onClick={onClose}>
            取消
          </button>
          <button type="submit" className="btn btn--primary btn--sm" disabled={submitting}>
            {submitting ? "正在保存…" : "保存应用"}
          </button>
        </footer>
      </form>
    </div>
  );
}
