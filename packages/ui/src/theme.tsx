import * as React from "react"
import { CheckIcon, MonitorIcon, MoonIcon, SunIcon } from "lucide-react"
import { DropdownMenu } from "radix-ui"

import { Button, type ButtonProps } from "./components/button"
import { cn } from "./lib/utils"
import { COLOR_SCHEME_QUERY, THEME_STORAGE_KEY } from "./theme-script.js"

type Theme = "dark" | "light" | "system"
type ResolvedTheme = "dark" | "light"
type ThemeProviderProps = {
  children: React.ReactNode
  defaultTheme?: Theme
  storageKey?: string
  disableTransitionOnChange?: boolean
}
type ThemeState = {
  theme: Theme
  resolvedTheme: ResolvedTheme
  setTheme: (theme: Theme) => void
}
const ThemeContext = React.createContext<ThemeState | undefined>(undefined)

function isTheme(value: unknown): value is Theme {
  return value === "dark" || value === "light" || value === "system"
}

function readTheme(storageKey: string, fallback: Theme): Theme {
  if (typeof window === "undefined") return fallback
  try {
    const stored = window.localStorage.getItem(storageKey)
    return isTheme(stored) ? stored : fallback
  } catch {
    return fallback
  }
}

function getMediaQuery() {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") return undefined
  return window.matchMedia(COLOR_SCHEME_QUERY)
}

/** A cached external snapshot gives SSR a deterministic first render. */
function createThemeStore(storageKey: string, defaultTheme: Theme) {
  const media = getMediaQuery()
  let theme = readTheme(storageKey, defaultTheme)
  const resolve = (value: Theme): ResolvedTheme => value === "system" ? (media?.matches ? "dark" : "light") : value
  let snapshot = `${theme}:${resolve(theme)}`
  const serverSnapshot = `${defaultTheme}:${defaultTheme === "dark" ? "dark" : "light"}`
  const listeners = new Set<() => void>()

  function update(next: Theme) {
    theme = next
    const nextSnapshot = `${theme}:${resolve(theme)}`
    if (nextSnapshot === snapshot) return
    snapshot = nextSnapshot
    listeners.forEach((notify) => notify())
  }

  function onSystemChange() {
    if (theme === "system") update(theme)
  }

  function onStorage(event: StorageEvent) {
    if (event.key !== storageKey && event.key !== null) return
    try {
      if (event.storageArea && event.storageArea !== window.localStorage) return
    } catch {
      return
    }
    update(isTheme(event.newValue) ? event.newValue : defaultTheme)
  }

  return {
    getSnapshot: () => snapshot,
    getServerSnapshot: () => serverSnapshot,
    subscribe(notify: () => void) {
      listeners.add(notify)
      if (listeners.size === 1 && typeof window !== "undefined") {
        window.addEventListener("storage", onStorage)
        media?.addEventListener("change", onSystemChange)
        // Catch storage/system changes between rendering and subscribing.
        update(readTheme(storageKey, theme))
      }
      return () => {
        listeners.delete(notify)
        if (listeners.size === 0 && typeof window !== "undefined") {
          window.removeEventListener("storage", onStorage)
          media?.removeEventListener("change", onSystemChange)
        }
      }
    },
    setTheme(next: Theme) {
      if (!isTheme(next)) return
      try {
        window.localStorage.setItem(storageKey, next)
      } catch {
        // A denied/full store must not prevent an in-memory theme change.
      }
      update(next)
    },
  }
}

function ThemeProvider({ children, defaultTheme = "system", storageKey = THEME_STORAGE_KEY, disableTransitionOnChange = true }: ThemeProviderProps) {
  const store = React.useMemo(() => createThemeStore(storageKey, defaultTheme), [storageKey, defaultTheme])
  const snapshot = React.useSyncExternalStore(store.subscribe, store.getSnapshot, store.getServerSnapshot)
  const [theme, resolvedTheme] = snapshot.split(":") as [Theme, ResolvedTheme]

  React.useEffect(() => {
    const root = document.documentElement
    if (root.classList.contains(resolvedTheme) && !root.classList.contains(resolvedTheme === "dark" ? "light" : "dark")) return

    let style: HTMLStyleElement | undefined
    let firstFrame = 0
    let secondFrame = 0
    if (disableTransitionOnChange) {
      style = document.createElement("style")
      style.textContent = "*,*::before,*::after{transition:none!important}"
      document.head.appendChild(style)
    }

    root.classList.remove("dark", "light")
    root.classList.add(resolvedTheme)

    if (style) {
      // Flush the temporary style, then restore transitions after one paint.
      window.getComputedStyle(root).getPropertyValue("color")
      firstFrame = window.requestAnimationFrame(() => {
        secondFrame = window.requestAnimationFrame(() => style?.remove())
      })
    }

    return () => {
      window.cancelAnimationFrame(firstFrame)
      window.cancelAnimationFrame(secondFrame)
      style?.remove()
    }
  }, [resolvedTheme, disableTransitionOnChange])

  const value = React.useMemo(() => ({ theme, resolvedTheme, setTheme: store.setTheme }), [theme, resolvedTheme, store])

  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>
}

function useTheme(): ThemeState {
  const context = React.useContext(ThemeContext)
  if (!context) throw new Error("useTheme must be used within a ThemeProvider")
  return context
}

const themeOptions = [
  { value: "light", label: "Light", Icon: SunIcon },
  { value: "dark", label: "Dark", Icon: MoonIcon },
  { value: "system", label: "System", Icon: MonitorIcon },
] as const

type ThemeToggleProps = Omit<ButtonProps, "asChild" | "children"> & { compact?: boolean }

function ThemeToggle({ className, compact = false, variant = "ghost", size, ...props }: ThemeToggleProps) {
  const { theme, setTheme } = useTheme()
  const selected = themeOptions.find((option) => option.value === theme) ?? themeOptions[2]
  const Icon = selected.Icon

  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger asChild>
        <Button
          type="button"
          variant={variant}
          size={size ?? (compact ? "icon-sm" : "sm")}
          className={className}
          aria-label={`Theme: ${selected.label}`}
          {...props}
        >
          <Icon aria-hidden="true" />
          {!compact && <span>{selected.label}</span>}
        </Button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content align="end" sideOffset={6} className="z-(--layer-overlay) min-w-36 rounded-md border border-control-border bg-popover p-1 text-xs text-popover-foreground shadow-sm">
          <DropdownMenu.Label className="px-2 py-1.5 font-semibold text-muted-foreground">Color theme</DropdownMenu.Label>
          <DropdownMenu.RadioGroup value={theme} onValueChange={(value) => { if (isTheme(value)) setTheme(value) }}>
            {themeOptions.map(({ value, label, Icon: OptionIcon }) => (
              <DropdownMenu.RadioItem
                key={value}
                value={value}
                className={cn("relative flex cursor-default items-center gap-2 rounded-sm py-1.5 pr-7 pl-2 outline-none select-none data-highlighted:bg-accent data-highlighted:text-accent-foreground", value === theme && "text-primary")}
              >
                <OptionIcon aria-hidden="true" className="size-3.5" />
                {label}
                <DropdownMenu.ItemIndicator className="absolute right-2"><CheckIcon aria-hidden="true" className="size-3.5" /></DropdownMenu.ItemIndicator>
              </DropdownMenu.RadioItem>
            ))}
          </DropdownMenu.RadioGroup>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  )
}

export { ThemeProvider, useTheme, ThemeToggle }
export { getThemeScript, THEME_INIT_SCRIPT, THEME_STORAGE_KEY } from "./theme-script.js"
export type { ThemeScriptOptions } from "./theme-script.js"
export type { Theme, ResolvedTheme, ThemeProviderProps, ThemeState, ThemeToggleProps }
