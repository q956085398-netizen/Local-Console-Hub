import { useCallback, useSyncExternalStore } from "react";

/**
 * Track a CSS media query from React (SSR-safe no-op fallback). Used to move
 * the session sidebar into a drawer at narrow widths (UI_STYLE_GUIDE §11:
 * same hierarchy, terminal space keeps priority).
 */
export function useMediaQuery(query: string): boolean {
  const subscribe = useCallback(
    (onChange: () => void) => {
      const list = window.matchMedia(query);
      list.addEventListener("change", onChange);
      return () => list.removeEventListener("change", onChange);
    },
    [query],
  );
  return useSyncExternalStore(
    subscribe,
    () => window.matchMedia(query).matches,
    () => false,
  );
}
