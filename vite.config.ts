import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';
import { version } from './package.json';
// The header badge shows the released version from package.json, never a hardcoded one.
export default defineConfig({ plugins: [vue()], define: { __APP_VERSION__: JSON.stringify(version) }, server: { port: 1420, strictPort: true } });
