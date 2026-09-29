import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { INITIAL_CONNECTION, watchBackendConnection } from "../state/backend-connection";
import type { BackendConnection } from "../state/backend-connection";

/**
 * The T00 bootstrap's typed ping, kept alive as a quiet status-bar signal.
 *
 * An adapter, and only that: it supplies the real `invoke("ping")` to
 * `state/backend-connection.ts` and renders the state that module decides.
 * The decision itself — including that a host which is late is still worth
 * waiting for, so `unavailable` is not a permanent verdict — lives there,
 * where a test can drive it with no DOM and no Tauri host (issue #29).
 *
 * This is the only place the window reaches the Tauri API for the ping.
 */
export function useBackendPing(): BackendConnection {
  const [connection, setConnection] = useState<BackendConnection>(INITIAL_CONNECTION);

  useEffect(() => watchBackendConnection(() => invoke("ping"), setConnection), []);

  return connection;
}
