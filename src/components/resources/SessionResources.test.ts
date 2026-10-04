import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import SessionResources from "./SessionResources";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

describe("SessionResources", () => {
  it("does not render a resource table before a check", () => {
    const markup = renderToStaticMarkup(createElement(SessionResources, { sessionId: "api" }));
    expect(markup).toContain("查看");
    expect(markup).toContain("尚未查看");
    expect(markup).not.toContain("<table");
    expect(markup).not.toContain("0%");
    expect(markup).not.toContain("0 B");
    expect(markup).not.toContain("信息不可用");
  });
});
