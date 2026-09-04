import { StrictMode } from "react"
import { createRoot } from "react-dom/client"
import { ThemeProvider } from "@rostfrei/ui/theme"
import { Gallery } from "./Gallery"
import "./styles.css"

createRoot(document.getElementById("app")!).render(
  <StrictMode>
    <ThemeProvider>
      <Gallery />
    </ThemeProvider>
  </StrictMode>
)
