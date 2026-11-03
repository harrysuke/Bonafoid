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
    port: 1420,
    strictPort: true,
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
        // Chunk rules only for packages the host actually imports; the
        // monaco / dnd-kit / zod / react-hook-form branches went with the
        // dependencies.
        manualChunks(id) {
          if (!id.includes('node_modules')) return undefined;
          if (id.includes('reactflow')) return 'flow';
          if (id.includes('@tauri-apps')) return 'tauri';
          return 'vendor';
        },
      },
    },
  },
});
