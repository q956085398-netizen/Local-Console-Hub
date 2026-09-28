/**
 * The view model one session row and workspace render (T06 #7, T07 #8).
 *
 * It pairs what a session *is* (its validated configuration) with what it is
 * *doing* (its runtime snapshot), plus the UI-only extras the rail and header
 * render. T06 filled it from fixtures; T07 fills it from the backend
 * (`list_session_configs` + `list_sessions` + the `session-state-changed`
 * events) whenever a backend is answering.
 *
 * The extras stay documented here because they are still not contracts:
 *
 * - `group` — a UI concern with **no landed field and no owning ticket** (the
 *   config schema has none). Live sessions are all filed under
 *   [`LIVE_GROUP`], which says where they came from rather than inventing a
 *   classification the config does not have.
 * - `busy` / `ready` — spec §4's optional runtime flags; T08 (#9) produces
 *   them. They supplement lifecycle state and never replace it.
 * - `dependsOn` — likewise unlanded; the details panel renders what it finds.
 * - `lines` — the preview stream. It is what the terminal shows when there is
 *   no backend to attach to (the browser preview), and is never rendered for a
 *   live session: there, the PTY is the stream.
 */

import type { SessionConfigDto } from "../types/config";
import type { RunRecordDto, SessionRuntimeDto } from "../types/runtime";

/** One line of the backend-less preview stream (`Fixtures` only). */
export interface TerminalPreviewLine {
  kind: "sys" | "out" | "err" | "in";
  text: string;
}

/** Workload group of the sidebar (labels and hints from the V2 prototype). */
export interface WorkloadGroup {
  id: string;
  label: string;
  hint: string;
}

/** One session as the workspace renders it. */
export interface SessionView {
  config: SessionConfigDto;
  runtime: SessionRuntimeDto;
  /** Newest last; mirrors the run history T10 will read from the backend. */
  runs: RunRecordDto[];
  group: string;
  busy?: boolean;
  ready?: boolean;
  dependsOn?: string[];
  /** Preview-only: the stream shown when no backend is attached. */
  lines?: TerminalPreviewLine[];
}

/**
 * The group live sessions are filed under.
 *
 * One group, named for its source: the config file is the only thing that
 * decides which sessions exist, and it carries nothing to group them by. When a
 * schema field for that lands, this is where it replaces the placeholder.
 */
export const LIVE_GROUP: WorkloadGroup = {
  id: "configured",
  label: "Configured",
  hint: "config.yaml",
};

/** The snapshot a registered session reports before it has ever been read. */
export function stoppedRuntime(config: SessionConfigDto): SessionRuntimeDto {
  return {
    sessionId: config.id,
    status: "stopped",
    ptyAttached: false,
    // The nested logging block is the config-layer struct verbatim, which
    // serializes its path in snake_case (see the note in `types/runtime.ts`).
    logging: {
      mode: config.logging.mode,
      source: config.logging.source,
      external_path: config.logging.externalPath,
    },
    buffer: { bytes: 0, lines: 0, droppedBytes: 0 },
  };
}

/**
 * Pair each registered configuration with its snapshot.
 *
 * The two lists come from the same registry, so they describe the same
 * sessions; they are joined by id rather than by position because nothing
 * promises the same order. A configuration whose snapshot has not been read
 * yet is rendered stopped — the truthful reading of "nothing has run that we
 * know of" — and the first state event replaces it.
 */
export function sessionsFromLive(
  configs: readonly SessionConfigDto[],
  runtimes: readonly SessionRuntimeDto[],
): SessionView[] {
  const byId = new Map(runtimes.map((runtime) => [runtime.sessionId, runtime]));
  return configs.map((config) => ({
    config,
    runtime: byId.get(config.id) ?? stoppedRuntime(config),
    runs: [],
    group: LIVE_GROUP.id,
  }));
}
