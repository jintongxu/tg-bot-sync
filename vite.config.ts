import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
export default defineConfig({ plugins: [react()], server: { host: '127.0.0.1', port: 18080, strictPort: true }, build: { rollupOptions: { input: { main: 'index.html', progress: 'progress.html' } } } });
