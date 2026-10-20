import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { exavViewer } from '@exav/viewer/vite';
import { fileURLToPath } from 'url';
import { dirname, resolve } from 'path';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const host = process.env.TAURI_DEV_HOST;

export default defineConfig(async () => ({
  plugins: [react(), exavViewer()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: 'ws',
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // Trunk'ın geçici dosyaları Windows'ta derleme sırasında kilitlenebilir.
      ignored: ['**/src-tauri/**', '**/.stage/**'],
    },
  },
  resolve: {
    alias: {
      '@': resolve(__dirname, './src'),
      // CAD worker'ı düzeltilmiş motoru kullanır; node_modules değişmez.
      '../../wasm/exav_viewer_dwg.js': resolve(__dirname, '../../.cache/cad-viewer/wasm/exav_viewer_dwg.js'),
    },
  },
}));
