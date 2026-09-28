/**
 * Frontend-only view state vocabulary for the V2 workspace (T06 #7).
 *
 * These are UI concerns, not runtime contracts: which tab is front, and the
 * label order the tab strip renders. Lifecycle truth stays in the DTOs.
 */

/** The normative workspace tabs (UI_STYLE_GUIDE §6). */
export type WorkspaceTab = "terminal" | "logs" | "details";

/** Tab strip definition, in normative order. */
export const WORKSPACE_TABS: readonly { key: WorkspaceTab; label: string }[] = [
  { key: "terminal", label: "终端" },
  { key: "logs", label: "日志" },
  { key: "details", label: "详情" },
];
