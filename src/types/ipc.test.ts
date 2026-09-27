import { describe, expect, it } from "vitest";
import { isPingResponse } from "./ipc";

/**
 * Seam under test (pre-agreed): the typed frontend ↔ backend ping DTO boundary.
 *
 * The fixture below is the documented shape of the Rust `ping` command
 * (src-tauri/src/ipc/mod.rs). It is an independent source of truth for this
 * test — if the backend contract changes, this fixture must change with it,
 * which is exactly what the guard is supposed to catch.
 */
const backendPingPayload: unknown = {
  appName: "Local Console Hub",
  appVersion: "0.1.0",
  protocol: 1,
};

describe("isPingResponse (ping IPC contract guard)", () => {
  it("accepts the payload the Rust ping command documents", () => {
    expect(isPingResponse(backendPingPayload)).toBe(true);
  });

  it("rejects non-objects", () => {
    expect(isPingResponse(null)).toBe(false);
    expect(isPingResponse(undefined)).toBe(false);
    expect(isPingResponse("pong")).toBe(false);
    expect(isPingResponse(1)).toBe(false);
  });

  it("rejects payloads with a missing required field", () => {
    for (const field of ["appName", "appVersion", "protocol"]) {
      const incomplete = { ...(backendPingPayload as object) };
      delete incomplete[field as keyof typeof incomplete];
      expect(isPingResponse(incomplete)).toBe(false);
    }
  });

  it("rejects payloads with wrongly typed fields", () => {
    expect(isPingResponse({ appName: 1, appVersion: "0.1.0", protocol: 1 })).toBe(false);
    expect(isPingResponse({ appName: "Local Console Hub", appVersion: 2, protocol: 1 })).toBe(
      false,
    );
    expect(
      isPingResponse({ appName: "Local Console Hub", appVersion: "0.1.0", protocol: "1" }),
    ).toBe(false);
  });
});
