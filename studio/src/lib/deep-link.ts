// Studio has no router. A correlation link is a URL fragment, so it can be
// opened in a new tab or shared without changing what the server serves.
const CORRELATION = "#correlation="

export function correlationHref(correlationId: string): string {
  return `${CORRELATION}${encodeURIComponent(correlationId)}`
}

export function linkedCorrelation(
  hash: string = window.location.hash
): string | undefined {
  if (!hash.startsWith(CORRELATION)) return undefined
  try {
    return decodeURIComponent(hash.slice(CORRELATION.length)) || undefined
  } catch {
    return undefined
  }
}

export function clearLinkedCorrelation(): void {
  if (linkedCorrelation() === undefined) return
  window.history.replaceState(
    null,
    "",
    window.location.pathname + window.location.search
  )
}
