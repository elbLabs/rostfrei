import { StrictMode } from "react"
import ReactDOM from "react-dom/client"
import { RouterProvider } from "@tanstack/react-router"

import { router } from "./router"

const rootElement = document.getElementById("app")!

if (!rootElement.innerHTML) {
  const root = ReactDOM.createRoot(rootElement)
  root.render(
    <StrictMode>
      <RouterProvider router={router} />
    </StrictMode>
  )
}
