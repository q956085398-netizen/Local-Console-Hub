/**
 * What a terminal view has rendered, and how the next batch joins it (T07 #8).
 *
 * A view can be created at any moment — when the user selects a session, and
 * again when the window is shown after being hidden — but the session it is
 * looking at is not restarted by either. So it attaches to a stream already in
 * flight: the backend hands over the retained scrollback plus the byte offset
 * that scrollback reaches (`TerminalAttachmentDto`), and the live batches that
 * follow carry the range of the run's stream they cover.
 *
 * That makes the rule small, and it is the whole reason the backend tracks
 * offsets:
 *
 * - a batch ending at or before the offset the view attached at has already
 *   been replayed → drop it, or the user sees the same output twice;
 * - a batch from another generation belongs to a run this view is not showing
 *   → drop it, or a restart's output appears under the run that ended;
 * - anything else is new → render it whole, and move the view's position to
 *   its end.
 *
 * The backend guarantees no batch straddles an attachment offset, so "render
 * whole" never needs to slice a batch — an invariant that is asserted
 * backend-side in `session::core`'s attachment test. What this module *does*
 * have to tolerate is a gap: a batch that starts past the position the view
 * reached. That cannot happen while a listener is registered, and the state
 * records it rather than hiding it, because a stream with a hole in it is
 * something the user should be told about.
 *
 * The bytes are decoded here rather than in the component so the rule and the
 * decoding stay in one tested place.
 */

import {
  decodeBase64,
  type TerminalAttachmentDto,
  type TerminalOutputDto,
} from "../types/terminal";

/** Where a view is in the stream it renders. */
export interface TerminalStreamState {
  /** The run this view is showing; `null` before it has attached. */
  generation: number | null;
  /** How far into that run's stream the view has rendered, in bytes. */
  shown: number;
  /** Whether any part of the stream was missed between batches. */
  gapped: boolean;
}

/** What one batch means for a view. */
export interface BatchOutcome {
  /** The view's position afterwards. */
  state: TerminalStreamState;
  /** Byte-identical to what the backend sent; `null` when nothing is new. */
  bytes: Uint8Array | null;
}

/** The state a view starts in, from the attachment it was handed. */
export function streamFromAttachment(attachment: TerminalAttachmentDto): TerminalStreamState {
  return {
    generation: attachment.generation,
    // The chunks the caller just replayed cover exactly this many bytes of the
    // run's stream, so this — and not zero — is where live output begins.
    shown: attachment.emitted,
    gapped: false,
  };
}

/** Fold one live batch into a view's position. */
export function applyBatch(state: TerminalStreamState, batch: TerminalOutputDto): BatchOutcome {
  if (state.generation === null || batch.generation !== state.generation) {
    // Another run's output. A view that swapped it in would be showing two
    // runs at once, and the run it is attached to is still the one it was.
    return { state, bytes: null };
  }

  if (batch.end <= state.shown) {
    return { state, bytes: null };
  }

  return {
    state: {
      ...state,
      shown: batch.end,
      // A batch that begins after the view's position means bytes between were
      // never rendered. Recorded, not repaired: inventing them is impossible,
      // and silence would make the terminal look complete when it is not.
      gapped: state.gapped || batch.start > state.shown,
    },
    bytes: decodeBase64(batch.data),
  };
}
