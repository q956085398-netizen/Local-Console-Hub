import { isLogStatusDto, type LogStatusDto } from "../types/logs";

/** A user request that changes the current run's logging state. */
export type LogAction =
  | { type: "save"; successMessage: string }
  | { type: "recording"; recording: boolean; successMessage: string };

/** Session Core's two log mutations, injected at the app seam. */
export interface LogActionBackend {
  invoke: (command: string, args: Record<string, unknown>) => Promise<unknown>;
}

/** Effects the Logs view needs from one completed action. */
export interface LogActionHandlers {
  /** Invalidate reads that began before this mutation. */
  onStart: () => void;
  onBusyChange: (busy: boolean) => void;
  onStatus: (status: LogStatusDto) => void;
  onSuccess: (message: string) => void;
  onFailure: (message: string) => void;
  onRefresh: () => void;
}

export interface LogActionCoordinator {
  /** Ignores overlapping requests; a later request may start after this settles. */
  run: (action: LogAction) => Promise<void>;
  /** Prevents pending work from reporting into a session view that has closed. */
  dispose: () => void;
}

type ErrorMessage = (error: unknown) => string;

/**
 * Coordinate one view's log mutations against Session Core.
 *
 * Only one write is allowed at a time, so a rapid start/stop cannot be
 * submitted in an ambiguous order. The response is the authoritative status:
 * it is applied before any success notice, and a returned `lastError` is a
 * failure even when the command itself resolved. Every callback is suppressed
 * after `dispose`, which the hook calls when its keyed session view unmounts.
 */
export function createLogActionCoordinator(
  sessionId: string,
  backend: LogActionBackend,
  handlers: LogActionHandlers,
  errorMessage: ErrorMessage = () => "未知错误",
): LogActionCoordinator {
  let busy = false;
  let disposed = false;

  const run = async (action: LogAction): Promise<void> => {
    if (busy || disposed) return;

    busy = true;
    handlers.onStart();
    handlers.onBusyChange(true);

    try {
      const command = actionCommand(action);
      const args =
        action.type === "save" ? { sessionId } : { sessionId, recording: action.recording };
      const value = await backend.invoke(command, args);
      if (disposed) return;

      if (!isLogStatusDto(value) || value.sessionId !== sessionId) {
        handlers.onFailure(
          `${actionContext(sessionId, action)}失败：后端返回的日志状态无效或属于其他会话。正在重新读取；若问题持续，请检查 Session Core 的日志状态。`,
        );
        handlers.onRefresh();
        return;
      }

      handlers.onStatus(value);
      const failure = actionFailure(sessionId, action, value);
      if (failure !== undefined) {
        handlers.onFailure(failure);
        return;
      }
      handlers.onSuccess(action.successMessage);
    } catch (error) {
      if (disposed) return;
      const detail = errorMessage(error).trim() || "未知错误";
      handlers.onFailure(
        `${actionContext(sessionId, action)}失败：${detail}。正在重新读取日志状态。`,
      );
      handlers.onRefresh();
    } finally {
      if (!disposed) {
        busy = false;
        handlers.onBusyChange(false);
      }
    }
  };

  return {
    run,
    dispose: () => {
      disposed = true;
    },
  };
}

function actionFailure(
  sessionId: string,
  action: LogAction,
  status: LogStatusDto,
): string | undefined {
  if (status.lastError != null) {
    const path = status.lastError.path ? ` · 路径：${status.lastError.path}` : "";
    return `${actionContext(sessionId, action)} · ${status.lastError.operation} 失败：${status.lastError.message}${path}`;
  }

  if (action.type === "save") {
    if (
      status.mode === "on_error" &&
      status.source === "captured" &&
      status.logFile != null &&
      status.logFilePresent
    ) {
      return undefined;
    }
    return `${actionContext(sessionId, action)}失败：后端未确认日志文件已保存。请检查当前日志策略和文件状态后重试。`;
  }

  if (
    status.mode === "manual" &&
    status.source === "captured" &&
    status.state === (action.recording ? "capturing" : "off")
  ) {
    return undefined;
  }
  return action.recording
    ? `${actionContext(sessionId, action)}失败：后端未确认开始记录。请检查当前日志状态和策略后重试。`
    : `${actionContext(sessionId, action)}失败：后端未确认停止记录。请检查当前日志状态和策略后重试。`;
}

function actionCommand(action: LogAction): string {
  return action.type === "save" ? "save_run_log" : "set_log_recording";
}

function actionContext(sessionId: string, action: LogAction): string {
  const label =
    action.type === "save" ? "保存本次运行日志" : action.recording ? "开始记录" : "停止记录";
  return `会话 ${sessionId} · 操作 ${label} (${actionCommand(action)})`;
}
