export const THEME_STORAGE_KEY = "rostfrei-theme"
export const COLOR_SCHEME_QUERY = "(prefers-color-scheme: dark)"

/**
 * Generate the synchronous, pre-paint theme script without loading React.
 * @param {import("./theme-script.js").ThemeScriptOptions} [options]
 */
export function getThemeScript({
  storageKey = THEME_STORAGE_KEY,
  defaultTheme = "system",
} = {}) {
  const key = JSON.stringify(storageKey).replace(/</g, "\\u003c")
  const fallback = JSON.stringify(defaultTheme).replace(/</g, "\\u003c")
  return `(function(){var t=${fallback};try{var v=localStorage.getItem(${key});if(v==='light'||v==='dark'||v==='system')t=v}catch(e){}var d=t==='dark'||(t==='system'&&typeof matchMedia==='function'&&matchMedia('${COLOR_SCHEME_QUERY}').matches);var r=document.documentElement;r.classList.remove('light','dark');r.classList.add(d?'dark':'light')})()`
}

export const THEME_INIT_SCRIPT = getThemeScript()
