import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { isPingResponse } from "../types/ipc";

/** What the status bar can say about the backend connection. */
export type BackendConnection =
  { state: "pending" } | { state: "connected"; version: string } | { state: "unavailable" };

function toErrorMessage(reason: unknown): string {
  if (typeof reason === "string") return reason;
  if (reason instanceof Error) return reason.message;
  return JSON.stringify(reason) ?? String(reason);
}

/**
 * The T00 bootstrap's typed ping, kept alive as a quiet status-bar signal.
 * In a plain browser preview `invoke` rejects, which renders as "预览" — the
 * V2 shell is fixture-driven until T07–T10, so that is the honest label.
 */
export function useBackendPing(): BackendConnection {
  const [connection, setConnection] = useState<BackendConnection>({ state: "pending" });

  useEffect(() => {
    let cancelled = false;
    invoke("ping")
      .then((value: unknown) => {
        if (cancelled) return;
        if (isPingResponse(value)) {
          setConnection({ state: "connected", version: value.appVersion });
        } else {
          setConnection({ state: "unavailable" });
        }
      })
      .catch((reason: unknown) => {
        if (!cancelled) {
          // Swallow the raw transport error; the bar only needs the state.
          void toErrorMessage(reason);
          setConnection({ state: "unavailable" });
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  return connection;
}
