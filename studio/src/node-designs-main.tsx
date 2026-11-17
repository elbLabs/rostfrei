import { StrictMode } from "react"
import { createRoot } from "react-dom/client"
import { ThemeProvider } from "@rostfrei/ui/theme"
import NodeDesigns from "@/node-designs"
import "@/index.css"
import "@/node-designs.css"

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <ThemeProvider defaultTheme="light" storageKey="rostfrei-node-study-theme">
      <NodeDesigns />
    </ThemeProvider>
  </StrictMode>
)
