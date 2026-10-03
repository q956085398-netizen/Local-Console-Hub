/**
 * Frontend mirror of `list_listeners` (`src-tauri/src/ipc/listen.rs`).
 *
 * The command is one read. A failure is a field, not an empty success: `failure`
 * is set and `checkedAtMs` is null, and `rows` is then not a list that was
 * just collected.
 */

export const LIST_LISTENERS = "list_listeners";

export type ListenerAttribution = "session" | "external" | "unavailable";

const ATTRIBUTIONS: readonly ListenerAttribution[] = ["session", "external", "unavailable"];

export interface ListenerRowDto {
  protocol: "TCP" | "UDP";
  address: string;
  port: number;
  pid: number | null;
  processName: string | null;
  programPath: string | null;
  attribution: ListenerAttribution;
  sessionId: string | null;
}

export interface ListenerListDto {
  checkedAtMs: number | null;
  inProgress: boolean;
  failure: string | null;
  rows: ListenerRowDto[];
}

function isRow(value: unknown): value is ListenerRowDto {
  if (typeof value !== "object" || value === null) return false;
  const row = value as Record<string, unknown>;
  const protocol = row.protocol === "TCP" || row.protocol === "UDP";
  const port =
    typeof row.port === "number" &&
    Number.isInteger(row.port) &&
    row.port >= 0 &&
    row.port <= 65535;
  const pid =
    row.pid === null || (typeof row.pid === "number" && Number.isInteger(row.pid) && row.pid >= 0);
  const processName = row.processName === null || typeof row.processName === "string";
  const programPath = row.programPath === null || typeof row.programPath === "string";
  const attribution = ATTRIBUTIONS.includes(row.attribution as ListenerAttribution);
  const sessionId = row.sessionId === null || typeof row.sessionId === "string";
  return (
    protocol &&
    typeof row.address === "string" &&
    port &&
    pid &&
    processName &&
    programPath &&
    attribution &&
    sessionId
  );
}

/** Runtime guard for `invoke("list_listeners")`. */
export function isListenerListDto(value: unknown): value is ListenerListDto {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Record<string, unknown>;
  const checked =
    candidate.checkedAtMs === null ||
    (typeof candidate.checkedAtMs === "number" && Number.isFinite(candidate.checkedAtMs));
  const failure = candidate.failure === null || typeof candidate.failure === "string";
  return (
    checked &&
    typeof candidate.inProgress === "boolean" &&
    failure &&
    Array.isArray(candidate.rows) &&
    candidate.rows.every(isRow)
  );
}
