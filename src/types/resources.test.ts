import { describe, expect, it } from "vitest";
import { isSessionResourcesDto } from "./resources";

describe("isSessionResourcesDto", () => {
  it("accepts a finished check, an unread field, and a session that was not checked", () => {
    expect(
      isSessionResourcesDto({
        sessionId: "api",
        checkedAtMs: 10,
        treeUnavailable: false,
        members: [{ pid: 4, role: "own", cpuPercentHundredths: 0, memoryBytes: 4096 }],
      }),
    ).toBe(true);
    expect(
      isSessionResourcesDto({
        sessionId: "api",
        checkedAtMs: 10,
        treeUnavailable: true,
        members: [{ pid: 4, role: "tree", cpuPercentHundredths: null, memoryBytes: null }],
      }),
    ).toBe(true);
    expect(
      isSessionResourcesDto({
        sessionId: "api",
        checkedAtMs: null,
        treeUnavailable: false,
        members: [],
      }),
    ).toBe(true);
  });

  it("rejects a row that is not a resource reading", () => {
    expect(
      isSessionResourcesDto({
        sessionId: "api",
        checkedAtMs: 10,
        treeUnavailable: false,
        members: [{ pid: 4, role: "external", cpuPercentHundredths: 0, memoryBytes: 1 }],
      }),
    ).toBe(false);
    expect(
      isSessionResourcesDto({
        sessionId: "api",
        checkedAtMs: 10,
        treeUnavailable: false,
        members: [{ pid: 4 }],
      }),
    ).toBe(false);
  });
});
