import { defineConfig } from 'vite';

const apiProxy = {
  target: 'http://127.0.0.1:1309',
  changeOrigin: true,
  rewrite: (path: string): string => path.replace(/^\/api/u, ''),
};

export default defineConfig({
  server: {
    proxy: {
      '/api': apiProxy,
    },
  },
  preview: {
    proxy: {
      '/api': apiProxy,
    },
  },
});
