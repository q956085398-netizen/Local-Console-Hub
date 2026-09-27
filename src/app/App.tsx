import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { isPingResponse, type PingResponse } from "../types/ipc";

/** Render an unknown rejection as an understandable message (DEVELOPMENT.md §9). */
function toErrorMessage(reason: unknown): string {
  if (typeof reason === "string") return reason;
  if (reason instanceof Error) return reason.message;
  return JSON.stringify(reason) ?? String(reason);
}

/**
 * Bootstrap shell (T00).
 *
 * Purpose: prove the typed frontend → backend ping command works end to end.
 * This is deliberately not the V2 workspace UI — that arrives with T06 (#7)
 * per docs/UI_STYLE_GUIDE.md.
 */
export default function App() {
  const [ping, setPing] = useState<PingResponse | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    invoke("ping")
      .then((value) => {
        if (cancelled) return;
        if (isPingResponse(value)) {
          setPing(value);
        } else {
          setError("后端返回了不符合契约的 ping 响应");
        }
      })
      .catch((reason: unknown) => {
        if (!cancelled) setError(toErrorMessage(reason));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <main style={{ display: "flex", flexDirection: "column", minHeight: "100%" }}>
      <header
        style={{
          borderBottom: "1px solid var(--border-subtle)",
          padding: "10px 16px",
          fontWeight: 600,
        }}
      >
        Local Console Hub
      </header>
      <section style={{ padding: "16px", color: "var(--text-secondary)" }}>
        {error ? (
          <p style={{ color: "var(--danger)" }}>后端连接失败：{error}</p>
        ) : ping ? (
          <p>
            后端已连接：{ping.appName} v{ping.appVersion}（IPC 协议 {ping.protocol}）
          </p>
        ) : (
          <p>正在连接后端…</p>
        )}
      </section>
    </main>
  );
}
