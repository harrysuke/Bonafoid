/**
 * Shared ESLint configuration for the npm workspace (ESLint 8, eslintrc mode).
 *
 * One root config covers `packages/ide-core`, `apps/ide-web` and
 * `apps/ide-tauri` because ESLint cascades configs from the linted file's
 * directory upwards.
 */
module.exports = {
  root: true,
  env: { browser: true, es2022: true, node: true },
  parser: '@typescript-eslint/parser',
  parserOptions: {
    ecmaVersion: 'latest',
    sourceType: 'module',
    ecmaFeatures: { jsx: true },
  },
  plugins: ['@typescript-eslint'],
  extends: ['eslint:recommended', 'plugin:@typescript-eslint/recommended'],
  settings: { react: { version: 'detect' } },
  ignorePatterns: [
    'node_modules/',
    'dist/',
    'target/',
    '**/dist/**',
    '**/target/**',
    '*.config.js',
    '*.config.mjs',
    '*.d.ts',
  ],
  rules: {
    // The IDE intentionally casts at one documented boundary (the editor <->>
    // generated Rust model shapes) and treats some JSON-ish props as unknown.
    '@typescript-eslint/no-explicit-any': 'error',
    '@typescript-eslint/no-unused-vars': [
      'error',
      { argsIgnorePattern: '^_', varsIgnorePattern: '^_' },
    ],
    '@typescript-eslint/consistent-type-imports': [
      'error',
      { prefer: 'type-imports', fixStyle: 'inline-type-imports' },
    ],
    'no-console': ['warn', { allow: ['warn', 'error'] }],
    eqeqeq: ['error', 'smart'],
    'prefer-const': 'error',
  },
  overrides: [
    {
      // Test files may lean on non-null assertions and console output.
      files: ['**/*.test.ts', '**/*.test.tsx', '**/scripts/**'],
      rules: {
        'no-console': 'off',
        '@typescript-eslint/no-non-null-assertion': 'off',
      },
    },
  ],
};