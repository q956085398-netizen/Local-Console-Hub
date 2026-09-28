import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { isPingResponse } from "../types/ipc";

/** What the status bar can say about the backend connection. */
export type BackendConnection =
  { state: "pending" } | { state: "connected"; version: string } | { state: "unavailable" };

/**
 * The T00 bootstrap's typed ping, kept alive as a quiet status-bar signal.
 *
 * Only the connection state matters here: a rejection (in a browser preview
 * `invoke` has no host to call) collapses to `unavailable`, which is what
 * labels the shell a preview. The transport error itself is deliberately not
 * surfaced — a browser preview rejecting is the expected case, not a fault to
 * report, and the real error path belongs to whichever command failed.
 */
export function useBackendPing(): BackendConnection {
  const [connection, setConnection] = useState<BackendConnection>({ state: "pending" });

  useEffect(() => {
    let cancelled = false;
    invoke("ping")
      .then((value: unknown) => {
        if (cancelled) return;
        setConnection(
          isPingResponse(value)
            ? { state: "connected", version: value.appVersion }
            : { state: "unavailable" },
        );
      })
      .catch(() => {
        if (!cancelled) setConnection({ state: "unavailable" });
      });
    return () => {
      cancelled = true;
    };
  }, []);

  return connection;
}
