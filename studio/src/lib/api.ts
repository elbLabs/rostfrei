import type {
  Fixture,
  TestDefinitionRevision,
  TestDefinitionSummary,
  TestReport,
} from "@/lib/types"

const API_BASE = (import.meta.env.VITE_TRACER_API_URL ?? "/api").replace(
  /\/$/,
  ""
)
const CONTROL_TOKEN =
  import.meta.env.VITE_TRACER_TOKEN ?? "local-development-token"

interface TestCollection {
  items: TestDefinitionSummary[]
}

export async function listTests(): Promise<TestDefinitionSummary[]> {
  const collection = await requestJson<TestCollection>("/tests")
  return collection.items
}

export function getTest(testId: string): Promise<TestDefinitionRevision> {
  return requestJson(`/tests/${encodeURIComponent(testId)}`)
}

export function getFixture(fixtureId: string): Promise<Fixture> {
  return requestJson(
    `/test-scenario/fixtures/${encodeURIComponent(fixtureId)}`
  )
}

export function runTest(runHref: string): Promise<TestReport> {
  return requestJson(runHref, { method: "POST" })
}

async function requestJson<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(apiUrl(path), {
    ...init,
    headers: {
      ...requestHeaders("application/json"),
      ...init?.headers,
    },
  })
  if (!response.ok) throw await responseError(response)
  return (await response.json()) as T
}

function requestHeaders(accept: string): HeadersInit {
  return {
    accept,
    authorization: `Bearer ${CONTROL_TOKEN}`,
  }
}

function apiUrl(path: string): string {
  if (/^https?:\/\//.test(path)) return path
  return `${API_BASE}${path.startsWith("/") ? path : `/${path}`}`
}


async function responseError(response: Response): Promise<Error> {
  const fallback = `${response.status} ${response.statusText}`
  try {
    const body = (await response.json()) as { message?: string; code?: string }
    return new Error(body.message ?? body.code ?? fallback)
  } catch {
    return new Error(fallback)
  }
}
