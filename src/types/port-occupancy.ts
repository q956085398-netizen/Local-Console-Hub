/**
 * Frontend mirror of `port_occupancy` (`src-tauri/src/ipc/listen.rs`).
 *
 * One read, taken before `activate_session`. `prompt` false means start
 * proceeds. A failure sets `prompt` and `failure` and leaves `rows` empty:
 * that is not a successful empty check, and it does not name a session.
 */

import { isListenerRowDto, type ListenerRowDto } from "./listen";

export const PORT_OCCUPANCY_COMMAND = "port_occupancy";

export interface PortOccupancyDto {
  prompt: boolean;
  port: number | null;
  failure: string | null;
  rows: ListenerRowDto[];
}

/** Runtime guard for `invoke("port_occupancy")`. */
export function isPortOccupancyDto(value: unknown): value is PortOccupancyDto {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Record<string, unknown>;
  const port =
    candidate.port === null ||
    (typeof candidate.port === "number" &&
      Number.isInteger(candidate.port) &&
      candidate.port >= 0 &&
      candidate.port <= 65535);
  const failure = candidate.failure === null || typeof candidate.failure === "string";
  return (
    typeof candidate.prompt === "boolean" &&
    port &&
    failure &&
    Array.isArray(candidate.rows) &&
    candidate.rows.every(isListenerRowDto)
  );
}
