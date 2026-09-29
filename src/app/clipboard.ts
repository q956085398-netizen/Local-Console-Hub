/**
 * Putting a path on the system clipboard, for the surfaces that offer a
 * 复制路径 control: a run row's log file (T10) and the selected-session header's
 * working directory.
 *
 * The path is already in hand on both sides — the header copies `config.cwd`
 * out of the DTO it is rendering, a run row copies the file the backend
 * reported — so nothing here asks a command for it (D-021's rule is about
 * commands resolving paths, not about the frontend re-reading one it was given).
 *
 * ## Why the outcome is said out loud
 *
 * `navigator.clipboard` only exists in a secure context — a hosted preview is
 * not one — and a granted write can still be refused by the browser. Both cases
 * fall back to naming the text so the user can copy it by hand: the notice is
 * the only channel these surfaces have, and a failure nobody mentions reads as
 * a copy that worked.
 */

/**
 * Put `path` on the clipboard, reporting the outcome through `onNotice`.
 *
 * Reports the path itself in all three endings, including the ones that failed:
 * the value is the thing the user wanted, and a notice that only said "failed"
 * would leave them to find it again.
 */
export function copyPathToClipboard(path: string, onNotice: (message: string) => void): void {
  const written = navigator.clipboard?.writeText(path);
  if (written === undefined) {
    onNotice(`复制不可用，请手动复制：${path}`);
    return;
  }
  written
    .then(() => onNotice(`已复制路径：${path}`))
    .catch(() => onNotice(`复制失败，请手动复制：${path}`));
}
