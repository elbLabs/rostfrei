import { StrictMode } from "react"
import { createRoot } from "react-dom/client"
import { ThemeProvider } from "@rostfrei/ui/theme"
import duckIcon from "@rostfrei/ui/brand/duck-icon.png"

import App from "@/App"
import InsightsApp from "@/InsightsApp"
import "@/index.css"

const favicon = document.createElement("link")
favicon.rel = "icon"
favicon.type = "image/png"
favicon.href = duckIcon
document.head.append(favicon)

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <ThemeProvider defaultTheme="system" storageKey="rostfrei-theme">
      {import.meta.env.VITE_INSIGHTS_ENABLED === "true" ? (
        <InsightsApp />
      ) : (
        <App />
      )}
    </ThemeProvider>
  </StrictMode>
)
