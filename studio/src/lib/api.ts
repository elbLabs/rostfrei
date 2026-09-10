import type {
  AggregateInstance,
  CommandExecutionResult,
  CommandInputDocument,
  Fixture,
  OperationMessageSeries,
  OperationSnapshot,
  TestDefinitionRevision,
  TestDefinitionSummary,
  TestReport,
  TracerCatalog,
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

interface AggregateInstanceCollection {
  items: AggregateInstance[]
}

export async function getCatalog(): Promise<TracerCatalog> {
  const catalog = await requestJson<TracerCatalog>("/catalog", {
    signal: AbortSignal.timeout(12_000),
  })
  if (catalog.catalogVersion !== 1) {
    throw new Error(
      `Unsupported Tracer catalog version ${catalog.catalogVersion}`
    )
  }
  return catalog
}

export async function listAggregateInstances(
  instancesHref: string
): Promise<AggregateInstance[]> {
  const collection = await requestJson<AggregateInstanceCollection>(
    advertisedHref(instancesHref),
    { signal: AbortSignal.timeout(12_000) }
  )
  return collection.items
}

export function getCommandInputs(
  inputsHrefTemplate: string
): Promise<CommandInputDocument> {
  return requestCommandInputs(advertisedHref(inputsHrefTemplate))
}

async function requestCommandInputs(
  path: string
): Promise<CommandInputDocument> {
  const response = await request(path, {
    signal: AbortSignal.timeout(12_000),
  })
  const source = await response.text()
  const document = JSON.parse(source) as CommandInputDocument
  const root = parseJsonNode(source)
  const fieldNodes = root.properties?.get("fields")?.items ?? []
  document.fields.forEach((field, fieldIndex) => {
    const optionNodes =
      fieldNodes[fieldIndex]?.properties?.get("options")?.items ?? []
    field.options.forEach((option, optionIndex) => {
      const valueNode = optionNodes[optionIndex]?.properties?.get("value")
      if (valueNode)
        option.valueJson = source.slice(valueNode.start, valueNode.end)
    })
  })
  return document
}

export async function listTests(
  definitionsHref: string
): Promise<TestDefinitionSummary[]> {
  const collection = await requestJson<TestCollection>(
    advertisedHref(definitionsHref)
  )
  return collection.items
}

export function getTest(
  definitionHref: string
): Promise<TestDefinitionRevision> {
  return requestJson(advertisedHref(definitionHref))
}

export function getFixture(fixtureId: string): Promise<Fixture> {
  return requestJson(`/test-scenario/fixtures/${encodeURIComponent(fixtureId)}`)
}

export function runTest(runHref: string): Promise<TestReport> {
  return requestJson(advertisedHref(runHref), { method: "POST" })
}

export class CommandSubmissionIndeterminateError extends Error {
  override name = "CommandSubmissionIndeterminateError"
}

export async function submitCommand(
  mode: "preview" | "test",
  hrefTemplate: string,
  schemaVersion: number,
  payloadJson: string,
  idempotencyKey?: string
): Promise<CommandExecutionResult> {
  if (mode === "test" && !idempotencyKey) {
    throw new Error("Test publication requires an idempotency key")
  }
  const submitHref = advertisedHref(hrefTemplate)
  let response: Response
  try {
    response = await request(submitHref, {
      method: "POST",
      body: `{"schemaVersion":${schemaVersion},"payload":${payloadJson}}`,
      headers: {
        "content-type": "application/json",
        ...(idempotencyKey ? { "idempotency-key": idempotencyKey } : {}),
      },
      signal: AbortSignal.timeout(12_000),
    })
  } catch (error) {
    const definitelyRejected =
      error instanceof TracerResponseError &&
      error.status >= 400 &&
      error.status < 500 &&
      error.status !== 408
    if (mode === "test" && !definitelyRejected) {
      throw indeterminateSubmissionError()
    }
    throw error
  }

  let operation: OperationSnapshot
  let operationHref: string
  try {
    operation = (await response.json()) as OperationSnapshot
    const location = response.headers.get("location")
    if (!location) throw new Error("Missing operation Location")
    operationHref = advertisedHref(location)
    const expectedMode = mode === "preview" ? "simulate" : "test"
    if (operation.mode !== expectedMode) {
      throw new Error("Unexpected operation mode")
    }
  } catch (error) {
    if (mode === "test") throw indeterminateSubmissionError()
    throw error
  }

  const deadline = Date.now() + 10_000
  let latest = operation
  while (!isTerminal(latest.status) && Date.now() < deadline) {
    await delay(Math.min(200, Math.max(0, deadline - Date.now())))
    const remaining = deadline - Date.now()
    if (remaining <= 0) break
    try {
      latest = await requestJson<OperationSnapshot>(operationHref, {
        signal: AbortSignal.timeout(remaining),
      })
    } catch (error) {
      return { operation: latest, inspectionError: errorMessage(error) }
    }
  }

  if (!isTerminal(latest.status)) return { operation: latest }

  try {
    const messageSeriesHref = advertisedHref(latest.messageSeriesHref)
    const query = messageSeriesHref.includes("?") ? "&" : "?"
    const series = await requestJson<OperationMessageSeries>(
      `${messageSeriesHref}${query}within=10s&settleFor=500ms`,
      { signal: AbortSignal.timeout(12_000) }
    )
    return { operation: latest, series }
  } catch (error) {
    return { operation: latest, inspectionError: errorMessage(error) }
  }
}

async function requestJson<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await request(path, init)
  return (await response.json()) as T
}

async function request(path: string, init?: RequestInit): Promise<Response> {
  const response = await fetch(apiUrl(path), {
    ...init,
    redirect: "error",
    headers: {
      ...requestHeaders("application/json"),
      ...init?.headers,
    },
  })
  if (!response.ok) throw await responseError(response)
  return response
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

function advertisedHref(href: string): string {
  if (
    !href.startsWith("/") ||
    href.startsWith("//") ||
    href.includes("\\") ||
    href.includes("#") ||
    Array.from(href).some((character) => {
      const code = character.charCodeAt(0)
      return code <= 31 || code === 127
    }) ||
    /[{}]/.test(href)
  ) {
    throw new Error("Tracer advertised an unsafe link")
  }
  return href
}

function isTerminal(status: OperationSnapshot["status"]): boolean {
  return (
    status === "completed" || status === "failed" || status === "indeterminate"
  )
}

function delay(milliseconds: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, milliseconds))
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : "Operation inspection failed"
}

interface JsonNode {
  start: number
  end: number
  properties?: Map<string, JsonNode>
  items?: JsonNode[]
}

function parseJsonNode(source: string): JsonNode {
  let cursor = 0

  const whitespace = () => {
    while (/\s/.test(source[cursor] ?? "")) cursor += 1
  }
  const parse = (): JsonNode => {
    whitespace()
    const start = cursor
    if (source[cursor] === '"') {
      cursor += 1
      while (cursor < source.length) {
        if (source[cursor] === "\\") {
          cursor += 2
        } else if (source[cursor] === '"') {
          cursor += 1
          break
        } else {
          cursor += 1
        }
      }
      return { start, end: cursor }
    }
    if (source[cursor] === "[") {
      cursor += 1
      const items: JsonNode[] = []
      whitespace()
      while (source[cursor] !== "]") {
        items.push(parse())
        whitespace()
        if (source[cursor] === ",") cursor += 1
        whitespace()
      }
      cursor += 1
      return { start, end: cursor, items }
    }
    if (source[cursor] === "{") {
      cursor += 1
      const properties = new Map<string, JsonNode>()
      whitespace()
      while (source[cursor] !== "}") {
        const key = parse()
        const name = JSON.parse(source.slice(key.start, key.end)) as string
        whitespace()
        cursor += 1
        const value = parse()
        properties.set(name, value)
        whitespace()
        if (source[cursor] === ",") cursor += 1
        whitespace()
      }
      cursor += 1
      return { start, end: cursor, properties }
    }
    while (cursor < source.length && !/[\s,\]}]/.test(source[cursor])) {
      cursor += 1
    }
    return { start, end: cursor }
  }

  return parse()
}

function indeterminateSubmissionError(): CommandSubmissionIndeterminateError {
  return new CommandSubmissionIndeterminateError(
    "Tracer did not confirm the Test publication. It may have reached the bus; retry only with the same idempotency key."
  )
}

class TracerResponseError extends Error {
  readonly status: number

  constructor(message: string, status: number) {
    super(message)
    this.status = status
  }

  override name = "TracerResponseError"
}

async function responseError(response: Response): Promise<TracerResponseError> {
  const fallback = `${response.status} ${response.statusText}`
  try {
    const body = (await response.json()) as { message?: string; code?: string }
    return new TracerResponseError(
      body.message ?? body.code ?? fallback,
      response.status
    )
  } catch {
    return new TracerResponseError(fallback, response.status)
  }
}
