import js from "@eslint/js";
import reactHooks from "eslint-plugin-react-hooks";
import tseslint from "typescript-eslint";

export default tseslint.config(
  {
    // `.claude/worktrees/` holds the app's per-session checkouts of this repo.
    // Git already excludes them; without this the TS parser sees two candidate
    // tsconfig roots and refuses to lint anything at all.
    ignores: [
      "dist/",
      "node_modules/",
      "src-tauri/target/",
      "src-tauri/gen/",
      ".claude/worktrees/",
    ],
  },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ["**/*.{ts,tsx}"],
    plugins: {
      "react-hooks": reactHooks,
    },
    rules: {
      ...reactHooks.configs.recommended.rules,
    },
  },
  {
    // Standalone Node scripts (icon generator, UI-state capture) need
    // Node/browser-standard globals that no-undef otherwise flags. `document`
    // and `setTimeout` are here rather than under `src/` because they are used
    // inside `page.evaluate` callbacks — code that runs in the captured
    // browser, not in the script.
    files: ["scripts/**/*.mjs"],
    languageOptions: {
      globals: {
        console: "readonly",
        process: "readonly",
        fetch: "readonly",
        AbortSignal: "readonly",
        URL: "readonly",
        document: "readonly",
        setTimeout: "readonly",
      },
    },
  },
);
