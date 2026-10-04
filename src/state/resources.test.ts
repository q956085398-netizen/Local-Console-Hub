import { describe, expect, it } from "vitest";
import type { SessionResourcesDto } from "../types/resources";
import {
  TREE_UNAVAILABLE_NOTE,
  UNAVAILABLE_LABEL,
  UNCONFIRMED_NOTE,
  applyResourceCheck,
  formatCpu,
  formatMemory,
  resourceCaption,
  resourceTotals,
  showsResourceTable,
  treeNote,
} from "./resources";

function finished(overrides: Partial<SessionResourcesDto> = {}): SessionResourcesDto {
  return {
    sessionId: "api",
    checkedAtMs: 1_700_000_000_000,
    treeUnavailable: false,
    members: [
      { pid: 4, role: "own", cpuPercentHundredths: 2500, memoryBytes: 4096 },
      { pid: 5, role: "tree", cpuPercentHundredths: 100, memoryBytes: 1024 },
    ],
    ...overrides,
  };
}

describe("resource readings", () => {
  it("spells an unread field as unavailable and keeps a measured zero", () => {
    expect(formatCpu(null)).toBe(UNAVAILABLE_LABEL);
    expect(formatMemory(null)).toBe(UNAVAILABLE_LABEL);
    expect(formatCpu(0)).toBe("0.0%");
    expect(formatMemory(0)).toBe("0 B");
    expect(formatCpu(2500)).toBe("25.0%");
    expect(formatMemory(4096)).toBe("4.0 KiB");
    expect(UNAVAILABLE_LABEL).not.toBe("0");
  });

  it("does not add a total out of the fields that happened to be readable", () => {
    const partial = finished({
      members: [
        { pid: 4, role: "own", cpuPercentHundredths: 100, memoryBytes: null },
        { pid: 5, role: "tree", cpuPercentHundredths: null, memoryBytes: 2048 },
      ],
    });
    expect(resourceTotals(partial.members)).toEqual({
      cpu: UNAVAILABLE_LABEL,
      memory: UNAVAILABLE_LABEL,
    });
    expect(resourceTotals(finished().members)).toEqual({ cpu: "26.0%", memory: "5.0 KiB" });
    expect(resourceTotals([finished().members[0]])).toBeNull();
  });

  it("drops the previous numbers when the process is no longer this session", () => {
    const current = applyResourceCheck(finished());
    expect(showsResourceTable(current)).toBe(true);
    expect(resourceCaption(current, false).justChecked).toBe(true);

    const exited = applyResourceCheck({
      sessionId: "api",
      checkedAtMs: null,
      treeUnavailable: false,
      members: [],
    });
    expect(showsResourceTable(exited)).toBe(false);
    expect(exited.members).toEqual([]);
    expect(exited.checkedAtMs).toBeNull();
    expect(resourceCaption(exited, false).justChecked).toBe(false);
    expect(resourceCaption(exited, true).justChecked).toBe(false);
    expect(resourceCaption(current, true).justChecked).toBe(false);
  });

  it("does not treat an unread tree as an empty resource table", () => {
    const view = applyResourceCheck(
      finished({
        treeUnavailable: true,
        members: [{ pid: 4, role: "own", cpuPercentHundredths: null, memoryBytes: 4096 }],
      }),
    );
    expect(showsResourceTable(view)).toBe(true);
    expect(treeNote(view)).toBe(TREE_UNAVAILABLE_NOTE);
    expect(formatCpu(view.members[0].cpuPercentHundredths)).toBe(UNAVAILABLE_LABEL);
    expect(UNCONFIRMED_NOTE).not.toContain("0%");
  });

  it("shows no table for a check that did not finish", () => {
    const view = applyResourceCheck(null);
    expect(showsResourceTable(view)).toBe(false);
    expect(treeNote(view)).toBeNull();
    expect(resourceCaption(view, false).text).toBe("没有读数");
  });
});
