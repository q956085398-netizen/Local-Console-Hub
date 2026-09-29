import { describe, expect, it } from "vitest";
import {
  SESSION_FOCUS_REQUESTED,
  isSessionFocusRequestedDto,
  type SessionFocusRequestedDto,
} from "./tray";

/**
 * The event name is the wire contract (`src-tauri/src/tray/mod.rs`); the
 * literal is asserted rather than the constant, so renaming one side is a
 * failing test instead of a listener that quietly never fires.
 */
describe("the tray's focus request", () => {
  it("is published under the name the backend emits", () => {
    expect(SESSION_FOCUS_REQUESTED).toBe("session-focus-requested");
  });

  it("accepts a payload naming a session", () => {
    const payload: SessionFocusRequestedDto = { sessionId: "comfyui" };

    expect(isSessionFocusRequestedDto(payload)).toBe(true);
  });

  it.each([
    ["a missing session id", {}],
    ["an id that is not a string", { sessionId: 7 }],
    ["an id that is null", { sessionId: null }],
    ["a bare string payload", "comfyui"],
    ["null", null],
    ["undefined", undefined],
  ])("rejects %s", (_description, payload) => {
    expect(isSessionFocusRequestedDto(payload)).toBe(false);
  });
});
