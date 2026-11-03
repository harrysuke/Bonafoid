import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { fileURLToPath, URL } from 'node:url';

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  // `@nocode/ide-core` ships raw TypeScript sources from the workspace, so it
  // must not be pre-bundled by esbuild.
  optimizeDeps: {
    exclude: ['@nocode/ide-core'],
  },
  server: {
    port: 5173,
    strictPort: false,
    fs: {
      // Allow serving files from the monorepo root (packages/ide-core/src).
      allow: [fileURLToPath(new URL('../..', import.meta.url))],
    },
  },
  build: {
    outDir: 'dist',
    sourcemap: true,
    rollupOptions: {
      output: {
        // Only packages that are actually imported get a chunk. The monaco /
        // dnd-kit / zod / react-hook-form branches were removed with the
        // dependencies themselves; a chunk rule for a package nothing imports
        // is a rule that can never fire.
        manualChunks(id) {
          if (!id.includes('node_modules')) return undefined;
          if (id.includes('reactflow')) return 'flow';
          return 'vendor';
        },
      },
    },
  },
});
