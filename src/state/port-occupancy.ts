/**
 * Whether a start should ask who holds the configured port (#99).
 *
 * The window calls `port_occupancy` only when this module says to check.
 * Cancel stays here: it does not call `activate`. A successful TCP connection
 * is not an input: it does not say who owns the port.
 */

import type { SessionStatusValue } from "../types/runtime";
import type { ListenerRowDto } from "../types/listen";
import type { PortOccupancyDto } from "../types/port-occupancy";
import {
  EXTERNAL_LABEL,
  nameListeners,
  ownerLabel,
  processLabel,
  UNAVAILABLE_LABEL,
  type SessionName,
} from "./ports";

const STARTABLE: readonly SessionStatusValue[] = ["stopped", "exited", "error"];

export type BeforeOccupancyCheck =
  | { kind: "preview" }
  | { kind: "activate" }
  | { kind: "check" };

export type OccupancyFollowUp =
  | { kind: "activate" }
  | { kind: "prompt"; mode: "unreadable" }
  | { kind: "prompt"; mode: "occupied"; rows: ListenerRowDto[] };

/** One occupant, with every missing field already spelled 信息不可用. */
export interface OccupantLine {
  key: string;
  processName: string;
  processNeutral: boolean;
  pid: string;
  path: string;
  protocol: "TCP" | "UDP";
  address: string;
  attribution: string;
  /** External and 信息不可用 stay neutral. A named session does not. */
  attributionNeutral: boolean;
}

/**
 * What a start click does before any command.
 *
 * No configured port, and a status `activate` would not start from, proceed.
 * A disconnected window keeps the preview path. Only a connected start of a
 * port-configured stopped, exited, or error session checks occupancy.
 */
export function beforeOccupancyCheck(input: {
  live: boolean;
  port: number | null | undefined;
  status: SessionStatusValue;
}): BeforeOccupancyCheck {
  if (!input.live) return { kind: "preview" };
  if (typeof input.port !== "number" || !STARTABLE.includes(input.status)) {
    return { kind: "activate" };
  }
  return { kind: "check" };
}

/**
 * What the occupancy read means.
 *
 * `prompt: false` is a clear start, including a successful collection that
 * found nothing on the port. A failure, or a prompt with no rows, is
 * unreadable: the rows are not shown, so a session cannot be named from a
 * failed check.
 */
export function afterOccupancyCheck(dto: PortOccupancyDto): OccupancyFollowUp {
  if (!dto.prompt) return { kind: "activate" };
  if (dto.failure !== null || dto.rows.length === 0) {
    return { kind: "prompt", mode: "unreadable" };
  }
  return { kind: "prompt", mode: "occupied", rows: dto.rows };
}

/** Cancel does not start. Continue is the existing activate. */
export function afterPromptChoice(choice: "cancel" | "continue"): "activate" | "stay" {
  return choice === "continue" ? "activate" : "stay";
}

function shown(value: string | null | undefined): string {
  const text = value?.trim() ?? "";
  return text ? text : UNAVAILABLE_LABEL;
}

/**
 * One line per listener. The session name is looked up the same way the ports
 * page does. The session being started is not named as its own occupant.
 * External and a field that was not read are 外部 and 信息不可用.
 */
export function describeOccupants(
  rows: readonly ListenerRowDto[],
  sessions: readonly SessionName[],
  startingId: string,
): OccupantLine[] {
  return nameListeners(rows, sessions).map((row, index) => {
    const self = row.attribution === "session" && row.sessionId === startingId;
    const attribution = self ? UNAVAILABLE_LABEL : ownerLabel(row);
    const processName = processLabel(row.processName);
    return {
      key: `${row.key}#${index}`,
      processName,
      processNeutral: processName === UNAVAILABLE_LABEL,
      pid: row.pid === null ? UNAVAILABLE_LABEL : String(row.pid),
      path: shown(row.programPath),
      protocol: row.protocol,
      address: shown(row.address),
      attribution,
      attributionNeutral: attribution === EXTERNAL_LABEL || attribution === UNAVAILABLE_LABEL,
    };
  });
}
