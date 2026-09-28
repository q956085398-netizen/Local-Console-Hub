/**
 * Frontend mirror of the terminal contract (T07 #8).
 *
 * The authoritative definitions live in Rust
 * (`src-tauri/src/session/terminal.rs` for the attachment,
 * `src-tauri/src/session/event.rs` for the live batches); these types and their
 * runtime guards are the typed frontend side of the same contract, exactly as
 * `runtime.ts` mirrors the session snapshots.
 *
 * Bytes travel base64-encoded in both directions. A terminal's stream is not
 * text: a read from the console host can end inside a multi-byte character, and
 * a view that decoded each chunk as text would render the seams as replacement
 * characters. The helpers below are the only place this module touches that
 * encoding, so no other file has to know about it.
 */

import type { BufferSummaryDto } from "./runtime";
import { isBufferSummaryDto } from "./runtime";

/** Which stream a retained chunk came from (`logging::buffer::Stream`). */
export type StreamValue = "stdout" | "stderr";

const STREAMS: readonly StreamValue[] = ["stdout", "stderr"];

/** One retained chunk of a session's scrollback, as the wire carries it. */
export interface RetainedChunkDto {
  stream: StreamValue;
  /** The chunk's bytes, base64 (standard alphabet, padded). */
  data: string;
}

/**
 * What a terminal view is given when it attaches to a session
 * (`SessionCore::terminal_attachment`).
 *
 * `chunks` is the scrollback as it stands, `emitted` is how far into the run's
 * stream that scrollback reaches, and `generation` names the run both belong
 * to. A view replays the chunks and then renders the live batches whose `end`
 * is past `emitted` — the backend guarantees no batch straddles that offset, so
 * "append whole or drop whole" is sound (`src/state/terminal-stream.ts`).
 */
export interface TerminalAttachmentDto {
  sessionId: string;
  /** Whether a live terminal is attached right now. */
  ptyAttached: boolean;
  generation: number;
  emitted: number;
  /** Oldest first. */
  chunks: RetainedChunkDto[];
  buffer: BufferSummaryDto;
}

/** One batch of a run's output (`terminal-output` event payload). */
export interface TerminalOutputDto {
  sessionId: string;
  generation: number;
  /** First byte of the run's stream this batch covers. */
  start: number;
  /** One past the last byte this batch covers. */
  end: number;
  /** The batch's bytes, base64 (standard alphabet, padded). */
  data: string;
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isCount(source: Record<string, unknown>, key: string): boolean {
  const value = source[key];
  return typeof value === "number" && Number.isInteger(value) && value >= 0;
}

/** Runtime guard for one retained chunk. */
export function isRetainedChunkDto(value: unknown): value is RetainedChunkDto {
  if (!isObject(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.stream === "string" &&
    STREAMS.includes(candidate.stream as StreamValue) &&
    typeof candidate.data === "string"
  );
}

/** Runtime guard for an attachment. */
export function isTerminalAttachmentDto(value: unknown): value is TerminalAttachmentDto {
  if (!isObject(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.sessionId === "string" &&
    typeof candidate.ptyAttached === "boolean" &&
    isCount(candidate, "generation") &&
    isCount(candidate, "emitted") &&
    Array.isArray(candidate.chunks) &&
    candidate.chunks.every(isRetainedChunkDto) &&
    isBufferSummaryDto(candidate.buffer)
  );
}

/** Runtime guard for one batch of live output. */
export function isTerminalOutputDto(value: unknown): value is TerminalOutputDto {
  if (!isObject(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.sessionId === "string" &&
    isCount(candidate, "generation") &&
    isCount(candidate, "start") &&
    isCount(candidate, "end") &&
    typeof candidate.data === "string"
  );
}

/**
 * Decode a base64 payload to the bytes it carries.
 *
 * Throws on input that is not base64: every caller has a guard-validated string
 * from the backend at hand, so a failure here is a broken contract rather than
 * user input, and the caller reports it (the terminal view surfaces it instead
 * of rendering a silently empty stream).
 */
export function decodeBase64(data: string): Uint8Array {
  const binary = atob(data);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return bytes;
}

/** Encode bytes for a call that carries them to the backend. */
export function encodeBase64(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) {
    binary += String.fromCharCode(byte);
  }
  return btoa(binary);
}
