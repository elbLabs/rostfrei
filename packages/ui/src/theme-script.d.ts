export type ThemeScriptOptions = {
  defaultTheme?: "light" | "dark" | "system"
  storageKey?: string
}

export declare const THEME_STORAGE_KEY: "rostfrei-theme"
export declare const COLOR_SCHEME_QUERY: "(prefers-color-scheme: dark)"
export declare function getThemeScript(options?: ThemeScriptOptions): string
export declare const THEME_INIT_SCRIPT: string
