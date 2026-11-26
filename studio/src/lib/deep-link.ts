import { workspaceModes, type Workspace } from "@/lib/workspace"

// Studio has no router. A link is a URL fragment, so it can be opened in a new
// tab, shared or framed without changing what the server serves:
// `#correlation=<ID>` opens that correlation, `&embed=1` shows it without
// Studio's navigation.
export interface StudioLink {
  correlationId?: string
  embed: boolean
}

export function correlationHref(
  correlationId: string,
  { embed = false }: { embed?: boolean } = {}
): string {
  return `#correlation=${encodeURIComponent(correlationId)}${embed ? "&embed=1" : ""}`
}

export function studioLink(hash: string = window.location.hash): StudioLink {
  const fields = new Map<string, string>()
  for (const field of hash.replace(/^#/, "").split("&")) {
    const separator = field.indexOf("=")
    if (separator < 1) continue
    try {
      fields.set(
        field.slice(0, separator),
        decodeURIComponent(field.slice(separator + 1))
      )
    } catch {
      continue
    }
  }
  return {
    correlationId: fields.get("correlation") || undefined,
    embed: fields.get("embed") === "1",
  }
}

// What a history entry showed. Following a link adds an entry, so Back returns
// to the tab and correlation recorded on the entry before it.
export interface StudioView {
  workspace: Workspace
  correlationId?: string
}

const VIEW = "rostfreiStudioView"

export function recordedView(): StudioView | undefined {
  const state: unknown = window.history.state
  if (typeof state !== "object" || state === null) return undefined
  const view: unknown = Reflect.get(state, VIEW)
  if (typeof view !== "object" || view === null) return undefined
  const correlationId: unknown = Reflect.get(view, "correlationId")
  const workspace = workspaceModes.find(
    (mode) => mode.id === Reflect.get(view, "workspace")
  )?.id
  if (!workspace) return undefined
  return {
    workspace,
    correlationId:
      typeof correlationId === "string" ? correlationId : undefined,
  }
}

// A correlation link belongs to Correlations; it is removed from the address
// once the entry shows another tab.
export function recordView(view: StudioView): void {
  const { pathname, search, hash } = window.location
  const stale =
    view.workspace !== "correlations" &&
    studioLink().correlationId !== undefined
  window.history.replaceState(
    { ...(window.history.state as object | null), [VIEW]: view },
    "",
    pathname + search + (stale ? "" : hash)
  )
}
