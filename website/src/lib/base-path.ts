/** Router locations can include the configured deployment prefix. */
export function withoutBasePath(pathname: string) {
  const base = import.meta.env.BASE_URL.replace(/\/+$/, "")
  const path =
    base && (pathname === base || pathname.startsWith(`${base}/`))
      ? pathname.slice(base.length)
      : pathname
  return path.replace(/\/+$/, "") || "/"
}
