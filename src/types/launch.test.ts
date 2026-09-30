import { describe, expect, it } from "vitest";
import { SESSION_OPENED, isSessionOpenedDto, type SessionOpenedDto } from "./launch";

/**
 * The event name is the wire contract (`src-tauri/src/app/launch.rs`); the
 * literal is asserted rather than the constant, so renaming one side is a
 * failing test instead of a listener that quietly never fires.
 */
describe("the launch request's open request", () => {
  it("is published under the name the backend emits", () => {
    expect(SESSION_OPENED).toBe("session-opened");
  });

  /**
   * The two window-facing messages are different requests — one asks to look
   * at a session, the other to be put in a terminal — so a listener must not
   * be woken by the other's event.
   */
  it("is not the tray's focus request under another name", () => {
    expect(SESSION_OPENED).not.toBe("session-focus-requested");
  });

  it("accepts a payload naming a session", () => {
    const payload: SessionOpenedDto = { sessionId: "terminal-1" };

    expect(isSessionOpenedDto(payload)).toBe(true);
  });

  it.each([
    ["a missing session id", {}],
    ["an id that is not a string", { sessionId: 7 }],
    ["an id that is null", { sessionId: null }],
    ["a bare string payload", "terminal-1"],
    ["null", null],
    ["undefined", undefined],
  ])("rejects %s", (_description, payload) => {
    expect(isSessionOpenedDto(payload)).toBe(false);
  });
});
