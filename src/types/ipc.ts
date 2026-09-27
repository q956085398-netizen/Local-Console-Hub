/**
 * Frontend mirror of the backend IPC DTOs (src-tauri/src/ipc/).
 *
 * The authoritative definitions live in Rust; these TypeScript types plus
 * their runtime guards are the typed frontend side of the contract
 * (MVP_IMPLEMENTATION_SPEC.md §9: keep the frontend/backend contract explicit).
 */

/**
 * Version of the ping IPC contract. The authoritative value lives in Rust
 * (`PING_PROTOCOL_VERSION` in src-tauri/src/ipc/mod.rs); bump this mirror
 * when the payload shape changes in a way the frontend must react to.
 */

/** Response of the Rust `ping` command (serde camelCase). */
export interface PingResponse {
  appName: string;
  appVersion: string;
  protocol: number;
}

/** Runtime guard for values returned by `invoke("ping")`. */
export function isPingResponse(value: unknown): value is PingResponse {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.appName === "string" &&
    typeof candidate.appVersion === "string" &&
    typeof candidate.protocol === "number" &&
    Number.isInteger(candidate.protocol)
  );
}
