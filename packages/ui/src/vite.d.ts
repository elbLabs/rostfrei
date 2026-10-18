import type { ThemeScriptOptions } from "./theme-script.js"

/** Structurally compatible with Vite's Plugin type; no Vite runtime is loaded. */
export type ReferenceThemePlugin = {
  name: string
  transformIndexHtml: {
    order: "pre"
    handler: () => {
      tag: "script"
      children: string
      injectTo: "head-prepend"
    }[]
  }
}

export declare function referenceTheme(
  options?: ThemeScriptOptions
): ReferenceThemePlugin

export type { ThemeScriptOptions }
