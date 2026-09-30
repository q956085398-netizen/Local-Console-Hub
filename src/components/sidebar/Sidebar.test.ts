import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import Sidebar from "./Sidebar";

function render(): string {
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
    }),
  );
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
});
