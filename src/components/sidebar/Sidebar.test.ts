import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { SessionGroup } from "../../state/derivations";
import type { ListedPort, PortGroup } from "../../state/ports";
import type { SessionView } from "../../state/session-view";
import Sidebar, {
  initialCollapsedPortGroups,
  sessionGroupAfterTitleToggle,
  sessionGroupExpanded,
  sessionGroupToggleName,
  type SidebarProps,
} from "./Sidebar";

function render(overrides: Partial<SidebarProps> = {}): string {
  return renderToStaticMarkup(
    createElement(Sidebar, {
      groups: [],
      selectedId: "",
      now: new Date(0),
      summary: "0 运行 · 0 会话",
      query: "",
      onQueryChange: vi.fn(),
      onSelect: vi.fn(),
      onAdd: vi.fn(),
      onAddApplication: vi.fn(),
      ...overrides,
    }),
  );
}

function session(id: string, name: string, group: string): SessionView {
  return {
    group,
    config: {
      id,
      name,
      sessionType: "terminal",
      display: "internal",
      lifecycle: "managed",
      logging: { mode: "off", source: "none" },
    },
    runtime: {
      sessionId: id,
      status: "stopped",
      ptyAttached: false,
      external: false,
      logging: { mode: "off", source: "none" },
      buffer: { bytes: 0, lines: 0, droppedBytes: 0 },
      health: null,
    },
    runs: [],
  };
}

function group(id: string, label: string, hint: string, names: readonly string[]): SessionGroup {
  return {
    group: { id, label, hint },
    items: names.map((name) => session(name.toLowerCase(), name, id)),
  };
}

/** Rows the rail keeps once the collapse rules have been applied. */
function visibleNames(
  collapsed: ReadonlySet<string>,
  groupId: string,
  query: string,
  names: readonly string[],
): readonly string[] {
  return sessionGroupExpanded(collapsed, groupId, query, names.length) ? names : [];
}

function listener(
  port: number,
  processName: string,
  attribution: ListedPort["attribution"] = "external",
): ListedPort {
  return {
    key: `${attribution}:${port}`,
    protocol: "TCP",
    address: "127.0.0.1",
    port,
    pid: port,
    processName,
    programPath: null,
    attribution,
    sessionId: attribution === "session" ? "comfy" : null,
    sessionName: attribution === "session" ? "ComfyUI" : null,
  };
}

function portGroup(
  id: PortGroup["id"],
  title: string,
  hint: string,
  rows: readonly ListedPort[],
): PortGroup {
  return { id, title, hint, rows: [...rows] };
}

/** Port rows still on screen once that group's collapse rule has been applied. */
function visiblePorts(
  collapsed: ReadonlySet<string>,
  groupId: string,
  query: string,
  rows: readonly ListedPort[],
): readonly ListedPort[] {
  return sessionGroupExpanded(collapsed, groupId, query, rows.length) ? rows : [];
}

describe("Sidebar", () => {
  /**
   * Spec #59 decision 7: the quick entry and the form entry are two controls
   * with two labels. The reference's mixed wording ("新建 PowerShell / 服务")
   * is what that decision cancels — a label that mentions both is how the two
   * became confused in the first place.
   */
  it("keeps the quick entry and the application form apart", () => {
    const markup = render();

    expect(markup).toContain("新建 PowerShell");
    expect(markup).toContain("添加应用");
    expect(markup).not.toContain("新建 PowerShell / 服务");
  });

  it("places the two footer actions on one row and keeps them off the ports view", () => {
    const sessions = render();
    const row = sessions.match(/<div class="sidebar__actions">([\s\S]*?)<\/div>/);

    expect(row).not.toBeNull();
    expect(row?.[1]).toContain("新建 PowerShell");
    expect(row?.[1]).toContain("添加应用");
    expect(row?.[1]?.match(/<button/g)).toHaveLength(2);

    const ports = render({ view: "ports" });
    expect(ports).toContain("只查看占用，不结束进程。");
    expect(ports).not.toContain("sidebar__actions");
    expect(ports).not.toContain("新建 PowerShell");
    expect(ports).not.toContain("添加应用");
  });

  it("names an expanded session group and still shows its rows, title, and hint", () => {
    const markup = render({
      groups: [
        group("configured", "Configured", "config.yaml", ["ComfyUI"]),
        group("temporary", "Temporary", "用完即走的终端", ["Scratch"]),
      ],
      selectedId: "comfyui",
    });

    expect(markup).toContain('aria-expanded="true"');
    expect(markup).toContain('aria-label="Configured，已展开"');
    expect(markup).toContain('aria-label="Temporary，已展开"');
    expect(markup).toContain("ComfyUI");
    expect(markup).toContain("Scratch");
    expect(markup).toContain("config.yaml");
    expect(markup).toContain("用完即走的终端");
    expect(markup).toContain('aria-current="true"');
    expect(markup).toContain("新建 PowerShell");
    expect(markup).toContain("添加应用");
  });

  it("starts external folded and leaves managed and unavailable open", () => {
    const markup = render({
      view: "ports",
      portGroups: [
        portGroup("managed", "受管", "对上了会话", [listener(8188, "comfy.exe", "session")]),
        portGroup("external", "外部", "Hub 以外", [listener(54321, "nginx.exe")]),
        portGroup("unavailable", "信息不可用", "不猜测归属", [
          listener(7777, "mystery.exe", "unavailable"),
        ]),
      ],
    });

    expect(markup).toContain('aria-label="受管，已展开"');
    expect(markup).toContain('aria-label="外部，已折叠"');
    expect(markup).toContain('aria-label="信息不可用，已展开"');
    expect(markup).toContain("sidebar__group-toggle");
    expect(markup).toContain("sidebar__group-chevron");
    expect(markup).toContain("comfy.exe");
    expect(markup).toContain("mystery.exe");
    expect(markup).toContain("对上了会话");
    expect(markup).toContain("Hub 以外");
    expect(markup).toContain("不猜测归属");
    expect(markup).not.toContain("nginx.exe");
    expect(markup).not.toContain(">54321<");
    expect(markup).toContain("只查看占用，不结束进程。");
    expect(markup).not.toContain("结束占用");
  });

  it("shows external rows when the query matches inside that group", () => {
    const markup = render({
      view: "ports",
      portQuery: "nginx",
      portGroups: [portGroup("external", "外部", "Hub 以外", [listener(54321, "nginx.exe")])],
    });

    expect(markup).toContain('aria-label="外部，已展开"');
    expect(markup).toContain("nginx.exe");
    expect(markup).toContain(">54321<");
    expect(markup).toContain("Hub 以外");
  });

  it("keeps the empty port state when nothing matches", () => {
    const markup = render({
      view: "ports",
      portQuery: "no-such-port",
      portGroups: [],
      portEmpty: "没有匹配的端口。",
    });

    expect(markup).toContain("没有匹配的端口。");
    expect(markup).not.toContain("session-row");
    expect(markup).not.toContain("nginx.exe");
  });
});

describe("session group collapse", () => {
  it("starts every session group expanded", () => {
    const collapsed = new Set<string>();

    expect(visibleNames(collapsed, "configured", "", ["ComfyUI", "KoboldCpp"])).toEqual([
      "ComfyUI",
      "KoboldCpp",
    ]);
    expect(visibleNames(collapsed, "temporary", "", ["Scratch"])).toEqual(["Scratch"]);
    expect(visibleNames(collapsed, "custom", "", ["Other"])).toEqual(["Other"]);
    expect(sessionGroupToggleName("Configured", true)).toBe("Configured，已展开");
  });

  it("hides a group's rows when its title is toggled, and shows them on the next toggle", () => {
    const collapsed = sessionGroupAfterTitleToggle(new Set(), "configured", "", 2);

    expect(visibleNames(collapsed, "configured", "", ["ComfyUI", "KoboldCpp"])).toEqual([]);
    expect(visibleNames(collapsed, "temporary", "", ["Scratch"])).toEqual(["Scratch"]);
    expect(sessionGroupToggleName("Configured", false)).toBe("Configured，已折叠");

    const opened = sessionGroupAfterTitleToggle(collapsed, "configured", "", 2);
    expect(visibleNames(opened, "configured", "", ["ComfyUI", "KoboldCpp"])).toEqual([
      "ComfyUI",
      "KoboldCpp",
    ]);
  });

  it("opens a collapsed group while a query matches a row, then restores that choice", () => {
    const collapsed = sessionGroupAfterTitleToggle(new Set(), "configured", "", 2);
    expect(visibleNames(collapsed, "configured", "", ["ComfyUI", "KoboldCpp"])).toEqual([]);

    expect(visibleNames(collapsed, "configured", "comfy", ["ComfyUI"])).toEqual(["ComfyUI"]);
    const foldedTemporary = sessionGroupAfterTitleToggle(new Set(), "temporary", "", 1);
    expect(sessionGroupExpanded(foldedTemporary, "temporary", "comfy", 0)).toBe(false);
    const foldedCustom = sessionGroupAfterTitleToggle(new Set(), "custom", "", 1);
    expect(visibleNames(foldedCustom, "custom", "other", ["Other"])).toEqual(["Other"]);
    expect(visibleNames(foldedCustom, "custom", "", ["Other"])).toEqual([]);
    expect([...collapsed]).toEqual(["configured"]);

    expect(visibleNames(collapsed, "configured", "", ["ComfyUI", "KoboldCpp"])).toEqual([]);
    expect(visibleNames(collapsed, "configured", "   ", ["ComfyUI", "KoboldCpp"])).toEqual([]);
  });
});

describe("port group collapse", () => {
  const managed = [listener(8188, "comfy.exe", "session")];
  const external = [listener(54321, "nginx.exe"), listener(54322, "edge.exe")];
  const unavailable = [listener(7777, "mystery.exe", "unavailable")];

  it("starts with external collapsed and the other port groups expanded", () => {
    const collapsed = initialCollapsedPortGroups();

    expect([...collapsed]).toEqual(["external"]);
    expect(visiblePorts(collapsed, "external", "", external)).toEqual([]);
    expect(visiblePorts(collapsed, "managed", "", managed)).toEqual(managed);
    expect(visiblePorts(collapsed, "unavailable", "", unavailable)).toEqual(unavailable);
    expect(sessionGroupToggleName("外部", false)).toBe("外部，已折叠");
    expect(sessionGroupToggleName("受管", true)).toBe("受管，已展开");
  });

  it("toggles one port group from its title and leaves the others as they were", () => {
    const opened = sessionGroupAfterTitleToggle(initialCollapsedPortGroups(), "external", "", 2);
    expect(visiblePorts(opened, "external", "", external)).toEqual(external);
    expect(visiblePorts(opened, "managed", "", managed)).toEqual(managed);
    expect(sessionGroupToggleName("外部", true)).toBe("外部，已展开");

    const shut = sessionGroupAfterTitleToggle(opened, "external", "", 2);
    expect(visiblePorts(shut, "external", "", external)).toEqual([]);

    const foldedManaged = sessionGroupAfterTitleToggle(
      initialCollapsedPortGroups(),
      "managed",
      "",
      1,
    );
    expect(visiblePorts(foldedManaged, "managed", "", managed)).toEqual([]);
    expect(visiblePorts(foldedManaged, "unavailable", "", unavailable)).toEqual(unavailable);
    expect(visiblePorts(foldedManaged, "external", "", external)).toEqual([]);
    const reopened = sessionGroupAfterTitleToggle(foldedManaged, "managed", "", 1);
    expect(visiblePorts(reopened, "managed", "", managed)).toEqual(managed);
  });

  it("opens external for a search hit, then restores the stored choice when the query is cleared", () => {
    const collapsed = initialCollapsedPortGroups();
    expect(visiblePorts(collapsed, "external", "nginx", [external[0]])).toEqual([external[0]]);
    expect([...collapsed]).toEqual(["external"]);
    expect(visiblePorts(collapsed, "external", "no-such-port", [])).toEqual([]);
    expect(visiblePorts(collapsed, "external", "", external)).toEqual([]);
    expect(visiblePorts(collapsed, "external", "   ", external)).toEqual([]);

    const opened = sessionGroupAfterTitleToggle(collapsed, "external", "", external.length);
    expect(opened.has("external")).toBe(false);
    expect(visiblePorts(opened, "external", "nginx", [external[0]])).toEqual([external[0]]);
    expect(visiblePorts(opened, "external", "", external)).toEqual(external);
  });

  it("does not let a port title toggle rewrite the session collapse set", () => {
    const sessions = new Set<string>();
    const ports = initialCollapsedPortGroups();
    const nextPorts = sessionGroupAfterTitleToggle(ports, "managed", "", 1);
    const nextSessions = sessionGroupAfterTitleToggle(sessions, "configured", "", 2);

    expect(ports.has("external")).toBe(true);
    expect(ports.has("managed")).toBe(false);
    expect([...nextPorts].sort()).toEqual(["external", "managed"]);
    expect([...nextSessions]).toEqual(["configured"]);
    expect(nextSessions.has("external")).toBe(false);
    expect(nextPorts.has("configured")).toBe(false);
    expect(visibleNames(sessions, "configured", "", ["ComfyUI"])).toEqual(["ComfyUI"]);
  });
});
