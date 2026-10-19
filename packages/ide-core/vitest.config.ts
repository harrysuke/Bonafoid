import { defineConfig } from 'vitest/config';

/**
 * Tests for the shared IDE core.
 *
 * The suite deliberately covers logic, not rendering: the model utils and the
 * zustand store are hand-written, stateful and easy to break from a refactor,
 * while a component-rendering harness would add jsdom and a testing-library
 * before anything catches a real defect. Add those when there is a rendering
 * behaviour worth pinning.
 */
export default defineConfig({
  test: {
    environment: 'node',
    include: ['src/**/*.test.ts'],
    // Store tests mutate module-level state (autosave timers, working copies).
    // Running files sequentially keeps them independent.
    fileParallelism: false,
    restoreMocks: true,
  },
});