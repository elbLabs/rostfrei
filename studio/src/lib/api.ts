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

export async function getCatalog(signal?: AbortSignal): Promise<TracerCatalog> {
  const catalog = await requestJson<TracerCatalog>("/catalog", {
    signal,
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
  definitionsHref: string,
  signal?: AbortSignal
): Promise<TestDefinitionSummary[]> {
  const collection = await requestJson<TestCollection>(
    advertisedHref(definitionsHref),
    { signal }
  )
  if (!collection || !Array.isArray(collection.items)) {
    throw new Error(
      `${requestLabel(definitionsHref)}: Tracer did not return a test list.`
    )
  }
  return collection.items
}

export function getTest(
  definitionHref: string,
  signal?: AbortSignal
): Promise<TestDefinitionRevision> {
  return requestJson(advertisedHref(definitionHref), { signal })
}

export function getFixture(
  fixtureId: string,
  signal?: AbortSignal
): Promise<Fixture> {
  return requestJson(
    `/test-scenario/fixtures/${encodeURIComponent(fixtureId)}`,
    { signal }
  )
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
  try {
    return (await response.json()) as T
  } catch {
    throw new Error(
      `${requestLabel(path, init?.method)}: Tracer returned an invalid or incomplete JSON response.`
    )
  }
}

async function request(path: string, init?: RequestInit): Promise<Response> {
  const label = requestLabel(path, init?.method)
  const timeout = AbortSignal.timeout(init?.method === "POST" ? 60_000 : 10_000)
  const signal = init?.signal
    ? AbortSignal.any([init.signal, timeout])
    : timeout
  let response: Response
  try {
    response = await fetch(apiUrl(path), {
      ...init,
      signal,
      redirect: "error",
      headers: { ...requestHeaders("application/json"), ...init?.headers },
    })
  } catch {
    throw new Error(
      `${label}: ${timeout.aborted ? "Tracer did not respond before the request timed out." : signal.aborted ? "Request cancelled or timed out." : "Could not reach the Tracer API. Check the server, proxy target, and network connection."}`
    )
  }
  if (!response.ok) {
    const error = await responseError(response)
    error.message = `${label}: ${error.message}`
    throw error
  }
  return response
}

function requestLabel(path: string, method = "GET"): string {
  // Exclude URL credentials, query strings, and authorization headers.
  return `${method} ${new URL(apiUrl(path), window.location.origin).pathname}`
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
  if (response.status === 401)
    return new TracerResponseError(
      `${fallback}: Authentication failed. Check VITE_TRACER_TOKEN.`,
      response.status
    )
  if (response.status === 403)
    return new TracerResponseError(
      `${fallback}: The configured Tracer token does not have permission for this request.`,
      response.status
    )
  if (response.status === 502 || response.status === 504)
    return new TracerResponseError(
      `${fallback}: The Studio proxy could not reach Tracer. Check VITE_TRACER_TARGET and that Tracer is running.`,
      response.status
    )
  try {
    const body = (await response.json()) as { message?: string; code?: string }
    return new TracerResponseError(
      body.message || body.code
        ? `${fallback}: ${body.message ?? body.code}`
        : fallback,
      response.status
    )
  } catch {
    return new TracerResponseError(fallback, response.status)
  }
}
