// Browser-test entry point. Never imported by the production bundle.
import { createRoot } from "react-dom/client"
import { ThemeProvider } from "@rostfrei/ui/theme"
import { InsightsWorkspace } from "../src/InsightsApp"
import type { InsightsSession } from "../src/lib/insights"
import "../src/index.css"

const session: InsightsSession = {
  subject: "support-engineer-42",
  clientId: "studio",
  roles: [
    "insights.read",
    "insights.request",
    "insights.approve",
    "insights.policy.manage",
    "insights.audit.read",
  ],
  async request<T>(path: string, body?: unknown, method?: string): Promise<T> {
    const response = await fetch(`/api${path}`, {
      method: method ?? (body === undefined ? "GET" : "POST"),
      headers: { "content-type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
    })
    if (!response.ok) throw new Error("Fixture request rejected.")
    return (await response.json()) as T
  },
  logout() {
    location.reload()
  },
}

createRoot(document.getElementById("root")!).render(
  <ThemeProvider defaultTheme="system" storageKey="rostfrei-theme">
    <InsightsWorkspace session={session} />
  </ThemeProvider>
)
