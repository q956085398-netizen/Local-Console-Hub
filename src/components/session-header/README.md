# components/session-header/

The selected-session header: identity, type and lifecycle badges, purpose,
the close-impact callout, the compact metadata line and the action group
(T06 #7, docs/UI_STYLE_GUIDE.md §5).

Button availability derives from `availableActions` in
`src/state/derivations.ts` (lifecycle rules of MVP_IMPLEMENTATION_SPEC.md §5)
— never UI guesses. Start and Stop are mutually exclusive; the overflow menu
holds the low-frequency and destructive actions (directory, focus terminal,
log policy, copy path, force-kill the managed tree per D-007). The two that act
on the working directory — `目录` plus the menu's `打开目录`, and the menu's
`复制路径` — are offered only where the config has a `cwd` to act on.

The header reports an action by label and the *caller* decides what it means
(`App.tsx`): with a backend connected, `启动` / `停止` / `重启` /
`强制结束进程树` are Session Core commands; in the preview workspace there is no
run behind them, so they surface a notice instead of pretending to act. The
buttons themselves are identical in both modes — the lifecycle rules they
render are the same rules either way. `复制路径` is the exception in both
directions: it needs no command (the path is already in the config this header
renders, `src/app/clipboard.ts`), so it copies in either mode and reports what
the clipboard did.
