import { defineConfig } from 'vite'

export default defineConfig({
  resolve: { alias: { '@': new URL('.', import.meta.url).pathname } },
  clearScreen: false,
  server: {
    host: '127.0.0.1',
    port: 1420,
    strictPort: true,
    // .build holds the Rust toolchain, pnpm store and sidecar sources; watching
    // it exhausts the inotify limit before the dev server finishes starting.
    watch: {
      ignored: ['**/.build/**', '**/src-tauri/**', '**/dist/**'],
      // The inotify quota is shared with editors and other projects. Polling
      // keeps development usable even when those processes exhaust the quota.
      // CHOKIDAR_USEPOLLING=false opts back into native watching.
      usePolling: true,
      interval: 300,
    },
  },
  build: { target: 'es2021' },
})
