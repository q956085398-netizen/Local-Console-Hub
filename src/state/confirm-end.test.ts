import { describe, expect, it } from "vitest";
import { isConfirmEndResult } from "../types/confirm-end";
import {
  afterEndConfirmation,
  createdAtFrom,
  endTargetOf,
  occupantActionEnds,
  type OccupantAction,
} from "./confirm-end";

const actions: OccupantAction[] = ["refresh", "continue", "view", "cancel", "confirm"];

describe("endTargetOf", () => {
  it("names an external process by pid and creation time", () => {
    expect(
      endTargetOf({ pid: 42, attribution: "external", createdAt: "132" } as {
        pid: number;
        attribution: string;
      }),
    ).toEqual({ pid: 42, createdAt: "132" });
  });

  it("does not name a managed session, an unread row, or a pid without a creation time", () => {
    const session = { pid: 42, attribution: "session", createdAt: "132" };
    const unread = { pid: null, attribution: "unavailable", createdAt: "132" };
    const noTime = { pid: 42, attribution: "external" };
    const idle = { pid: 0, attribution: "external", createdAt: "132" };
    expect(endTargetOf(session)).toBeNull();
    expect(endTargetOf(unread)).toBeNull();
    expect(endTargetOf(noTime)).toBeNull();
    expect(endTargetOf(idle)).toBeNull();
    expect(createdAtFrom({ createdAt: "12x" })).toBeNull();
    expect(createdAtFrom({})).toBeNull();
  });
});

describe("occupant actions", () => {
  it("ends a process only from the explicit confirm", () => {
    expect(actions.filter(occupantActionEnds)).toEqual(["confirm"]);
    expect(occupantActionEnds("refresh")).toBe(false);
    expect(occupantActionEnds("continue")).toBe(false);
    expect(occupantActionEnds("view")).toBe(false);
    expect(occupantActionEnds("cancel")).toBe(false);
  });

  it("leaves the process running and the session unchanged on cancel", () => {
    expect(afterEndConfirmation("cancel")).toEqual({
      requestEnd: false,
      processLeftRunning: true,
      sessionUnchanged: true,
    });
    expect(afterEndConfirmation("confirm")).toEqual({
      requestEnd: true,
      processLeftRunning: false,
      sessionUnchanged: true,
    });
  });
});

describe("isConfirmEndResult", () => {
  it("accepts the refusal and rejects a payload that could be a session change", () => {
    expect(isConfirmEndResult({ ended: false, message: "没有结束。进程已经退出。" })).toBe(true);
    expect(isConfirmEndResult({ ended: true, message: "已结束刚才核对过的这个进程。" })).toBe(true);
    expect(isConfirmEndResult({ ended: false })).toBe(false);
    expect(isConfirmEndResult({ sessionId: "holder", status: "running" })).toBe(false);
  });
});
