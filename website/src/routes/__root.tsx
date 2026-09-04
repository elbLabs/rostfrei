import { TanStackDevtools } from "@tanstack/react-devtools"
import { Outlet, createRootRoute } from "@tanstack/react-router"
import { TanStackRouterDevtoolsPanel } from "@tanstack/react-router-devtools"

import { ThemeProvider } from "@/components/theme-provider"
import { PageMetadata } from "@/components/page-shell/PageMetadata"
import duckIcon from "@rostfrei/ui/brand/duck-icon.png"
import "../index.css"

export const Route = createRootRoute({
  component: RootComponent,
})

function RootComponent() {
  return (
    <ThemeProvider defaultTheme="system" storageKey="rostfrei-theme">
      <link rel="icon" type="image/png" href={duckIcon} />
      <PageMetadata />
      <div className="min-h-svh bg-background text-foreground">
        <Outlet />
      </div>
      {import.meta.env.DEV && (
        <TanStackDevtools
          config={{
            position: "bottom-right",
          }}
          plugins={[
            {
              name: "TanStack Router",
              render: <TanStackRouterDevtoolsPanel />,
            },
          ]}
        />
      )}
    </ThemeProvider>
  )
}
