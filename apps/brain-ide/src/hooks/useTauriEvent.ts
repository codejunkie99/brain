import { useEffect } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";

/**
 * Subscribe to a Tauri event that returns an `UnlistenFn`. Handles
 * the unlisten cleanup so callers don't have to chase async cleanup.
 */
export function useTauriEvent<T>(
  subscribe: (fn: (payload: T) => void) => Promise<UnlistenFn>,
  handler: (payload: T) => void,
  deps: unknown[],
) {
  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    let cancelled = false;
    void subscribe((payload) => {
      if (!cancelled) handler(payload);
    }).then((fn) => {
      if (cancelled) {
        fn();
      } else {
        unlisten = fn;
      }
    });
    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
}
