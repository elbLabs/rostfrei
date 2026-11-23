import {
  advertisedHref,
  request,
  requestJson,
  TracerResponseError,
  tracerUrl,
  type TracerCapability,
} from "@/lib/api"
import type { OperationSnapshot, TracerCatalog } from "@/lib/types"

export type OperatorMode = "test" | "dispatch"

export interface OperatorCommand {
  context: string
  command: string
  schemaVersion: number
  shape: unknown
  href: string
  testInputsHref?: string
}

export interface OperatorAttempt extends OperatorCommand {
  mode: OperatorMode
  payload: string
  key: string
}

export interface OperatorReceipt {
  href: string
  operation: OperationSnapshot
}

export function operatorChoices(catalog: TracerCatalog, mode: OperatorMode) {
  return catalog.contexts.flatMap((context) =>
    context.commands.flatMap((command) =>
      command.versions.flatMap((version): OperatorCommand[] => {
        const href =
          mode === "test"
            ? version.testHrefTemplate
            : version.dispatchHrefTemplate
        if (!href) return []
        try {
          tracerUrl(href)
        } catch {
          return []
        }
        return [
          {
            context: context.id,
            command: command.id,
            schemaVersion: version.schemaVersion,
            shape: version.payloadTemplate,
            href,
            testInputsHref:
              mode === "test" ? version.testInputsHrefTemplate : undefined,
          },
        ]
      })
    )
  )
}

export async function publishOperatorCommand(
  attempt: OperatorAttempt,
  capability: TracerCapability,
  signal: AbortSignal
): Promise<OperatorReceipt> {
  requireMode(attempt.mode, capability)
  const response = await request(
    advertisedHref(attempt.href),
    {
      method: "POST",
      signal,
      timeoutMs: 12_000,
      cache: "no-store",
      headers: {
        "content-type": "application/json",
        "idempotency-key": attempt.key,
      },
      // Preserve original JSON, including integers larger than JS safe integers.
      body: `{"schemaVersion":${attempt.schemaVersion},"payload":${attempt.payload}}`,
    },
    capability
  )
  // CommandSubmissionResponse flattens its operation and adds the key echo.
  const { idempotencyKey, ...operation } =
    (await response.json()) as OperationSnapshot & {
      idempotencyKey: string
    }
  const href = response.headers.get("location")
  if (response.status !== 202 || idempotencyKey !== attempt.key || !href) {
    throw new Error("Tracer did not return a verifiable publication receipt")
  }
  return {
    href: advertisedHref(href),
    operation: validateSnapshot(operation, attempt),
  }
}

export async function readOperatorStatus(
  receipt: OperatorReceipt,
  attempt: OperatorAttempt,
  capability: TracerCapability,
  signal: AbortSignal
): Promise<OperatorReceipt> {
  requireMode(attempt.mode, capability)
  const operation = validateSnapshot(
    await requestJson<OperationSnapshot>(
      receipt.href,
      { signal, cache: "no-store" },
      capability
    ),
    attempt
  )
  if (
    operation.operationId !== receipt.operation.operationId ||
    operation.correlationId !== receipt.operation.correlationId
  ) {
    throw new Error("The operation changed identity")
  }
  return { href: receipt.href, operation }
}

function requireMode(mode: OperatorMode, capability: TracerCapability) {
  if (
    mode === "test"
      ? capability !== "control"
      : typeof capability !== "object" || capability.kind !== "dispatch"
  ) {
    throw new Error("The credential does not match this command target")
  }
}

function validateSnapshot(
  operation: OperationSnapshot,
  attempt: OperatorAttempt
) {
  if (
    operation?.mode !== attempt.mode ||
    operation.context !== attempt.context ||
    operation.command !== attempt.command ||
    operation.schemaVersion !== attempt.schemaVersion ||
    !["queued", "running", "completed", "failed", "indeterminate"].includes(
      operation.status
    ) ||
    typeof operation.operationId !== "string" ||
    !operation.operationId ||
    typeof operation.correlationId !== "string" ||
    !operation.correlationId
  )
    throw new Error("Tracer did not return the requested operation")
  return operation
}

export function definitiveOperatorRejection(error: unknown) {
  return (
    error instanceof TracerResponseError &&
    ([400, 401, 403, 404, 413, 422, 429].includes(error.status) ||
      (error.status === 501 && error.code === "mode-unavailable") ||
      (error.status === 503 &&
        [
          "test-scenario-unavailable",
          "capacity-exhausted",
          "concurrency-exhausted",
        ].includes(error.code ?? "")))
  )
}
