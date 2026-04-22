export type OperationStatus = "queued" | "running" | "completed" | "failed" | "indeterminate";
export type OperationMode = "simulate" | "test" | "dispatch";

export type PredictedDomainEvent = {
  ordinal: number;
  predictedStreamVersion: number;
  eventType: string;
  schemaVersion: number;
  payload?: unknown;
};

export type OperationResult = {
  decision: "accepted";
  baseStreamVersion?: number;
  predictedEvents: PredictedDomainEvent[];
  appended?: boolean;
  published: boolean;
  commandMessageId?: string;
  responseMessageId?: string;
  duplicate?: boolean;
} | {
  decision: "rejected";
  baseStreamVersion?: number;
  rejection?: unknown;
  appended?: boolean;
  published: boolean;
  commandMessageId?: string;
  responseMessageId?: string;
  duplicate?: boolean;
};

export type OperationFailure = {
  code: string;
  message: string;
  commandMessageId?: string;
  duplicate?: boolean;
};

export type OperationSnapshot = {
  operationId: string;
  correlationId: string;
  mode: OperationMode;
  status: OperationStatus;
  command: string;
  schemaVersion: number;
  aggregateType: string;
  aggregateId: string;
  latestEventId: number;
  result?: OperationResult;
  failure?: OperationFailure;
};

export type OperationInput = {
  hrefTemplate: string;
  aggregateId: string;
  schemaVersion: number;
  payload: unknown;
};

type CorrelationEventBase = {
  id: number;
  correlationId: string;
};

export type CorrelationCommandEvent = CorrelationEventBase & {
  type: "command";
  operationId: string;
  command: string;
  schemaVersion: number;
  aggregateType: string;
  aggregateId: string;
};

export type CorrelationDomainEvent = CorrelationEventBase & {
  type: "domain-event";
  eventType: string;
  schemaVersion: number;
  streamVersion?: number;
  payload?: unknown;
};

export type CorrelationIntegrationEvent = CorrelationEventBase & {
  type: "integration-event";
  eventType: string;
  schemaVersion: number;
  messageId?: string;
  subject?: string;
  payload?: unknown;
};

export type CorrelationCommandResultEvent = CorrelationEventBase & {
  type: "command-result";
  operationId: string;
  outcome: "accepted" | "rejected" | "failed" | "indeterminate";
  result?: unknown;
};

export type CorrelationEvent =
  | CorrelationCommandEvent
  | CorrelationDomainEvent
  | CorrelationIntegrationEvent
  | CorrelationCommandResultEvent;

export type CatalogCommandVersion = {
  schemaVersion: number;
  contentType: string;
  fields: CatalogField[];
  payloadTemplate: unknown;
  inputsHrefTemplate: string;
  simulateHrefTemplate: string;
  testHrefTemplate?: string;
  dispatchHrefTemplate?: string;
};

export type CatalogField = {
  name: string;
  value: {
    kind: string;
    scalar?: string | {
      representation?: string;
    };
  };
};

export type CatalogCommand = {
  id: string;
  label: string;
  versions: CatalogCommandVersion[];
};

export type CatalogAggregate = {
  id: string;
  label: string;
  aggregateType: string;
  instancesHref: string;
  commands: CatalogCommand[];
};

export type CatalogContext = {
  id: string;
  label: string;
  aggregates: CatalogAggregate[];
};

export type TracerCatalog = {
  catalogVersion: number;
  contexts: CatalogContext[];
  testScenario?: {
    resetHref: string;
  };
};

export type AggregateInstance = {
  aggregateId: string;
  streamVersion: number;
};

export type AggregateInstanceCollection = {
  items: AggregateInstance[];
};

export type CommandInputOption = {
  value: unknown;
  label: string;
  description?: string;
};

export type CommandInputField = {
  name: string;
  label: string;
  options: CommandInputOption[];
};

export type CommandInputDocument = {
  fields: CommandInputField[];
};

type SseFrame = {
  id: string;
  name: string;
  data: string;
};

class SseProtocolError extends Error {}

function endpoint(baseUrl: string, path: string) {
  return `${baseUrl.trim().replace(/\/$/, "")}${path}`;
}

function authorizationHeaders(bearerToken: string): Record<string, string> {
  return bearerToken ? { authorization: `Bearer ${bearerToken}` } : {};
}

async function errorMessage(response: Response) {
  try {
    const body = (await response.json()) as { message?: string };
    return body.message || `${response.status} ${response.statusText}`;
  } catch {
    return `${response.status} ${response.statusText}`;
  }
}

function parseFrames(buffer: string, flush = false) {
  const normalized = buffer.replaceAll("\r\n", "\n");
  const chunks = normalized.split("\n\n");
  const remainder = flush ? "" : (chunks.pop() ?? "");
  const frames: SseFrame[] = [];

  for (const chunk of chunks) {
    if (!chunk || chunk.startsWith(":")) continue;
    let id = "";
    let name = "message";
    const data: string[] = [];

    for (const line of chunk.split("\n")) {
      if (line.startsWith("id:")) id = line.slice(3).trimStart();
      if (line.startsWith("event:")) name = line.slice(6).trimStart();
      if (line.startsWith("data:")) data.push(line.slice(5).trimStart());
    }

    if (data.length) frames.push({ id, name, data: data.join("\n") });
  }

  return { frames, remainder };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function isUnsignedInteger(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function optionalString(value: Record<string, unknown>, key: string) {
  return value[key] === undefined || typeof value[key] === "string";
}

function parseCorrelationEvent(frame: SseFrame, correlationId: string): CorrelationEvent {
  let value: unknown;
  try {
    value = JSON.parse(frame.data) as unknown;
  } catch {
    throw new SseProtocolError("The correlation stream returned invalid JSON");
  }

  if (
    !isRecord(value) ||
    !isUnsignedInteger(value.id) ||
    value.correlationId !== correlationId ||
    value.type !== frame.name ||
    frame.id !== String(value.id)
  ) {
    throw new SseProtocolError("The correlation stream returned an invalid event envelope");
  }

  if (value.type === "command") {
    if (
      typeof value.operationId !== "string" ||
      typeof value.command !== "string" ||
      !isUnsignedInteger(value.schemaVersion) ||
      typeof value.aggregateType !== "string" ||
      typeof value.aggregateId !== "string"
    ) {
      throw new SseProtocolError("The correlation stream returned an invalid command event");
    }
    return value as CorrelationCommandEvent;
  }

  if (value.type === "domain-event") {
    if (
      typeof value.eventType !== "string" ||
      !isUnsignedInteger(value.schemaVersion) ||
      (value.streamVersion !== undefined && !isUnsignedInteger(value.streamVersion))
    ) {
      throw new SseProtocolError("The correlation stream returned an invalid domain event");
    }
    return value as CorrelationDomainEvent;
  }

  if (value.type === "integration-event") {
    if (
      typeof value.eventType !== "string" ||
      !isUnsignedInteger(value.schemaVersion) ||
      !optionalString(value, "messageId") ||
      !optionalString(value, "subject")
    ) {
      throw new SseProtocolError("The correlation stream returned an invalid integration event");
    }
    return value as CorrelationIntegrationEvent;
  }

  if (value.type === "command-result") {
    if (
      typeof value.operationId !== "string" ||
      (value.outcome !== "accepted" &&
        value.outcome !== "rejected" &&
        value.outcome !== "failed" &&
        value.outcome !== "indeterminate")
    ) {
      throw new SseProtocolError("The correlation stream returned an invalid command result");
    }
    return value as CorrelationCommandResultEvent;
  }

  throw new SseProtocolError(`The correlation stream returned unknown event ${frame.name}`);
}

function waitForRetry(delayMs: number, signal: AbortSignal) {
  return new Promise<void>((resolve) => {
    if (signal.aborted) {
      resolve();
      return;
    }
    const stopWaiting = () => {
      window.clearTimeout(timeout);
      resolve();
    };
    const timeout = window.setTimeout(() => {
      signal.removeEventListener("abort", stopWaiting);
      resolve();
    }, delayMs);
    signal.addEventListener("abort", stopWaiting, { once: true });
  });
}

export async function fetchCatalog(
  baseUrl: string,
  bearerToken: string,
  signal: AbortSignal,
) {
  const response = await fetch(endpoint(baseUrl, "/catalog"), {
    signal,
    headers: authorizationHeaders(bearerToken),
  });
  if (!response.ok) throw new Error(await errorMessage(response));
  return (await response.json()) as TracerCatalog;
}

export async function fetchAggregateInstances(
  baseUrl: string,
  bearerToken: string,
  href: string,
  signal: AbortSignal,
) {
  const response = await fetch(endpoint(baseUrl, href), {
    signal,
    headers: authorizationHeaders(bearerToken),
  });
  if (!response.ok) throw new Error(await errorMessage(response));
  return (await response.json()) as AggregateInstanceCollection;
}

export async function fetchCommandInputs(
  baseUrl: string,
  bearerToken: string,
  hrefTemplate: string,
  aggregateId: string,
  signal: AbortSignal,
) {
  const path = hrefTemplate.replace("{aggregateId}", encodeURIComponent(aggregateId));
  if (path === hrefTemplate) {
    throw new Error("The Tracer catalog returned an invalid command-input link");
  }
  const response = await fetch(endpoint(baseUrl, path), {
    signal,
    headers: authorizationHeaders(bearerToken),
  });
  if (!response.ok) throw new Error(await errorMessage(response));
  return (await response.json()) as CommandInputDocument;
}

export async function submitOperation(
  baseUrl: string,
  bearerToken: string,
  mode: OperationMode,
  input: OperationInput,
  signal: AbortSignal,
) {
  const operationId = `studio-${mode}-${crypto.randomUUID()}`;
  const path = input.hrefTemplate.replace(
    "{aggregateId}",
    encodeURIComponent(input.aggregateId),
  );
  if (path === input.hrefTemplate) {
    throw new Error(`The Tracer catalog returned an invalid ${mode} link`);
  }
  const response = await fetch(endpoint(baseUrl, path), {
    method: "POST",
    signal,
    headers: {
      ...authorizationHeaders(bearerToken),
      "content-type": "application/json",
      "idempotency-key": operationId,
    },
    body: JSON.stringify({
      schemaVersion: input.schemaVersion,
      payload: input.payload,
    }),
  });

  if (!response.ok) throw new Error(await errorMessage(response));
  return (await response.json()) as OperationSnapshot;
}

export async function fetchOperation(
  baseUrl: string,
  bearerToken: string,
  operationId: string,
  signal: AbortSignal,
) {
  const response = await fetch(
    endpoint(baseUrl, `/operations/${encodeURIComponent(operationId)}`),
    {
      signal,
      cache: "no-store",
      headers: authorizationHeaders(bearerToken),
    },
  );
  if (!response.ok) throw new Error(await errorMessage(response));
  return (await response.json()) as OperationSnapshot;
}

export async function waitForOperation(
  baseUrl: string,
  bearerToken: string,
  initial: OperationSnapshot,
  signal: AbortSignal,
  onChange: (operation: OperationSnapshot) => void,
) {
  let operation = initial;
  onChange(operation);

  while (
    operation.status !== "completed"
    && operation.status !== "failed"
    && operation.status !== "indeterminate"
  ) {
    await new Promise<void>((resolve) => window.setTimeout(resolve, 200));
    if (signal.aborted) throw new DOMException("The operation was cancelled", "AbortError");
    operation = await fetchOperation(
      baseUrl,
      bearerToken,
      operation.operationId,
      signal,
    );
    onChange(operation);
  }

  return operation;
}

export async function streamCorrelationEvents(
  baseUrl: string,
  bearerToken: string,
  correlationId: string,
  signal: AbortSignal,
  onEvent: (event: CorrelationEvent) => void,
) {
  let cursor = 0;
  let retryDelay = 300;
  const delivered = new Set<string>();

  while (!signal.aborted) {
    const headers: Record<string, string> = {
      accept: "text/event-stream",
      ...authorizationHeaders(bearerToken),
    };
    if (cursor) headers["last-event-id"] = String(cursor);

    try {
      const response = await fetch(
        endpoint(baseUrl, `/correlations/${encodeURIComponent(correlationId)}/events`),
        { headers, signal },
      );
      if (!response.ok) {
        const message = await errorMessage(response);
        if (response.status < 500 && response.status !== 408 && response.status !== 429) {
          throw new SseProtocolError(message);
        }
        throw new Error(message);
      }
      if (!response.body) {
        throw new SseProtocolError("The correlation-event stream did not provide a response body");
      }

      const reader = response.body.getReader();
      const decoder = new TextDecoder();
      let buffer = "";
      const consume = (flush = false) => {
        const parsed = parseFrames(buffer, flush);
        buffer = parsed.remainder;
        for (const frame of parsed.frames) {
          const event = parseCorrelationEvent(frame, correlationId);
          cursor = event.id;
          retryDelay = 300;
          const eventKey = `${event.correlationId}:${event.id}`;
          if (delivered.has(eventKey)) continue;
          delivered.add(eventKey);
          onEvent(event);
        }
      };

      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        buffer += decoder.decode(value, { stream: true });
        consume();
      }
      buffer += decoder.decode();
      consume(true);
    } catch (caught) {
      if (signal.aborted) return;
      if (caught instanceof SseProtocolError) throw caught;
    }

    await waitForRetry(retryDelay, signal);
    retryDelay = Math.min(retryDelay * 2, 5_000);
  }
}

export async function resetTestScenario(
  baseUrl: string,
  bearerToken: string,
  href: string,
  signal: AbortSignal,
) {
  const response = await fetch(endpoint(baseUrl, href), {
    method: "POST",
    signal,
    headers: authorizationHeaders(bearerToken),
  });
  if (!response.ok) throw new Error(await errorMessage(response));
}
