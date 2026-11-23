import { useEffect, useRef, useState } from "react"
import { Input } from "@rostfrei/ui/components/input"
import { NativeSelect } from "@rostfrei/ui/components/native-select"
import { Textarea } from "@rostfrei/ui/components/textarea"
import { Button } from "@/components/ui/button"
import {
  getCatalog,
  getCommandInputs,
  TracerResponseError,
  tracerUrl,
  type TracerCapability,
} from "@/lib/api"
import {
  definitiveOperatorRejection,
  operatorChoices,
  publishOperatorCommand,
  readOperatorStatus,
  type OperatorAttempt,
  type OperatorCommand,
  type OperatorMode,
  type OperatorReceipt,
} from "@/lib/operator-api"
import type {
  CommandInputDocument,
  OperationSnapshot,
  TracerCatalog,
} from "@/lib/types"
import "./operator-commands.css"

export function OperatorCommands({
  productionOnly = false,
  controlCatalog,
}: {
  productionOnly?: boolean
  controlCatalog?: TracerCatalog
}) {
  if (productionOnly) {
    const choices = controlCatalog
      ? operatorChoices(controlCatalog, "dispatch")
      : []
    if (!choices.length) return null
    return (
      <section
        className="operator-commands"
        aria-label="Production dispatch"
        data-operator-commands
      >
        <h2>Production dispatch</h2>
        <OperatorTarget mode="dispatch" choices={choices} />
      </section>
    )
  }
  return <OnDemandCommandCatalog />
}

function OnDemandCommandCatalog() {
  const [catalog, setCatalog] = useState<TracerCatalog>()
  const [discovering, setDiscovering] = useState(false)
  const [discoveryError, setDiscoveryError] = useState<string>()
  const [mode, setMode] = useState<OperatorMode>("test")
  const [dispatchEnabled, setDispatchEnabled] = useState(false)
  const discovery = useRef<AbortController>(undefined)

  useEffect(() => () => discovery.current?.abort(), [])

  const enableTest = async () => {
    if (discovery.current) return
    const controller = new AbortController()
    discovery.current = controller
    setDiscovering(true)
    setDiscoveryError(undefined)
    try {
      const result = await getCatalog(controller.signal)
      if (!controller.signal.aborted) setCatalog(result)
    } catch (error) {
      if (!controller.signal.aborted) setDiscoveryError(operatorError(error))
    } finally {
      discovery.current = undefined
      if (!controller.signal.aborted) setDiscovering(false)
    }
  }

  const test = catalog ? operatorChoices(catalog, "test") : []
  const production = catalog ? operatorChoices(catalog, "dispatch") : []

  return (
    <section
      className="operator-commands"
      aria-label="Operator commands"
      data-operator-commands
    >
      <header>
        <h2>Operator commands</h2>
        <p>
          Publish a registered command separately from read-only observation.
        </p>
      </header>
      {!catalog ? (
        <>
          <p>
            Read-only by default. Enable Test control to discover isolated Test
            commands and check whether this Tracer offers Production dispatch.
          </p>
          <Button
            type="button"
            onClick={() => void enableTest()}
            disabled={discovering}
          >
            {discovering ? "Discovering capabilities…" : "Enable Test control"}
          </Button>
          {discoveryError && <p role="alert">{discoveryError}</p>}
        </>
      ) : (
        <>
          <div className="operator-actions" aria-label="Command target">
            <Button
              type="button"
              variant={mode === "test" ? "default" : "outline"}
              aria-pressed={mode === "test"}
              onClick={() => setMode("test")}
            >
              Isolated Test
            </Button>
            {production.length > 0 && (
              <Button
                type="button"
                variant={mode === "dispatch" ? "default" : "outline"}
                aria-pressed={mode === "dispatch"}
                onClick={() => {
                  setDispatchEnabled(true)
                  setMode("dispatch")
                }}
              >
                {dispatchEnabled
                  ? "Production dispatch"
                  : "Enable Production dispatch"}
              </Button>
            )}
          </div>
          {production.length === 0 && (
            <p>Production dispatch is not advertised by this Tracer.</p>
          )}
          <div hidden={mode !== "test"}>
            <OperatorTarget mode="test" choices={test} />
          </div>
          {dispatchEnabled && (
            <div hidden={mode !== "dispatch"}>
              <OperatorTarget mode="dispatch" choices={production} />
            </div>
          )}
        </>
      )}
    </section>
  )
}

type Tracking =
  | { kind: "receipt"; attempt: OperatorAttempt; receipt: OperatorReceipt }
  | {
      kind: "unconfirmed"
      attempt: OperatorAttempt
      conflict: boolean
      detail: string
    }
  | { kind: "not-submitted"; attempt: OperatorAttempt; detail: string }

function OperatorTarget({
  mode,
  choices,
}: {
  mode: OperatorMode
  choices: OperatorCommand[]
}) {
  const production = mode === "dispatch"
  const label = production ? "Production" : "Test"
  const [selected, setSelected] = useState("")
  const [payload, setPayload] = useState("")
  const [credentialReady, setCredentialReady] = useState(false)
  const [review, setReview] = useState<OperatorAttempt>()
  const [tracking, setTracking] = useState<Tracking>()
  const [busy, setBusy] = useState(false)
  const [validationError, setValidationError] = useState<string>()
  const [inspectionError, setInspectionError] = useState<string>()
  const [inputs, setInputs] = useState<CommandInputDocument>()
  const [inputsError, setInputsError] = useState<string>()
  const [inputsLoading, setInputsLoading] = useState(false)
  const credential = useRef("")
  const password = useRef<HTMLInputElement>(null)
  const network = useRef<AbortController>(undefined)
  const inputDiscovery = useRef<AbortController>(undefined)
  const command = selected === "" ? undefined : choices[Number(selected)]
  const locked = busy || Boolean(review || tracking)

  useEffect(
    () => () => {
      network.current?.abort()
      inputDiscovery.current?.abort()
      credential.current = ""
    },
    []
  )

  const access = (): TracerCapability | undefined =>
    !production
      ? "control"
      : credential.current
        ? { kind: "dispatch", token: credential.current }
        : undefined

  const changeCommand = (value: string) => {
    inputDiscovery.current?.abort()
    setInputs(undefined)
    setInputsError(undefined)
    setInputsLoading(false)
    setValidationError(undefined)
    setSelected(value)
    setPayload("")
  }

  const discoverInputs = async (href: string) => {
    inputDiscovery.current?.abort()
    const controller = new AbortController()
    inputDiscovery.current = controller
    setInputsLoading(true)
    setInputs(undefined)
    setInputsError(undefined)
    try {
      const result = await getCommandInputs(href, controller.signal)
      if (!controller.signal.aborted) setInputs(result)
    } catch (error) {
      if (!controller.signal.aborted) setInputsError(operatorError(error))
    } finally {
      if (!controller.signal.aborted) setInputsLoading(false)
    }
  }

  const reviewCommand = () => {
    if (!command || locked || !access()) return
    try {
      JSON.parse(payload)
    } catch {
      setValidationError(
        "Enter a complete, valid JSON payload for the selected schema."
      )
      return
    }
    setValidationError(undefined)
    setReview({
      ...command,
      mode,
      payload,
      key: `studio:operator:${mode}:${crypto.randomUUID()}`,
    })
  }

  const perform = async (read?: Extract<Tracking, { kind: "receipt" }>) => {
    const capability = access()
    const attempt = read?.attempt ?? review
    if (!attempt || !capability || network.current) return
    const controller = new AbortController()
    network.current = controller
    setBusy(true)
    setReview(undefined)
    setInspectionError(undefined)
    let receipt = read?.receipt
    try {
      if (!receipt) {
        receipt = await publishOperatorCommand(
          attempt,
          capability,
          controller.signal
        )
        if (!controller.signal.aborted)
          setTracking({ kind: "receipt", attempt, receipt })
      }
      if (!read && isTerminal(receipt.operation.status)) return
      const deadline = AbortSignal.timeout(10_000)
      const signal = AbortSignal.any([controller.signal, deadline])
      do {
        if (!read)
          await new Promise((resolve) => window.setTimeout(resolve, 300))
        if (signal.aborted) break
        receipt = await readOperatorStatus(receipt, attempt, capability, signal)
        if (!controller.signal.aborted)
          setTracking({ kind: "receipt", attempt, receipt })
      } while (!read && !isTerminal(receipt.operation.status))
    } catch (error) {
      if (controller.signal.aborted) return
      const detail = operatorError(error)
      if (receipt) {
        setInspectionError(
          `Status could not be refreshed. Showing the last reported status. ${detail}`
        )
      } else if (
        tracking?.kind !== "unconfirmed" &&
        definitiveOperatorRejection(error)
      ) {
        setTracking({ kind: "not-submitted", attempt, detail })
      } else {
        setTracking({
          kind: "unconfirmed",
          attempt,
          detail,
          conflict:
            error instanceof TracerResponseError && error.status === 409,
        })
      }
    } finally {
      network.current = undefined
      if (!controller.signal.aborted) setBusy(false)
    }
  }

  const acceptCredential = () => {
    const input = password.current
    const token = input?.value.trim() ?? ""
    if (input) input.value = ""
    if (!/^[\x21-\x7E]+$/.test(token)) {
      setValidationError("Enter a valid Dispatch credential without spaces.")
      return
    }
    credential.current = token
    setCredentialReady(true)
    setValidationError(undefined)
  }

  return (
    <div className="operator-target" data-operator-target={mode}>
      <p>
        {production
          ? "Production commands can change real state and trigger downstream integrations. Supply production values independently; Test inputs are not production data."
          : "Test publishes through the real command pipeline to isolated Test state. It does not dispatch to production."}
      </p>
      {production && !credentialReady ? (
        <div className="operator-field">
          <p>
            The separate Dispatch credential is kept in memory while this panel
            is mounted. It is also required to read the dispatched operation.
          </p>
          <label htmlFor="operator-dispatch-token">Dispatch credential</label>
          <Input
            id="operator-dispatch-token"
            ref={password}
            type="password"
            autoComplete="off"
            spellCheck={false}
            aria-label="Dispatch credential"
            onKeyDown={(event) => {
              if (event.key === "Enter") acceptCredential()
            }}
          />
          <Button type="button" variant="outline" onClick={acceptCredential}>
            Use dispatch credential
          </Button>
        </div>
      ) : choices.length === 0 ? (
        <p>No {label} command publication is advertised by this Tracer.</p>
      ) : (
        <>
          {production && (
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={busy}
              onClick={() => {
                credential.current = ""
                setCredentialReady(false)
                setReview(undefined)
              }}
            >
              Forget dispatch credential
            </Button>
          )}
          <label className="operator-field">
            {label} command schema
            <NativeSelect
              aria-label={`${label} command schema`}
              value={selected}
              disabled={locked}
              onChange={(event) => changeCommand(event.target.value)}
            >
              <option value="">Choose a command and schema version</option>
              {choices.map((item, index) => (
                <option key={index} value={String(index)}>
                  {item.context} / {item.command} v{item.schemaVersion}
                </option>
              ))}
            </NativeSelect>
          </label>
          {command && (
            <>
              <details>
                <summary>Advertised payload shape (not default values)</summary>
                <pre>{JSON.stringify(command.shape, null, 2)}</pre>
              </details>
              {!production && command.testInputsHref && (
                <>
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    disabled={inputsLoading || locked}
                    onClick={() => void discoverInputs(command.testInputsHref!)}
                  >
                    Show Test inputs
                  </Button>
                  {inputsLoading && (
                    <p role="status">Discovering isolated Test inputs…</p>
                  )}
                  {inputs && (
                    <section data-operator-test-inputs>
                      <h3>Available values from isolated Test state</h3>
                      {inputs.fields.length === 0 && (
                        <p>No inputs were advertised for this schema.</p>
                      )}
                      {inputs.fields.map((field) => (
                        <details key={field.name}>
                          <summary>
                            {field.label} ({field.options.length})
                          </summary>
                          <ul>
                            {field.options.map((option, index) => (
                              <li key={index}>
                                {option.label}:{" "}
                                <code>
                                  {option.valueJson ??
                                    JSON.stringify(option.value)}
                                </code>
                              </li>
                            ))}
                          </ul>
                        </details>
                      ))}
                    </section>
                  )}
                  {inputsError && <p role="alert">{inputsError}</p>}
                </>
              )}
              <label className="operator-field">
                {label} payload JSON
                <Textarea
                  aria-label={`${label} payload JSON`}
                  value={payload}
                  disabled={locked}
                  spellCheck={false}
                  rows={4}
                  onChange={(event) => {
                    setPayload(event.target.value)
                    setValidationError(undefined)
                  }}
                />
              </label>
              {!locked && (
                <Button type="button" onClick={reviewCommand}>
                  Review{" "}
                  {production ? "Production dispatch" : "Test publication"}
                </Button>
              )}
            </>
          )}
        </>
      )}
      {validationError && <p role="alert">{validationError}</p>}
      {review && (
        <section
          className="operator-review"
          data-operator-review={mode}
          aria-label={`${label} publication review`}
        >
          <h3>
            {production
              ? "Confirm Production dispatch"
              : "Review isolated Test publication"}
          </h3>
          {production && (
            <p>
              Confirm only if this exact target and payload are intended.
              Production state and downstream integrations may change.
            </p>
          )}
          <p>
            <strong>{production ? "Production" : "Isolated Test"}:</strong>{" "}
            {review.context} / {review.command} v{review.schemaVersion}
          </p>
          <p>
            Tracer endpoint: <code>{tracerUrl(review.href).href}</code>
          </p>
          <p>
            Idempotency key: <code>{review.key}</code>
          </p>
          <pre aria-label="Exact command payload">{review.payload}</pre>
          <div className="operator-actions">
            <Button
              type="button"
              variant={production ? "destructive" : "default"}
              onClick={() => void perform()}
            >
              {production
                ? "Confirm Production dispatch"
                : "Publish to isolated Test"}
            </Button>
            <Button
              type="button"
              variant="outline"
              onClick={() => setReview(undefined)}
            >
              Cancel review
            </Button>
          </div>
        </section>
      )}
      {busy && (
        <p role="status">
          {tracking?.kind === "receipt"
            ? "Checking operation status…"
            : "Waiting for a publication receipt…"}
        </p>
      )}
      {tracking && (
        <section
          className="operator-result"
          aria-label={`${label} operation result`}
        >
          <div aria-live="polite">
            {tracking.kind === "receipt" ? (
              <OperatorStatus operation={tracking.receipt.operation} />
            ) : (
              <>
                <h3 data-operator-status={tracking.kind}>
                  {tracking.kind === "unconfirmed"
                    ? "Publication unconfirmed"
                    : "Submission refused"}
                </h3>
                <p>{tracking.detail}</p>
                {tracking.kind === "unconfirmed" && (
                  <p>
                    {tracking.conflict
                      ? "That key conflicts with an existing operation. Check recorded activity; do not switch keys to resend."
                      : `The command may already have reached the bus. Only retry the exact request with the same key. ${production ? "Check recorded activity first." : "Do not retry after a Test reset: the key is scoped to the previous Test generation."}`}
                  </p>
                )}
              </>
            )}
            <p>
              Idempotency key: <code>{tracking.attempt.key}</code>
            </p>
            {inspectionError && (
              <p data-operator-inspection-error>{inspectionError}</p>
            )}
          </div>
          {!review && (
            <div className="operator-actions">
              {tracking.kind === "receipt" &&
                (!isTerminal(tracking.receipt.operation.status) ||
                  inspectionError) && (
                  <Button
                    type="button"
                    variant="outline"
                    disabled={busy || (production && !credentialReady)}
                    onClick={() => void perform(tracking)}
                  >
                    Refresh operation status
                  </Button>
                )}
              {tracking.kind === "unconfirmed" && !tracking.conflict && (
                <Button
                  type="button"
                  variant="outline"
                  disabled={busy || (production && !credentialReady)}
                  onClick={() => setReview(tracking.attempt)}
                >
                  Review exact retry
                </Button>
              )}
              {(tracking.kind === "not-submitted" ||
                (tracking.kind === "receipt" &&
                  isTerminal(tracking.receipt.operation.status))) && (
                <Button
                  type="button"
                  variant="outline"
                  disabled={busy}
                  onClick={() => {
                    setTracking(undefined)
                    setInspectionError(undefined)
                    if (tracking.kind === "receipt") changeCommand("")
                  }}
                >
                  {" "}
                  {tracking.kind === "not-submitted"
                    ? "Edit command"
                    : "Start a different command"}
                </Button>
              )}
            </div>
          )}
          <p>
            Tracking is kept only in this view. Leaving it does not cancel
            publication. Use a separate tab to inspect recorded activity if you
            need to keep tracking this request.
          </p>
        </section>
      )}
    </div>
  )
}

function isTerminal(status: OperationSnapshot["status"]) {
  return (
    status === "completed" || status === "failed" || status === "indeterminate"
  )
}

function OperatorStatus({ operation }: { operation: OperationSnapshot }) {
  const { result, status } = operation
  const decision =
    status === "completed" &&
    typeof result === "object" &&
    result !== null &&
    "decision" in result
      ? result.decision
      : undefined
  const knownDecision =
    decision === "accepted" || decision === "rejected" ? decision : undefined
  return (
    <>
      <h3 data-operator-status={knownDecision ?? status}>
        Operation {status}
        {knownDecision && ` · command ${knownDecision}`}
      </h3>
      {(status === "queued" || status === "running") && (
        <p>No final business decision yet.</p>
      )}
      {status === "completed" && !knownDecision && (
        <p>No recognized business decision was returned.</p>
      )}
      {status === "failed" && (
        <p>The operation failed without establishing a business decision.</p>
      )}
      {status === "indeterminate" && (
        <p>
          Publication may have occurred, but no durable business decision was
          confirmed. Check recorded activity; do not submit another command to
          infer the result.
        </p>
      )}
      <p>
        Operation: <code>{operation.operationId}</code>
      </p>
      <p>
        Correlation: <code>{operation.correlationId}</code>
      </p>
    </>
  )
}

function operatorError(error: unknown) {
  if (error instanceof TracerResponseError) {
    if (error.status === 401 || error.status === 403)
      return `Tracer denied this credential (HTTP ${error.status}).`
    if (error.status === 404 || error.status === 410)
      return `The requested Tracer resource was not found or is no longer retained (HTTP ${error.status}).`
    return `Tracer returned HTTP ${error.status}.`
  }
  return "Tracer returned no usable response. Check the connection and advertised capability."
}
