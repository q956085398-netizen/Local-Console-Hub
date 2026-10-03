import { describe, expect, it } from "vitest";
import workspaceSource from "../components/ports/PortsWorkspace.tsx?raw";
import hookSource from "../components/ports/usePortList.ts?raw";
import type { ListenerRowDto } from "../types/listen";
import listenSource from "../types/listen.ts?raw";
import portsSource from "./ports.ts?raw";
import {
  acceptListenResult,
  beginListenRefresh,
  checkCaption,
  completeListenRefresh,
  filterPorts,
  initialListenRefresh,
  nameListeners,
  openableSessionId,
  portListMessage,
  shouldPollPorts,
  UDP_SOCKET_NOTE,
  udpNote,
  type ListedPort,
} from "./ports";

const sessions = [
  { id: "comfy", name: "ComfyUI" },
  { id: "shell", name: "PowerShell" },
];

function raw(overrides: Partial<ListenerRowDto> & Pick<ListenerRowDto, "port">): ListenerRowDto {
  return {
    protocol: "TCP",
    address: "127.0.0.1",
    pid: 100,
    processName: "app.exe",
    programPath: null,
    attribution: "external",
    sessionId: null,
    ...overrides,
  };
}

function listed(overrides: Partial<ListedPort> & Pick<ListedPort, "port">): ListedPort {
  const row = raw(overrides);
  const [named] = nameListeners(
    [
      {
        ...row,
        attribution: overrides.attribution ?? row.attribution,
        sessionId: overrides.sessionId ?? row.sessionId,
        processName: overrides.processName === undefined ? row.processName : overrides.processName,
      },
    ],
    sessions,
  );
  return { ...named, ...overrides, key: named.key };
}

describe("filterPorts", () => {
  const rows = nameListeners(
    [
      raw({ port: 9, processName: "alpha.exe", attribution: "session", sessionId: "comfy" }),
      raw({ port: 10, processName: "beta.exe", attribution: "external" }),
      raw({ port: 11, processName: "gamma.exe", attribution: "session", sessionId: "shell" }),
    ],
    sessions,
  );

  it("filters by port, process name, and session name", () => {
    expect(filterPorts(rows, "9").map((row) => row.port)).toEqual([9]);
    expect(filterPorts(rows, "ALPHA").map((row) => row.port)).toEqual([9]);
    expect(filterPorts(rows, "comfy").map((row) => row.port)).toEqual([9]);
    expect(filterPorts(rows, "PowerShell").map((row) => row.port)).toEqual([11]);
  });

  it("does not keep the previous rows when nothing matches", () => {
    const previous = filterPorts(rows, "alpha");
    expect(previous).toHaveLength(1);
    const empty = filterPorts(rows, "no-such-port");
    expect(empty).toEqual([]);
    expect(empty).not.toEqual(previous);
    expect(portListMessage(empty, "no-such-port", initialListenRefresh())).toBe("没有匹配的端口。");
  });
});

describe("checkCaption", () => {
  const previous = raw({ port: 9, processName: "kept.exe" });
  const now = 1_700_000_000_000;

  it("does not present a failure as a check that just finished", () => {
    const held = completeListenRefresh(
      initialListenRefresh(),
      { ok: true, checkedAtMs: now, rows: [previous] },
      true,
    );
    const failed = completeListenRefresh(
      beginListenRefresh(held),
      { ok: false, failure: "owner table failed" },
      true,
    );
    expect(failed.rows).toEqual([previous]);
    expect(failed.checkedAtMs).toBe(now);
    const caption = checkCaption(failed, now);
    expect(caption.justChecked).toBe(false);
    expect(caption.text).toContain("检查失败");
    expect(caption.text).toContain("上次成功");
    expect(caption.text).not.toContain("最近检查");
    const firstFailure = completeListenRefresh(
      beginListenRefresh(initialListenRefresh()),
      { ok: false, failure: "owner table failed" },
      true,
    );
    expect(portListMessage([], "", firstFailure)).toBe("这次检查没有得到列表。");
    expect(portListMessage(nameListeners(failed.rows, []), "", failed)).toBeNull();
  });

  it("does not label a hidden-window result as just checked", () => {
    const held = completeListenRefresh(
      initialListenRefresh(),
      { ok: true, checkedAtMs: now - 60_000, rows: [previous] },
      true,
    );
    const hidden = completeListenRefresh(
      beginListenRefresh(held),
      { ok: true, checkedAtMs: now, rows: [raw({ port: 99, processName: "hidden.exe" })] },
      false,
    );
    expect(hidden.rows).toEqual([previous]);
    expect(hidden.checkedAtMs).toBe(now - 60_000);
    expect(hidden.stale).toBe(true);
    const caption = checkCaption(hidden, now);
    expect(caption.justChecked).toBe(false);
    expect(caption.text).not.toContain("最近检查");
    expect(
      acceptListenResult({
        manual: false,
        portsView: true,
        visibleAtStart: true,
        visibleNow: false,
      }),
    ).toBe(false);
    expect(
      acceptListenResult({ manual: true, portsView: true, visibleAtStart: true, visibleNow: true }),
    ).toBe(true);
  });
});

describe("openable sessions", () => {
  it("does not offer external or unavailable rows as sessions", () => {
    const managed = listed({
      port: 9,
      attribution: "session",
      sessionId: "comfy",
      processName: "alpha.exe",
    });
    const external = listed({ port: 10, attribution: "external", processName: "beta.exe" });
    const unavailable = listed({
      port: 11,
      attribution: "unavailable",
      pid: null,
      processName: null,
    });
    expect(openableSessionId(managed, sessions)).toBe("comfy");
    expect(openableSessionId(external, sessions)).toBeNull();
    expect(openableSessionId(unavailable, sessions)).toBeNull();
    expect(udpNote("UDP")).toBe(UDP_SOCKET_NOTE);
    expect(udpNote("TCP")).toBeNull();
  });
});

describe("polling", () => {
  it("polls only while the ports page is showing on a visible connected window", () => {
    expect(shouldPollPorts("ports", true, true)).toBe(true);
    expect(shouldPollPorts("ports", false, true)).toBe(false);
    expect(shouldPollPorts("sessions", true, true)).toBe(false);
    expect(shouldPollPorts("ports", true, false)).toBe(false);
  });
});

describe("sample rows", () => {
  it("does not keep the prototype's sample listeners in the port sources", () => {
    const source = [portsSource, listenSource, workspaceSource, hookSource].join("\n");
    const banned = ["SillyTavern", "vertex-proxy", "svchost", "64728", "49670", "p8000"];
    for (const token of banned) {
      expect(source).not.toContain(token);
    }
  });
});
