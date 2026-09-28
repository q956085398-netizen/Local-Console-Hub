/**
 * The vocabulary of things a session control can ask for (T07 #8).
 *
 * A control reports *what it was asked to do*, not the words it rendered: the
 * label is presentation, and two buttons that happen to read the same would be
 * indistinguishable if the text were the protocol. The caller decides what an
 * action means — Session Core has an operation for the lifecycle ones, and the
 * rest are the UI's own business (or a later ticket's).
 */

/** Every action a session control can report. */
export type SessionAction =
  | "start"
  | "stop"
  | "restart"
  | "force-stop"
  | "open-url"
  | "open-directory"
  | "copy-path"
  | "new-session";

/** The wording each action renders as, for the notices that name it. */
export const SESSION_ACTION_LABELS: Record<SessionAction, string> = {
  start: "启动",
  stop: "停止",
  restart: "重启",
  "force-stop": "强制结束进程树",
  "open-url": "打开网页",
  "open-directory": "打开目录",
  "copy-path": "复制路径",
  "new-session": "新建会话",
};
