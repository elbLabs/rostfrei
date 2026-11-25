import path from "path"
import tailwindcss from "@tailwindcss/vite"
import react from "@vitejs/plugin-react"
import { defineConfig, loadEnv } from "vite"
import { referenceTheme } from "@rostfrei/ui/vite"

export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd(), "")
  // The optional evaluation API has no CORS either. A relative
  // VITE_EVAL_API_URL is proxied to the evaluation application.
  const evaluationPath = env.VITE_EVAL_API_URL?.replace(/\/$/, "")

  return {
    plugins: [referenceTheme(), react(), tailwindcss()],
    resolve: {
      alias: {
        "@": path.resolve(import.meta.dirname, "./src"),
      },
    },
    server: {
      proxy: {
        "/api": {
          target: env.VITE_TRACER_TARGET ?? "http://127.0.0.1:1309",
          changeOrigin: true,
          rewrite: (requestPath) => requestPath.replace(/^\/api/, ""),
        },
        ...(evaluationPath?.startsWith("/") && evaluationPath !== "/api"
          ? {
              [evaluationPath]: {
                target: env.VITE_EVAL_TARGET ?? "http://127.0.0.1:8095",
                changeOrigin: true,
                rewrite: (requestPath: string) =>
                  requestPath.slice(evaluationPath.length),
              },
            }
          : {}),
      },
    },
  }
})
