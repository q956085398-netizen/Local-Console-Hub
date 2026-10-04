/**
 * Frontend mirror of `session_resources` (`src-tauri/src/ipc/resources.rs`).
 *
 * The command is one read of a managed session. `checkedAtMs` is set only when
 * that session's own process still matched. Without it, `members` is empty,
 * and that empty list is not a resource table that was just collected.
 * A null CPU or memory field was not read. It is not zero.
 */

export const SESSION_RESOURCES_COMMAND = "session_resources";

export type ResourceRole = "own" | "tree";

const ROLES: readonly ResourceRole[] = ["own", "tree"];

export interface ResourceMemberDto {
  pid: number;
  role: ResourceRole;
  /** Hundredths of one percent of the machine. Null when the rate was not read. */
  cpuPercentHundredths: number | null;
  /** Working set in bytes. Null when it was not read. */
  memoryBytes: number | null;
}

export interface SessionResourcesDto {
  sessionId: string;
  checkedAtMs: number | null;
  treeUnavailable: boolean;
  members: ResourceMemberDto[];
}

function isCount(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function isOptionalCount(value: unknown): value is number | null {
  return value === null || isCount(value);
}

export function isResourceMemberDto(value: unknown): value is ResourceMemberDto {
  if (typeof value !== "object" || value === null) return false;
  const member = value as Record<string, unknown>;
  return (
    isCount(member.pid) &&
    ROLES.includes(member.role as ResourceRole) &&
    isOptionalCount(member.cpuPercentHundredths) &&
    isOptionalCount(member.memoryBytes)
  );
}

/** Runtime guard for `invoke("session_resources")`. */
export function isSessionResourcesDto(value: unknown): value is SessionResourcesDto {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Record<string, unknown>;
  const checked =
    candidate.checkedAtMs === null ||
    (typeof candidate.checkedAtMs === "number" && Number.isFinite(candidate.checkedAtMs));
  return (
    typeof candidate.sessionId === "string" &&
    checked &&
    typeof candidate.treeUnavailable === "boolean" &&
    Array.isArray(candidate.members) &&
    candidate.members.every(isResourceMemberDto)
  );
}
