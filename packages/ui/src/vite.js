import { getThemeScript } from "./theme-script.js"

/**
 * Inject the shared theme before CSS and module execution in dev and builds.
 * @param {import("./theme-script.js").ThemeScriptOptions} [options]
 * @returns {import("./vite.js").ReferenceThemePlugin}
 */
export function referenceTheme(options = {}) {
  const script = getThemeScript(options)

  return {
    name: "rostfrei:reference-theme",
    transformIndexHtml: {
      order: "pre",
      handler() {
        return [{ tag: "script", children: script, injectTo: "head-prepend" }]
      },
    },
  }
}
