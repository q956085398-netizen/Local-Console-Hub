/**
 * Shared bounded retry cadence for startup and transport recovery. Keeping the
 * schedule in one place prevents separate state watchers from drifting into
 * different retry behavior.
 */
export const RETRY_DELAYS_MS: readonly number[] = [500, 1_000, 2_000, 5_000, 10_000, 30_000];

/** The gap before the attempt after `failedAttempts` consecutive failures. */
export function retryDelayMs(failedAttempts: number): number {
  const last = RETRY_DELAYS_MS.length - 1;
  return RETRY_DELAYS_MS[Math.min(Math.max(failedAttempts, 0), last)];
}
