import { describe, expect, it } from "vitest";
import { applyBatch, streamFromAttachment } from "./terminal-stream";
import {
  encodeBase64,
  type TerminalAttachmentDto,
  type TerminalOutputDto,
} from "../types/terminal";

/**
 * The rule that lets a view attach to a stream already in flight (T07 #8).
 *
 * These are the cases the backend's offset bookkeeping exists to make
 * decidable: what to do with a batch that arrived after the attachment, one
 * that arrived before it, one from a run that has since been replaced, and the
 * one case that must not be papered over — a gap.
 */

function attachment(overrides: Partial<TerminalAttachmentDto> = {}): TerminalAttachmentDto {
  return {
    sessionId: "term",
    ptyAttached: true,
    generation: 4,
    emitted: 120,
    chunks: [],
    buffer: { bytes: 120, lines: 3, droppedBytes: 0 },
    ...overrides,
  };
}

function batch(overrides: Partial<TerminalOutputDto> = {}): TerminalOutputDto {
  return {
    sessionId: "term",
    generation: 4,
    start: 120,
    end: 125,
    data: encodeBase64(new TextEncoder().encode("hello")),
    ...overrides,
  };
}

function text(bytes: Uint8Array | null): string | null {
  return bytes === null ? null : new TextDecoder().decode(bytes);
}

describe("terminal stream", () => {
  it("starts a view where the replayed scrollback ended", () => {
    const state = streamFromAttachment(attachment({ emitted: 4_096 }));
    expect(state).toEqual({ generation: 4, shown: 4_096, gapped: false });
  });

  it("renders a batch that continues the attachment", () => {
    const state = streamFromAttachment(attachment());
    const outcome = applyBatch(state, batch());

    expect(text(outcome.bytes)).toBe("hello");
    expect(outcome.state.shown).toBe(125);
    expect(outcome.state.gapped).toBe(false);
  });

  it("drops a batch the replay already covered", () => {
    const state = streamFromAttachment(attachment({ emitted: 125 }));
    const outcome = applyBatch(state, batch());

    expect(outcome.bytes).toBeNull();
    expect(outcome.state).toBe(state);
  });

  it("drops a batch that only repeats the end of the replay", () => {
    // The backend flushes what it has pending when a view attaches, so a view
    // that attaches mid-burst will be sent the batch containing bytes it
    // replayed. It must be dropped, not appended.
    const state = streamFromAttachment(attachment({ emitted: 125 }));
    const outcome = applyBatch(state, batch({ start: 100, end: 125 }));

    expect(outcome.bytes).toBeNull();
  });

  it("drops output from a run the view is not showing", () => {
    const state = streamFromAttachment(attachment({ generation: 4 }));
    const outcome = applyBatch(state, batch({ generation: 5, start: 0, end: 10 }));

    expect(outcome.bytes).toBeNull();
    expect(outcome.state.generation).toBe(4);
  });

  it("records a gap rather than rendering an incomplete stream silently", () => {
    const state = streamFromAttachment(attachment({ emitted: 120 }));
    const outcome = applyBatch(state, batch({ start: 200, end: 210 }));

    expect(text(outcome.bytes)).not.toBeNull();
    expect(outcome.state.gapped).toBe(true);
    expect(outcome.state.shown).toBe(210);
  });

  it("keeps a recorded gap recorded", () => {
    const first = applyBatch(streamFromAttachment(attachment()), batch({ start: 200, end: 210 }));
    const second = applyBatch(first.state, batch({ start: 210, end: 220 }));

    expect(second.state.gapped).toBe(true);
  });

  it("renders bytes unchanged, including a partial multi-byte character", () => {
    // The reason the transport is base64 and not text: a chunk boundary inside
    // a character must survive the trip to the renderer, which decodes UTF-8
    // across writes. "你" split after its first byte is the worst case.
    const partial = new TextEncoder().encode("你").slice(0, 1);
    const state = streamFromAttachment(attachment());
    const outcome = applyBatch(state, batch({ data: encodeBase64(partial), start: 120, end: 121 }));

    expect(outcome.bytes).toEqual(partial);
  });

  it("does not advance the view for a batch it dropped", () => {
    const state = streamFromAttachment(attachment({ emitted: 120, generation: 4 }));
    const stale = applyBatch(state, batch({ generation: 4, start: 0, end: 30 }));
    const other = applyBatch(state, batch({ generation: 9, start: 0, end: 900 }));

    expect(stale.state.shown).toBe(120);
    expect(other.state.shown).toBe(120);
  });
});
