import { useState } from "react";
import { requestConfirmedEnd } from "../../state/confirm-end";
import type { EndTarget } from "../../types/confirm-end";

/**
 * The confirm step shared by the ports process view and the pre-start prompt.
 *
 * Asking does not end anyone. Cancel clears the question. Confirm is the only
 * call to the backend.
 */
export function useConfirmedEnd(onEnded?: () => void) {
  const [pending, setPending] = useState<EndTarget | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  const ask = (target: EndTarget) => {
    if (busy) return;
    setMessage(null);
    setPending(target);
  };

  const cancel = () => {
    if (busy) return;
    setPending(null);
    setMessage(null);
  };

  const confirm = () => {
    if (!pending || busy) return;
    const target = pending;
    setBusy(true);
    void requestConfirmedEnd(target).then((result) => {
      setBusy(false);
      if (result.ended) {
        setPending(null);
        setMessage(null);
        onEnded?.();
      } else {
        setMessage(result.message);
      }
    });
  };

  return { pending, busy, message, ask, cancel, confirm };
}
