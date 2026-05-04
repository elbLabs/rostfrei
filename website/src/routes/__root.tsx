import { TanStackDevtools } from "@tanstack/react-devtools"
import { Outlet, createRootRoute } from "@tanstack/react-router"
import { TanStackRouterDevtoolsPanel } from "@tanstack/react-router-devtools"

import { ThemeProvider } from "@/components/theme-provider"
import "../index.css"

export const Route = createRootRoute({
  component: RootComponent,
})

function RootComponent() {
  return (
    <ThemeProvider>
      <div className="min-h-svh bg-background text-foreground">
        <Outlet />
      </div>
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
    </ThemeProvider>
  )
}
