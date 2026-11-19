import Keycloak from "keycloak-js"

export interface InsightsSession {
  subject: string
  clientId: string
  roles: string[]
  request<T>(path: string, body?: unknown, method?: string): Promise<T>
  logout(): void
}

export interface InsightQuery {
  selection: {
    source: string
    resource: { kind: string; type: string; schemaVersion: number }
    aggregateType?: string
    aggregateId?: string
    ids?: string[]
  }
  fields: string[]
  filters?: { path: string; op: "eq" | "lt" | "gt"; value: unknown }[]
  purpose: string
  requestMissing?: boolean
  requestId?: string
}

export interface InsightResult {
  records: {
    reference: { id: string; version: string }
    fields: Record<string, unknown>
  }[]
  status: string
  requestId?: string
  auditId: string
  policyRevision: number
}

export interface AccessRequest {
  id: string
  identity: { issuer: string; subject: string; clientId: string }
  selection: InsightQuery["selection"]
  fields: string[]
  filterFields: string[]
  purpose: string
  records: { id: string; version: string }[]
  status: string
  expiresAtMs: number
}

let client: Keycloak | undefined
let initialization: Promise<boolean> | undefined

function identityClient(): Keycloak {
  if (client) return client
  const issuer = import.meta.env.VITE_INSIGHTS_ISSUER as string | undefined
  const clientId = import.meta.env.VITE_INSIGHTS_CLIENT_ID as string | undefined
  if (!issuer || !clientId)
    throw new Error(
      "Configure VITE_INSIGHTS_ISSUER and VITE_INSIGHTS_CLIENT_ID."
    )
  const url = new URL(issuer)
  const separator = url.pathname.lastIndexOf("/realms/")
  if (separator < 0 || url.search || url.hash)
    throw new Error("The Insights issuer must identify a Keycloak realm.")
  client = new Keycloak({
    url: `${url.origin}${url.pathname.slice(0, separator)}`,
    realm: decodeURIComponent(
      url.pathname.slice(separator + "/realms/".length)
    ),
    clientId,
  })
  return client
}

export async function insightsSession(): Promise<InsightsSession | undefined> {
  const keycloak = identityClient()
  initialization ??= keycloak.init({
    onLoad: "check-sso",
    pkceMethod: "S256",
    checkLoginIframe: false,
    responseMode: "query",
  })
  if (!(await initialization)) return undefined
  const base = (import.meta.env.VITE_INSIGHTS_API_URL ?? "/api").replace(
    /\/$/,
    ""
  )
  const audience = import.meta.env.VITE_INSIGHTS_AUDIENCE ?? "insights-api"
  return {
    subject: keycloak.subject ?? "Authenticated user",
    clientId: keycloak.clientId ?? "",
    roles: keycloak.resourceAccess?.[audience]?.roles ?? [],
    async request<T>(
      path: string,
      body?: unknown,
      method?: string
    ): Promise<T> {
      await keycloak.updateToken(30)
      if (!keycloak.token) throw new Error("Sign in again to continue.")
      const response = await fetch(`${base}${path}`, {
        method: method ?? (body === undefined ? "GET" : "POST"),
        headers: {
          authorization: `Bearer ${keycloak.token}`,
          ...(body === undefined ? {} : { "content-type": "application/json" }),
        },
        body: body === undefined ? undefined : JSON.stringify(body),
        cache: "no-store",
        redirect: "error",
        credentials: "omit",
        signal: AbortSignal.timeout(20_000),
      })
      if (!response.ok) {
        const error = (await response.json().catch(() => ({}))) as {
          error?: string
        }
        throw new Error(
          error.error ?? `Insights request failed (${response.status}).`
        )
      }
      return (await response.json()) as T
    },
    logout() {
      keycloak.clearToken()
      void keycloak.logout({
        redirectUri: window.location.origin + window.location.pathname,
      })
    },
  }
}

export function signInToInsights(): Promise<void> {
  return identityClient().login()
}
