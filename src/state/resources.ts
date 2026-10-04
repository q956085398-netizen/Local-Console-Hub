/**
 * How a managed session's resource check is shown (#108).
 *
 * A field that was not read is 「信息不可用」. A measured zero stays zero.
 * A check that did not finish — the session's process was gone, or its
 * creation time no longer matched — drops whatever numbers were on screen.
 * Those numbers are not labeled as a check that just finished.
 */

import type { ResourceMemberDto, ResourceRole, SessionResourcesDto } from "../types/resources";

export const UNAVAILABLE_LABEL = "信息不可用";

/** What the window says when the process tree could not be listed. */
export const TREE_UNAVAILABLE_NOTE = "进程树读不到。下面只是这个会话自己的进程。";

export const UNCONFIRMED_NOTE = "这次没有核对到仍属于这个会话的进程。";

export type ResourcePhase = "idle" | "current" | "unconfirmed";

export interface ResourceView {
  phase: ResourcePhase;
  checkedAtMs: number | null;
  treeUnavailable: boolean;
  members: ResourceMemberDto[];
}

export function idleResourceView(): ResourceView {
  return { phase: "idle", checkedAtMs: null, treeUnavailable: false, members: [] };
}

/**
 * Fold one attempt into what the window may show.
 *
 * A missing check time, a payload that is not a finished read, or an empty
 * member list clears the previous numbers. It does not keep them under a new
 * time.
 */
export function applyResourceCheck(attempt: SessionResourcesDto | null): ResourceView {
  if (attempt === null || attempt.checkedAtMs === null || attempt.members.length === 0) {
    return { phase: "unconfirmed", checkedAtMs: null, treeUnavailable: false, members: [] };
  }
  return {
    phase: "current",
    checkedAtMs: attempt.checkedAtMs,
    treeUnavailable: attempt.treeUnavailable,
    members: attempt.members,
  };
}

export function showsResourceTable(view: ResourceView): boolean {
  return view.phase === "current" && view.checkedAtMs !== null && view.members.length > 0;
}

export interface ResourceCaption {
  text: string;
  /** True only for a finished check of processes that still matched. */
  justChecked: boolean;
}

export function resourceCaption(view: ResourceView, pending: boolean): ResourceCaption {
  if (pending) return { text: "正在读取", justChecked: false };
  if (!showsResourceTable(view) || view.checkedAtMs === null) {
    return {
      text: view.phase === "unconfirmed" ? "没有读数" : "尚未查看",
      justChecked: false,
    };
  }
  return { text: `最近检查 ${formatCheckTime(view.checkedAtMs)}`, justChecked: true };
}

export function treeNote(view: ResourceView): string | null {
  if (!showsResourceTable(view) || !view.treeUnavailable) return null;
  return TREE_UNAVAILABLE_NOTE;
}

export function formatCpu(hundredths: number | null): string {
  if (hundredths === null || !Number.isFinite(hundredths)) return UNAVAILABLE_LABEL;
  return `${(hundredths / 100).toFixed(1)}%`;
}

/** `12.4 MiB` / `512 B`. Null is unread, which is not `0 B`. */
export function formatMemory(bytes: number | null): string {
  if (bytes === null || !Number.isFinite(bytes)) return UNAVAILABLE_LABEL;
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KiB", "MiB", "GiB", "TiB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${units[unit]}`;
}

export function roleLabel(role: ResourceRole): string {
  return role === "own" ? "自己的进程" : "进程树";
}

export interface ResourceTotals {
  cpu: string;
  memory: string;
}

/**
 * Sum of a finished check.
 *
 * One process has nothing to add. A missing field makes that total
 * 「信息不可用」 rather than a sum of the fields that happened to be readable.
 */
export function resourceTotals(members: readonly ResourceMemberDto[]): ResourceTotals | null {
  if (members.length < 2) return null;
  return { cpu: formatCpu(sumCpu(members)), memory: formatMemory(sumMemory(members)) };
}

function sumCpu(members: readonly ResourceMemberDto[]): number | null {
  let sum = 0;
  for (const member of members) {
    if (member.cpuPercentHundredths === null) return null;
    sum += member.cpuPercentHundredths;
  }
  return sum;
}

function sumMemory(members: readonly ResourceMemberDto[]): number | null {
  let sum = 0;
  for (const member of members) {
    if (member.memoryBytes === null) return null;
    sum += member.memoryBytes;
  }
  return sum;
}

export function formatCheckTime(ms: number): string {
  const date = new Date(ms);
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}
