import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { SessionGroup } from "../../state/derivations";
import type { SessionView } from "../../state/session-view";
import Sidebar, {
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

  it("leaves port group titles as labels", () => {
    const markup = render({
      view: "ports",
      portGroups: [{ id: "managed", title: "受管", hint: "对上了会话", rows: [] }],
    });

    expect(markup).toContain("受管");
    expect(markup).toContain("对上了会话");
    expect(markup).not.toContain("sidebar__group-toggle");
    expect(markup).not.toContain("aria-expanded");
    expect(markup).toContain("只查看占用，不结束进程。");
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
