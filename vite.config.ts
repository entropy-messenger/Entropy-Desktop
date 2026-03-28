import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

// https://vite.dev/config/
export default defineConfig({
  plugins: [svelte()],
  resolve: {
    conditions: ['browser']
  },
  server: {
    watch: {
      ignored: ['**/src-tauri/**']
    }
  },
  build: {
    minify: 'esbuild',
    sourcemap: false,
    reportCompressedSize: false,
    cssCodeSplit: true,
    chunkSizeWarningLimit: 1000
  }
})
