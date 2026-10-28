// ESLint flat config (ESLint 10, typescript-eslint 8, react-hooks 7).
// Type-aware rules run on src/**; plain JS config files get syntax-only linting.
import js from "@eslint/js";
import reactHooks from "eslint-plugin-react-hooks";
import globals from "globals";
import tseslint from "typescript-eslint";
import { defineConfig } from "eslint/config";

export default defineConfig(
  {
    ignores: [
      "dist/**",
      "node_modules/**",
      "target/**",
      "src-tauri/**",
      "crates/**",
      "packaging/**",
      "coverage/**",
      ".vnl-dev/**",
    ],
  },

  js.configs.recommended,

  // TypeScript sources: type-aware recommended rules via the project service.
  {
    files: ["**/*.{ts,tsx,mts,cts}"],
    extends: [...tseslint.configs.recommendedTypeChecked],
    languageOptions: {
      ecmaVersion: 2022,
      sourceType: "module",
      globals: { ...globals.browser },
      parserOptions: {
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
      },
    },
    rules: {
      // Allow intentionally-unused args/vars when prefixed with `_` (mirrors tsc's noUnused* escape hatch).
      "@typescript-eslint/no-unused-vars": [
        "error",
        { argsIgnorePattern: "^_", varsIgnorePattern: "^_", caughtErrorsIgnorePattern: "^_" },
      ],
      // Fire-and-forget promises are common in React event handlers (`onClick={() => void save()}` is preferred).
      "@typescript-eslint/no-misused-promises": ["error", { checksVoidReturn: { attributes: false } }],
      "@typescript-eslint/consistent-type-imports": ["warn", { fixStyle: "inline-type-imports" }],
    },
  },

  // React hooks rules (incl. the React Compiler-backed rules shipped in v7).
  {
    files: ["src/**/*.{ts,tsx}"],
    ...reactHooks.configs.flat.recommended,
  },

  // Tests: vitest globals, relax a few type-aware rules that fight with mocks.
  {
    files: ["src/**/*.test.{ts,tsx}", "src/test/**"],
    languageOptions: { globals: { ...globals.browser, ...globals.node, ...globals.vitest } },
    rules: {
      "@typescript-eslint/no-unsafe-assignment": "off",
      "@typescript-eslint/no-unsafe-member-access": "off",
      "@typescript-eslint/no-unsafe-call": "off",
      "@typescript-eslint/no-unsafe-argument": "off",
      "@typescript-eslint/unbound-method": "off",
    },
  },

  // Node-side JS config files (this file, scripts) are not part of tsconfig.
  {
    files: ["**/*.{js,mjs,cjs}"],
    languageOptions: { globals: { ...globals.node } },
  },
);
