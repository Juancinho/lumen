// @ts-check
import js from "@eslint/js";
import jsxA11y from "eslint-plugin-jsx-a11y";
import reactHooks from "eslint-plugin-react-hooks";
import reactRefresh from "eslint-plugin-react-refresh";
import globals from "globals";
import tseslint from "typescript-eslint";

export default tseslint.config(
  { ignores: ["dist", "src-tauri", "node_modules", "coverage"] },
  {
    files: ["**/*.{ts,tsx}"],
    extends: [
      js.configs.recommended,
      tseslint.configs.strictTypeChecked,
      tseslint.configs.stylisticTypeChecked,
      reactHooks.configs.flat["recommended-latest"],
      reactRefresh.configs.vite,
      jsxA11y.flatConfigs.strict,
    ],
    languageOptions: {
      ecmaVersion: 2022,
      globals: globals.browser,
      parserOptions: {
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
      },
    },
    rules: {
      // Shell boundary: only `src/ipc/` may talk to Tauri. Everything else consumes
      // typed functions from `src/ipc`, keeping the React UI independent of the
      // native bridge (ADR-002) and trivially mockable in tests.
      "no-restricted-imports": [
        "error",
        {
          patterns: [
            {
              group: ["@tauri-apps/*"],
              message:
                "Import Tauri APIs only inside src/ipc/. Use the typed wrappers from src/ipc.",
            },
          ],
        },
      ],
      "@typescript-eslint/consistent-type-imports": "error",
    },
  },
  {
    files: ["src/ipc/**/*.{ts,tsx}"],
    rules: { "no-restricted-imports": "off" },
  },
  {
    files: ["*.config.{ts,js}"],
    languageOptions: { globals: globals.node },
  },
  {
    files: ["eslint.config.js"],
    extends: [tseslint.configs.disableTypeChecked],
  },
);
