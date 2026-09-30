import { useEffect, useState } from "react";
import { X } from "lucide-react";
import {
  candidateResolution,
  type ExternalCandidateDto,
  type OpenChoiceDto,
  type OpenResolutionDto,
} from "../../types/runtime";
import "../dialog/dialog.css";
import "./OpenChoiceDialog.css";

export interface OpenChoiceDialogProps {
  /** The application the user clicked, for the heading. */
  sessionName: string;
  /** What the Hub found, and why it will not decide about it. */
  choice: OpenChoiceDto;
  /** Answer, in the user's words. */
  onResolve: (resolution: OpenResolutionDto) => void;
  /** Leave everything as it is. */
  onClose: () => void;
}

/**
 * The Hub found something running outside itself and will not decide about it
 * (#67, spec #59 decision 11).
 *
 * Three answers, and the third is the point: the user may associate the
 * instance they recognise, ask for the Hub's own copy, or close this and do
 * nothing. What the dialog must never offer is a default —自动 pick one — because
 * the Hub cannot tell from a name, a title or a port which of two running
 * programs is the application the entry names, and the user can.
 *
 * ## What it says about each instance
 *
 * Everything the Hub actually looked at: the image path it verified, the
 * window caption, and why this one is not certain. An instance it could not
 * inspect is still listed — knowing that *something* is running is what stops
 * the Hub from silently starting a second copy — but it cannot be chosen,
 * because there is no identity the Hub could remember and check again
 * (`ExternalCandidateDto.associable`), and the row says so rather than
 * offering a control that would be refused.
 */
export default function OpenChoiceDialog({
  sessionName,
  choice,
  onResolve,
  onClose,
}: OpenChoiceDialogProps) {
  const associable = choice.candidates.filter((candidate) => candidate.associable);
  const [selected, setSelected] = useState<string | null>(() =>
    associable[0] === undefined ? null : key(associable[0]),
  );
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  const chosen = choice.candidates.find((candidate) => key(candidate) === selected);

  const resolve = (resolution: OpenResolutionDto) => {
    if (submitting) return;
    setSubmitting(true);
    onResolve(resolution);
  };

  return (
    <div className="dialog__backdrop" role="presentation">
      <div className="dialog" role="dialog" aria-modal="true" aria-labelledby="open-choice-title">
        <header className="dialog__head">
          <h2 className="dialog__title" id="open-choice-title">
            「{sessionName}」可能已经在运行
          </h2>
          <button type="button" className="dialog__close" aria-label="关闭" onClick={onClose}>
            <X size={14} />
          </button>
        </header>

        <p className="dialog__lead">
          {choice.reason}
          <br />
          关联和启动是两件事：关联只是在列表里认下已经在运行的那一份，Hub
          不会因此获得结束它的权限；新开一份则由 Hub 启动一个自己的进程，此前运行的那份不受影响。
        </p>

        <div className="dialog__body">
          <ul className="open-choice__list">
            {choice.candidates.map((candidate) => {
              const id = key(candidate);
              return (
                <li key={id} className="open-choice__item">
                  <label className="open-choice__row">
                    <input
                      type="radio"
                      name="open-choice-candidate"
                      value={id}
                      checked={selected === id}
                      disabled={!candidate.associable}
                      onChange={() => setSelected(id)}
                    />
                    <span className="open-choice__text">
                      <span className="open-choice__label">{labelOf(candidate)}</span>
                      <span className="open-choice__detail">
                        进程 {candidate.pid}
                        {candidate.imagePath === undefined ? "" : ` · ${candidate.imagePath}`}
                      </span>
                      <span className="open-choice__why">{candidate.why}</span>
                    </span>
                  </label>
                </li>
              );
            })}
          </ul>

          {associable.length === 0 && (
            <p className="open-choice__none" role="status">
              没有可以关联的实例：Hub
              无法确认上面任何一个进程就是这条配置的应用，因此只能选择新开一份，或者先取消。
            </p>
          )}
        </div>

        <footer className="dialog__foot">
          <button type="button" className="btn btn--secondary btn--sm" onClick={onClose}>
            取消
          </button>
          <button
            type="button"
            className="btn btn--secondary btn--sm"
            disabled={submitting}
            onClick={() => resolve({ kind: "new" })}
          >
            新开一份
          </button>
          <button
            type="button"
            className="btn btn--primary btn--sm"
            disabled={submitting || chosen === undefined}
            onClick={() => chosen !== undefined && resolve(candidateResolution(chosen))}
          >
            关联选中的实例
          </button>
        </footer>
      </div>
    </div>
  );
}

/**
 * What one row is called.
 *
 * The window caption when there is one, because that is what the user sees on
 * screen and can match against; otherwise the file name. Never the caption
 * alone for *finding* anything — this is a label, and the identity beside it is
 * what an answer carries back.
 */
function labelOf(candidate: ExternalCandidateDto): string {
  const title = candidate.title;
  if (title !== undefined && title !== "") return title;
  return candidate.fileName;
}

/** A candidate's identity as a React key and a radio value. */
function key(candidate: ExternalCandidateDto): string {
  return `${candidate.pid}:${candidate.createdAt ?? ""}`;
}
