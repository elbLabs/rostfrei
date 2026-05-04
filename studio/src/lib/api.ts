import type {
  CorrelationEvent,
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

export function runTest(runHref: string): Promise<TestReport> {
  return requestJson(runHref, { method: "POST" })
}

export async function collectCorrelation(
  correlationId: string,
  onEvents?: (events: CorrelationEvent[]) => void | Promise<void>
): Promise<CorrelationEvent[]> {
  const controller = new AbortController()
  const response = await fetch(
    apiUrl(`/correlations/${encodeURIComponent(correlationId)}/events`),
    {
      headers: requestHeaders("text/event-stream"),
      signal: controller.signal,
    }
  )
  if (!response.ok) throw await responseError(response)
  if (!response.body) return []

  const events: CorrelationEvent[] = []
  const reader = response.body.getReader()
  const decoder = new TextDecoder()
  let buffer = ""
  let settledTimer: number | undefined
  const maximumTimer = window.setTimeout(() => controller.abort(), 12000)

  try {
    while (true) {
      const { done, value } = await reader.read()
      if (done) break
      buffer += decoder.decode(value, { stream: true }).replace(/\r\n/g, "\n")
      let boundary = buffer.indexOf("\n\n")
      while (boundary >= 0) {
        const frame = buffer.slice(0, boundary)
        buffer = buffer.slice(boundary + 2)
        const event = parseSseFrame(frame)
        if (event) {
          events.push(event)
          await onEvents?.([...events])
          if (settledTimer !== undefined) window.clearTimeout(settledTimer)
          if (events.some((candidate) => candidate.type === "command-result")) {
            settledTimer = window.setTimeout(() => controller.abort(), 500)
          }
        }
        boundary = buffer.indexOf("\n\n")
      }
    }
  } catch (error) {
    if (!(error instanceof DOMException && error.name === "AbortError"))
      throw error
  } finally {
    window.clearTimeout(maximumTimer)
    if (settledTimer !== undefined) window.clearTimeout(settledTimer)
    reader.releaseLock()
  }

  return events
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

function parseSseFrame(frame: string): CorrelationEvent | undefined {
  const data = frame
    .split("\n")
    .filter((line) => line.startsWith("data:"))
    .map((line) => line.slice(5).trimStart())
    .join("\n")
  if (!data) return undefined

  try {
    return JSON.parse(data) as CorrelationEvent
  } catch {
    return undefined
  }
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
