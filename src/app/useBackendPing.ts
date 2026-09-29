import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { watchBackendConnection, type BackendConnection } from "../state/backend-connection";

/**
 * The shell's view of the backend connection (#29).
 *
 * An adapter and nothing more: it supplies the real `invoke("ping")` and holds
 * the decision `src/state/backend-connection.ts` reports, which is where the
 * question itself lives — when to ask again, what a rejection means, and the
 * fact that a lost request is not the end of the connection. Keeping the Tauri
 * call on this side is what lets that module be driven by a test with no host
 * and no DOM, and the effect's cleanup is what stops the asking when the shell
 * goes away.
 */
export function useBackendPing(): BackendConnection {
  const [connection, setConnection] = useState<BackendConnection>({ state: "pending" });
  useEffect(() => watchBackendConnection(() => invoke("ping"), setConnection), []);
  return connection;
}
