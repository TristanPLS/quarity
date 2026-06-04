import { defineConfig, loadEnv } from 'vite'
import react from '@vitejs/plugin-react'

// https://vite.dev/config/
export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd(), '')
  // Cible du proxy /api en dev (npm run dev) -> le back. Évite tout CORS en dev.
  const apiTarget = env.VITE_API_PROXY_TARGET || 'http://localhost:8080'

  return {
    plugins: [react()],
    base: '/',
    server: {
      port: 5173,
      proxy: {
        '/api': { target: apiTarget, changeOrigin: true },
      },
    },
    build: {
      outDir: 'dist',
      sourcemap: false,
    },
  }
})
