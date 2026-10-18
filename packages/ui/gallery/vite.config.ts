import { fileURLToPath } from "node:url"
import { defineConfig } from "vite"
import react from "@vitejs/plugin-react"
import tailwindcss from "@tailwindcss/vite"
import { referenceTheme } from "@rostfrei/ui/vite"

export default defineConfig({
  root: fileURLToPath(new URL(".", import.meta.url)),
  plugins: [referenceTheme(), react(), tailwindcss()],
  server: { host: "127.0.0.1", port: 4177 },
})
