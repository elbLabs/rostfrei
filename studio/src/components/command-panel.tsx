import { useEffect, useState, type FormEvent } from "react"
import { ListTree, LoaderCircle, Play, RotateCcw, Send } from "lucide-react"

import { DraggablePanel } from "@/components/studio-sidebar"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { getCommandInputs } from "@/lib/api"
import type {
  CatalogCommand,
  CatalogCommandField,
  CatalogCommandVersion,
  CatalogContext,
  CatalogFieldValue,
  CommandInputField,
  CommandExecutionResult,
  TracerCatalog,
} from "@/lib/types"

interface CommandExecutionRequestBase {
  context: string
  commandId: string
  commandLabel: string
  payload: unknown
  payloadJson: string
  schemaVersion: number
}

export type CommandExecutionRequest = CommandExecutionRequestBase &
  (
    | {
        mode: "preview"
        submitHrefTemplate: string
      }
    | {
        mode: "test"
        submitHrefTemplate: string
        idempotencyKey: string
      }
  )

interface CommandPanelProps {
  open: boolean
  catalog?: TracerCatalog
  busy: boolean
  result?: CommandExecutionResult
  requestError?: string
  testStateRevision: number
  onClose: () => void
  onExecute: (request: CommandExecutionRequest) => Promise<boolean>
  onEdit: () => void
  onRefreshTestState: () => void
}

interface CommandChoice {
  key: string
  context: CatalogContext
  command: CatalogCommand
  version: CatalogCommandVersion
}

export function CommandPanel({
  open,
  catalog,
  busy,
  result,
  requestError,
  testStateRevision,
  onClose,
  onExecute,
  onEdit,
  onRefreshTestState,
}: CommandPanelProps) {
  const choices = commandChoices(catalog)
  const testAvailable = choices.some(
    (choice) => choice.version.testHrefTemplate !== undefined
  )
  const [choiceKey, setChoiceKey] = useState("")
  const [mode, setMode] = useState<"preview" | "test">("test")
  const [payloadValues, setPayloadValues] = useState<Record<string, string>>({})
  const [validationError, setValidationError] = useState<string>()
  const [idempotencyKey, setIdempotencyKey] = useState(createIdempotencyKey)
  const [commandInputs, setCommandInputs] = useState<{
    key: string
    fields: CommandInputField[]
  }>()
  const [inputsError, setInputsError] = useState<{
    key: string
    message: string
  }>()
  const selected = choices.find((choice) => choice.key === choiceKey)
  const selectedFields = selected?.version.fields
  const inputsHrefTemplate = selected?.version.testInputsHrefTemplate
  const inputsKey = inputsHrefTemplate
    ? `${inputsHrefTemplate}\u0000${testStateRevision}`
    : undefined
  const currentCommandInputs =
    inputsKey && commandInputs?.key === inputsKey ? commandInputs.fields : []
  const currentInputsError =
    inputsKey && inputsError?.key === inputsKey
      ? inputsError.message
      : undefined
  const inputsLoading = Boolean(
    inputsKey &&
    commandInputs?.key !== inputsKey &&
    inputsError?.key !== inputsKey
  )

  useEffect(() => {
    if (!inputsHrefTemplate || !inputsKey) return
    const requestKey = inputsKey
    let active = true
    void getCommandInputs(inputsHrefTemplate)
      .then((document) => {
        if (!active) return
        setCommandInputs({ key: requestKey, fields: document.fields })
        setPayloadValues((current) =>
          createPayloadDefaults(selectedFields ?? [], document.fields, current)
        )
      })
      .catch((error) => {
        if (!active) return
        setCommandInputs(undefined)
        setPayloadValues((current) =>
          createPayloadDefaults(selectedFields ?? [], [], current)
        )
        setInputsError({
          key: requestKey,
          message:
            error instanceof Error ? error.message : "Payload options failed",
        })
      })
    return () => {
      active = false
    }
  }, [inputsHrefTemplate, inputsKey, selectedFields, testStateRevision])

  const markEdited = () => {
    setValidationError(undefined)
    onEdit()
  }

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    if (!selected) return

    try {
      const builtPayload = buildPayload(
        selected.version.fields,
        payloadValues,
        currentCommandInputs,
        selected.version.payloadTemplate
      )
      const request: CommandExecutionRequestBase = {
        context: selected.context.id,
        commandId: selected.command.id,
        commandLabel: selected.command.label,
        payload: builtPayload.value,
        payloadJson: builtPayload.json,
        schemaVersion: selected.version.schemaVersion,
      }
      setValidationError(undefined)
      if (mode === "preview") {
        void onExecute({
          ...request,
          mode,
          submitHrefTemplate: selected.version.simulateHrefTemplate,
        })
        return
      }

      if (!selected.version.testHrefTemplate) {
        throw new Error("Test publication is not available for this command")
      }
      if (!VALID_IDEMPOTENCY_KEY.test(idempotencyKey)) {
        throw new Error(
          "Idempotency key must be 1-128 letters, numbers, dots, colons, dashes, or underscores"
        )
      }
      const publication: Extract<CommandExecutionRequest, { mode: "test" }> = {
        ...request,
        mode,
        submitHrefTemplate: selected.version.testHrefTemplate,
        idempotencyKey,
      }
      onClose()
      void onExecute(publication).then((rotateIdempotencyKey) => {
        if (!rotateIdempotencyKey) return
        setIdempotencyKey((current) =>
          current === publication.idempotencyKey
            ? createIdempotencyKey()
            : current
        )
      })
    } catch (error) {
      setValidationError(
        error instanceof Error ? error.message : "Payload is incomplete"
      )
    }
  }

  const error = validationError ?? requestError

  return (
    <DraggablePanel
      id="studio-command-panel"
      className="studio-command-panel"
      label="Command"
      icon={Send}
      open={open}
      initialPosition={{ x: 608, y: 70 }}
      closeLabel="Close command panel"
      onClose={onClose}
    >
      <div className="command-panel-scroll min-h-0 flex-1 overflow-y-auto px-3 pb-3">
        <form className="command-form" onSubmit={submit}>
          <div className="command-mode-row">
            <div
              className="command-mode-options"
              role="group"
              aria-label="Command execution mode"
            >
              <button
                type="button"
                className="command-mode-option"
                data-command-mode="preview"
                aria-pressed={mode === "preview"}
                disabled={busy}
                onClick={() => {
                  setMode("preview")
                  markEdited()
                }}
              >
                Preview
              </button>
              <button
                type="button"
                className="command-mode-option"
                data-command-mode="test"
                aria-pressed={mode === "test"}
                disabled={busy || !testAvailable}
                onClick={() => {
                  setMode("test")
                  markEdited()
                }}
              >
                Test bus
              </button>
            </div>
            <p>
              {mode === "test"
                ? "Publishes through the isolated Test command bus and may append Test events."
                : "Reads isolated Test history. Nothing is published or appended."}
            </p>
          </div>

          <label className="command-field">
            <span>Command and schema</span>
            <select
              aria-label="Command and schema"
              value={choiceKey}
              disabled={busy || choices.length === 0}
              onChange={(event) => {
                const nextKey = event.target.value
                const next = choices.find((choice) => choice.key === nextKey)
                setChoiceKey(nextKey)
                if (!next?.version.testHrefTemplate) setMode("preview")
                setCommandInputs(undefined)
                setInputsError(undefined)
                setPayloadValues(
                  createPayloadDefaults(next?.version.fields ?? [])
                )
                markEdited()
              }}
            >
              <option value="">Choose a command</option>
              {choices.map((choice) => (
                <option value={choice.key} key={choice.key}>
                  {choice.context.label} / {choice.command.label} / v
                  {choice.version.schemaVersion}
                </option>
              ))}
            </select>
          </label>

          <div className="command-payload-fields">
            <div className="command-payload-heading">
              <ListTree className="size-3" />
              <span>Payload</span>
            </div>
            {!selected ? (
              <p>Select a command to see its payload fields.</p>
            ) : selected.version.fields.length === 0 ? (
              <p>This command has no payload fields.</p>
            ) : (
              selected.version.fields.map((field) => (
                <PayloadField
                  key={field.name}
                  field={field}
                  input={currentCommandInputs.find(
                    (input) => input.name === field.name
                  )}
                  value={payloadValues[field.name]}
                  disabled={busy || inputsLoading}
                  onChange={(value) => {
                    setPayloadValues((current) => ({
                      ...current,
                      [field.name]: value,
                    }))
                    markEdited()
                  }}
                />
              ))
            )}
            {inputsLoading && (
              <small>Loading available payload values...</small>
            )}
            {currentInputsError && (
              <small className="command-input-error">
                {currentInputsError}
              </small>
            )}
          </div>

          {mode === "test" && (
            <label className="command-field">
              <span>Idempotency key</span>
              <div className="command-idempotency-field">
                <input
                  aria-label="Idempotency key"
                  value={idempotencyKey}
                  disabled={busy}
                  autoComplete="off"
                  spellCheck={false}
                  onChange={(event) => {
                    setIdempotencyKey(event.target.value)
                    markEdited()
                  }}
                />
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  disabled={busy}
                  onClick={() => {
                    setIdempotencyKey(createIdempotencyKey())
                    markEdited()
                    onRefreshTestState()
                  }}
                >
                  <RotateCcw />
                  New key
                </Button>
              </div>
              <small>
                Keep this key for retries. Generate a new one only for a new
                publication.
              </small>
            </label>
          )}

          {error && (
            <p className="command-form-error" role="alert">
              {error}
            </p>
          )}

          <Button
            type="submit"
            className="command-submit"
            disabled={
              busy ||
              inputsLoading ||
              !selected ||
              (mode === "test" && !selected.version.testHrefTemplate)
            }
          >
            {busy ? (
              <LoaderCircle className="animate-spin" />
            ) : mode === "test" ? (
              <Send />
            ) : (
              <Play />
            )}
            {busy
              ? "Waiting for result"
              : mode === "test"
                ? "Push to Test bus"
                : "Preview command"}
          </Button>
        </form>

        {!catalog && (
          <p className="command-empty">Connect Tracer to discover commands.</p>
        )}

        {result && <CommandResult result={result} />}
      </div>
    </DraggablePanel>
  )
}

function PayloadField({
  field,
  input,
  value,
  disabled,
  onChange,
}: {
  field: CatalogCommandField
  input?: CommandInputField
  value?: string
  disabled: boolean
  onChange: (value: string) => void
}) {
  const label = input?.label ?? humanize(field.name)
  const descriptor = unwrapOptional(field.value)
  const identifier = isIdentifierField(field)
  const selectionInput = identifier ? undefined : input
  const manualOptional =
    isOptional(field.value) &&
    !selectionInput &&
    !(descriptor.kind === "scalar" && scalarType(descriptor) === "bool")
  const noValue = value === NO_VALUE
  const controlValue = noValue ? undefined : value
  const simpleList = descriptor.kind === "list" && isSimpleList(descriptor)
  const emptyList = value === EMPTY_LIST
  const selectedOption = controlValue?.startsWith("option:")
    ? selectionInput?.options[Number(controlValue.slice(7))]
    : undefined

  return (
    <div className="command-field" data-payload-field={field.name}>
      <span>{label}</span>
      {identifier ? (
        <input
          type="text"
          aria-label={`Payload ${label}`}
          value={controlValue ?? ""}
          disabled={disabled || noValue}
          placeholder={`Enter ${label}`}
          autoComplete="off"
          spellCheck={false}
          onChange={(event) => onChange(event.target.value)}
        />
      ) : selectionInput ? (
        <select
          aria-label={`Payload ${label}`}
          value={controlValue ?? ""}
          disabled={
            disabled ||
            (selectionInput.options.length === 0 && !isOptional(field.value))
          }
          onChange={(event) => onChange(event.target.value)}
        >
          <option value="">
            {selectionInput.options.length === 0
              ? "No available values"
              : `Choose ${label}`}
          </option>
          {isOptional(field.value) && <option value="null">None</option>}
          {selectionInput.options.map((option, index) => (
            <option value={`option:${index}`} key={`${option.label}-${index}`}>
              {option.label}
            </option>
          ))}
        </select>
      ) : descriptor.kind === "scalar" && scalarType(descriptor) === "bool" ? (
        <select
          aria-label={`Payload ${label}`}
          value={controlValue ?? ""}
          disabled={disabled}
          onChange={(event) => onChange(event.target.value)}
        >
          <option value="">Choose true or false</option>
          {isOptional(field.value) && <option value="null">None</option>}
          <option value="true">True</option>
          <option value="false">False</option>
        </select>
      ) : simpleList ? (
        <div className="command-list-field">
          <textarea
            aria-label={`Payload ${label}`}
            value={emptyList || noValue ? "" : (value ?? "")}
            disabled={disabled || emptyList || noValue}
            rows={3}
            spellCheck={false}
            placeholder="One value per line"
            onChange={(event) => onChange(event.target.value)}
          />
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={disabled}
            onClick={() => onChange(emptyList ? "" : EMPTY_LIST)}
          >
            {emptyList ? "Enter items" : "Use empty list"}
          </Button>
        </div>
      ) : descriptor.kind !== "scalar" ? (
        <textarea
          aria-label={`Payload ${label}`}
          value={controlValue ?? ""}
          disabled={disabled || noValue}
          rows={3}
          spellCheck={false}
          placeholder="Enter one JSON value"
          onChange={(event) => onChange(event.target.value)}
        />
      ) : (
        <input
          aria-label={`Payload ${label}`}
          value={controlValue ?? ""}
          disabled={disabled || noValue}
          inputMode={isNumeric(descriptor) ? "decimal" : "text"}
          placeholder={isOptional(field.value) ? "Optional" : `Enter ${label}`}
          autoComplete="off"
          onChange={(event) => onChange(event.target.value)}
        />
      )}
      {selectedOption?.description && (
        <small>{selectedOption.description}</small>
      )}
      {simpleList && <small>Enter one item per line.</small>}
      {!identifier &&
        descriptor.kind !== "scalar" &&
        descriptor.kind !== "list" &&
        !input && (
          <small>
            Tracer does not advertise a scalar shape; enter one JSON value.
          </small>
        )}
      {manualOptional && (
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="command-optional-toggle"
          disabled={disabled}
          onClick={() => onChange(noValue ? "" : NO_VALUE)}
        >
          {noValue ? "Use a value" : "Use no value"}
        </Button>
      )}
    </div>
  )
}

function buildPayload(
  fields: CatalogCommandField[],
  values: Record<string, string>,
  inputs: CommandInputField[],
  template: unknown
): { json: string; value: unknown } {
  const inputsByName = new Map(inputs.map((input) => [input.name, input]))
  const properties = fields.map((field) => {
    const advertisedInput = inputsByName.get(field.name)
    const label = advertisedInput?.label ?? humanize(field.name)
    const encoded = encodeField(
      field.value,
      values[field.name],
      label,
      isIdentifierField(field) ? undefined : advertisedInput,
      field.name
    )
    return `${JSON.stringify(field.name)}:${encoded}`
  })
  const tuple =
    fields.length > 0 &&
    fields.every((field, index) => field.name === String(index))
  const json =
    Array.isArray(template) || tuple
      ? `[${fields
          .map((field) =>
            encodeField(
              field.value,
              values[field.name],
              inputsByName.get(field.name)?.label ?? humanize(field.name),
              isIdentifierField(field)
                ? undefined
                : inputsByName.get(field.name),
              field.name
            )
          )
          .join(",")}]`
      : template === null && fields.length === 0
        ? "null"
        : `{${properties.join(",")}}`
  return { json, value: JSON.parse(json) as unknown }
}

function encodeField(
  descriptor: CatalogFieldValue,
  value: string | undefined,
  label: string,
  input?: CommandInputField,
  fieldName?: string
): string {
  if (input) {
    if (value === "null" && isOptional(descriptor)) return "null"
    if (!value?.startsWith("option:")) throw new Error(`${label} is required`)
    const option = input.options[Number(value.slice(7))]
    if (!option) throw new Error(`${label} must use an available value`)
    const encoded = option.valueJson ?? JSON.stringify(option.value)
    if (encoded === undefined) throw new Error(`${label} has an invalid value`)
    return encoded
  }

  if (descriptor.kind === "optional") {
    if (value === undefined || value === NO_VALUE) return "null"
    if (
      value === "null" &&
      descriptor.value.kind === "scalar" &&
      scalarType(descriptor.value) === "bool"
    ) {
      return "null"
    }
    return encodeField(descriptor.value, value, label, undefined, fieldName)
  }
  if (value === undefined) throw new Error(`${label} is required`)

  if (fieldName && isIdentifierDescriptor(fieldName, descriptor)) {
    if (value.trim().length === 0) throw new Error(`${label} is required`)
    return JSON.stringify(value)
  }

  if (descriptor.kind === "list") {
    if (!isSimpleList(descriptor)) {
      return validatedJson(value, label, Array.isArray)
    }
    const items = value === EMPTY_LIST ? [] : value.split(/\r?\n/)
    return `[${items
      .map((item) => encodeField(descriptor.element, item, `${label} item`))
      .join(",")}]`
  }
  if (descriptor.kind !== "scalar") return validatedJson(value, label)

  const scalar = scalarType(descriptor)
  if (scalar === "bool") {
    if (value !== "true" && value !== "false") {
      throw new Error(`${label} must be true or false`)
    }
    return value
  }
  if (integerScalars.has(scalar)) {
    const unsigned = scalar.startsWith("u") || scalar === "usize"
    const pattern = unsigned ? /^(0|[1-9]\d*)$/ : /^-?(0|[1-9]\d*)$/
    if (!pattern.test(value)) throw new Error(`${label} must be a whole number`)
    return value
  }
  if (scalar === "f32" || scalar === "f64") {
    if (!/^-?(0|[1-9]\d*)(\.\d+)?([eE][+-]?\d+)?$/.test(value)) {
      throw new Error(`${label} must be a number`)
    }
    return value
  }
  if (scalar === "char" && Array.from(value).length !== 1) {
    throw new Error(`${label} must be one character`)
  }
  return JSON.stringify(value)
}

const EMPTY_LIST = "\u0001"
const NO_VALUE = "\u0002"

function validatedJson(
  value: string,
  label: string,
  predicate?: (value: unknown) => boolean
): string {
  let parsed: unknown
  try {
    parsed = JSON.parse(value) as unknown
  } catch {
    throw new Error(`${label} must be a valid JSON value`)
  }
  if (predicate && !predicate(parsed)) {
    throw new Error(`${label} has the wrong JSON shape`)
  }
  return value
}

const integerScalars = new Set([
  "i8",
  "i16",
  "i32",
  "i64",
  "i128",
  "isize",
  "u8",
  "u16",
  "u32",
  "u64",
  "u128",
  "usize",
])

function unwrapOptional(value: CatalogFieldValue): CatalogFieldValue {
  return value.kind === "optional" ? value.value : value
}

function isOptional(value: CatalogFieldValue): boolean {
  return value.kind === "optional"
}

function isSimpleList(
  value: Extract<CatalogFieldValue, { kind: "list" }>
): boolean {
  return value.element.kind !== "optional" && value.element.kind !== "list"
}

function scalarType(
  value: Extract<CatalogFieldValue, { kind: "scalar" }>
): string {
  return typeof value.scalar === "string"
    ? value.scalar
    : value.scalar.representation
}

function isNumeric(value: CatalogFieldValue): boolean {
  return (
    value.kind === "scalar" &&
    (integerScalars.has(scalarType(value)) ||
      scalarType(value) === "f32" ||
      scalarType(value) === "f64")
  )
}

function createPayloadDefaults(
  fields: CatalogCommandField[],
  inputs: CommandInputField[] = [],
  current: Record<string, string> = {}
): Record<string, string> {
  const inputsByName = new Map(inputs.map((input) => [input.name, input]))
  return Object.fromEntries(
    fields.flatMap((field) => {
      const input = inputsByName.get(field.name)
      if (isIdentifierField(field)) {
        const advertised = input?.options.find(
          (option) => typeof option.value === "string"
        )?.value
        return [
          [
            field.name,
            typeof advertised === "string"
              ? advertised
              : (current[field.name] ?? crypto.randomUUID()),
          ],
        ]
      }
      return input && input.options.length > 0 ? [[field.name, "option:0"]] : []
    })
  )
}

function isIdentifierField(field: CatalogCommandField): boolean {
  return isIdentifierDescriptor(field.name, unwrapOptional(field.value))
}

function isIdentifierDescriptor(
  name: string,
  descriptor: CatalogFieldValue
): boolean {
  const identifierName = /(?:^|[_-])id$/i.test(name) || /Id$/.test(name)
  if (!identifierName) return false
  return (
    descriptor.kind === "opaque" ||
    descriptor.kind === "entity" ||
    descriptor.kind === "aggregateReference" ||
    (descriptor.kind === "scalar" &&
      (scalarType(descriptor) === "string" ||
        scalarType(descriptor) === "char"))
  )
}

function humanize(name: string): string {
  const value = name.replace(/[_-]+/g, " ")
  return `${value.charAt(0).toUpperCase()}${value.slice(1)}`
}

function CommandResult({ result }: { result: CommandExecutionResult }) {
  const { operation, series } = result
  const decision = operationDecision(operation.result)
  const label = decision ?? operation.status
  const value = operation.failure ?? operation.result

  return (
    <section className="command-result" data-command-result aria-live="polite">
      <div className="command-result-header">
        <span>
          {operation.mode === "test"
            ? "Test result"
            : operation.mode === "dispatch"
              ? "Production result"
              : "Preview result"}
        </span>
        <Badge
          variant={
            label === "accepted"
              ? "success"
              : label === "rejected" ||
                  label === "failed" ||
                  label === "indeterminate"
                ? "danger"
                : "neutral"
          }
        >
          {label}
        </Badge>
      </div>
      <dl className="command-result-meta">
        <div>
          <dt>Command</dt>
          <dd>
            {operation.command} v{operation.schemaVersion}
          </dd>
        </div>
        <div>
          <dt>Context</dt>
          <dd>{operation.context}</dd>
        </div>
        <div>
          <dt>Messages</dt>
          <dd>{series?.messageSeries.messages.length ?? "pending"}</dd>
        </div>
        <div>
          <dt>Capture</dt>
          <dd>
            {series
              ? `${series.capture.fidelity}${series.capture.settled ? "" : " / partial"}`
              : "not ready"}
          </dd>
        </div>
      </dl>
      {value !== undefined && (
        <pre className="command-result-payload">
          {JSON.stringify(value, null, 2)}
        </pre>
      )}
      {series?.capture.note && (
        <p className="command-result-note">{series.capture.note}</p>
      )}
      {result.inspectionError && (
        <p className="command-result-note">{result.inspectionError}</p>
      )}
    </section>
  )
}

const VALID_IDEMPOTENCY_KEY = /^[A-Za-z0-9._:-]{1,128}$/

function createIdempotencyKey(): string {
  return `studio:test:${crypto.randomUUID()}`
}

function commandChoices(catalog?: TracerCatalog): CommandChoice[] {
  return (catalog?.contexts ?? []).flatMap((context) =>
    context.commands.flatMap((command) =>
      command.versions.map((version) => ({
        key: `${context.id}/${command.id}@${version.schemaVersion}`,
        context,
        command,
        version,
      }))
    )
  )
}

function operationDecision(
  result: unknown
): "accepted" | "rejected" | undefined {
  if (typeof result !== "object" || result === null) return undefined
  const decision = Reflect.get(result, "decision")
  return decision === "accepted" || decision === "rejected"
    ? decision
    : undefined
}
