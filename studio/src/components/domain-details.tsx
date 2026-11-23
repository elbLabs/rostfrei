import { useEffect, useId, useLayoutEffect, useRef } from "react"
import { EvidenceDisclosure } from "@rostfrei/ui/evidence"
import { InspectorFrame } from "@/components/inspector-frame"
import {
  aggregatePath,
  domainKindLabels,
  entityPath,
  record,
  string,
  type DomainLayout,
  type DomainNode,
} from "./domain-layout"
import type { DomainInventory } from "./inventory-panel-data"

function shortLabel(
  id: string | undefined,
  local: unknown,
  model: DomainLayout
) {
  return (id ? model.nodes.get(id)?.label : undefined) ?? string(local)
}

function aggregateLabel(value: unknown, model: DomainLayout) {
  const path = aggregatePath(value)
  return shortLabel(path && `aggregate:${path}`, record(value).local, model)
}

function entityLabel(value: unknown, model: DomainLayout) {
  const path = entityPath(value)
  return shortLabel(path && `entity:${path}`, record(value).local, model)
}

function fieldType(value: unknown, model: DomainLayout): string | undefined {
  const field = record(value)
  switch (field.kind) {
    case "scalar": {
      const scalar = record(field.scalar)
      const name = string(scalar.label) ?? string(scalar.id)
      const representation = string(scalar.representation)
      return (
        string(field.scalar) ??
        (name && representation
          ? `${name} (${representation})`
          : (name ?? representation))
      )
    }
    case "entity": {
      const label = entityLabel(field.id, model)
      return label ? `Entity: ${label}` : "Entity"
    }
    case "aggregateReference": {
      const label = aggregateLabel(field.aggregate, model)
      return label ? `Reference to ${label}` : "Aggregate reference"
    }
    case "list": {
      const element = fieldType(field.element, model)
      return element ? `List<${element}>` : "List (element type not reported)"
    }
    case "optional": {
      const inner = fieldType(field.value, model)
      return inner ? `Option<${inner}>` : "Optional (type not reported)"
    }
  }
}

function DeclaredFields({
  fields,
  model,
}: {
  fields: unknown[]
  model: DomainLayout
}) {
  const rows = fields.flatMap((entry) => {
    const field = record(entry)
    const name = string(field.name)
    return name === undefined
      ? []
      : [
          {
            name,
            type: string(field.declaredType) ?? fieldType(field.value, model),
          },
        ]
  })
  return (
    <div className="domain-declared-fields">
      {rows.length ? (
        <table aria-label="Declared fields">
          <thead>
            <tr>
              <th scope="col">Field</th>
              <th scope="col">Type</th>
            </tr>
          </thead>
          <tbody>
            {rows.map(({ name, type }, index) => (
              <tr key={index}>
                <th scope="row">
                  <code>{name}</code>
                </th>
                <td>{type ?? "Not reported"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : (
        <p>No fields declared.</p>
      )}
    </div>
  )
}

function ContextDeclarations({
  context,
  data,
  model,
}: {
  context: string
  data: DomainInventory
  model: DomainLayout
}) {
  const groups = [
    {
      title: "Registered commands",
      entries: data.registeredCommands,
      contextOf: (entry: unknown) => record(entry).context,
    },
    {
      title: "Registered queries",
      entries: data.registeredQueries,
      contextOf: (entry: unknown) => record(entry).context,
    },
    {
      title: "Domain services",
      entries: data.domainServices,
      contextOf: (entry: unknown) => record(record(entry).id).context,
    },
  ]
    .map(({ title, entries, contextOf }) => ({
      title,
      entries: (entries ?? []).filter((entry) => contextOf(entry) === context),
    }))
    .filter(({ entries }) => entries.length)

  return (
    <section className="inspector-section">
      <h3>Declarations</h3>
      {groups.length ? (
        groups.map(({ title, entries }) => (
          <EvidenceDisclosure
            key={title}
            title={`${title} (${entries.length})`}
          >
            <ul className="domain-detail-list">
              {entries.map((entry, index) => {
                const descriptor = record(entry)
                const name =
                  string(descriptor.name) ?? string(record(descriptor.id).local)
                const label = string(descriptor.label) ?? name
                const version = descriptor.schemaVersion
                if (Array.isArray(descriptor.fields))
                  return (
                    <li key={index}>
                      <details
                        className="domain-command-definition"
                        data-domain-command={name}
                      >
                        <summary>
                          {label}{" "}
                          {typeof version === "number" && (
                            <small>v{version}</small>
                          )}
                        </summary>
                        <DeclaredFields
                          fields={descriptor.fields}
                          model={model}
                        />
                      </details>
                    </li>
                  )
                return (
                  <li key={index}>
                    <span>
                      {label ?? "Unnamed declaration"}
                      {name && name !== label && (
                        <small>
                          <code>{name}</code>
                        </small>
                      )}
                    </span>
                    {typeof version === "number" && <small>v{version}</small>}
                  </li>
                )
              })}
            </ul>
          </EvidenceDisclosure>
        ))
      ) : (
        <p>No commands, queries, or services reported.</p>
      )}
    </section>
  )
}

export function DomainDetails({
  node,
  model,
  data,
  onClose,
}: {
  node: DomainNode
  model: DomainLayout
  data: DomainInventory
  onClose: () => void
}) {
  const title = useRef<HTMLHeadingElement>(null)
  const titleId = useId()
  useLayoutEffect(() => title.current?.focus({ preventScroll: true }), [])
  useEffect(() => {
    const escape = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return
      event.preventDefault()
      onClose()
    }
    window.addEventListener("keydown", escape)
    return () => window.removeEventListener("keydown", escape)
  }, [onClose])

  const value = record(node.value)
  const id = record(value.id)
  const owner = aggregateLabel(id.aggregate, model)
  const contextId = string(id.context)
  const context = shortLabel(
    contextId && `context:${contextId}`,
    contextId,
    model
  )
  const root = entityLabel(value.root, model)
  const runtimeType = string(value.aggregateType)
  return (
    <InspectorFrame
      as="section"
      className="domain-details"
      role="dialog"
      label="Domain details"
      aria-describedby={titleId}
      close={{ label: "Close details", onClick: onClose }}
    >
      <div className="inspector-scroll">
        <header className="inspector-content-heading">
          <span
            className={
              node.kind === "event"
                ? "message-kind kind-domain-event"
                : "message-kind"
            }
          >
            {domainKindLabels[node.kind]}
          </span>
          <h2 id={titleId} ref={title} tabIndex={-1}>
            {node.label}
          </h2>
          <div className="domain-detail-meta">
            {typeof value.schemaVersion === "number" && (
              <span aria-label={`Schema version ${value.schemaVersion}`}>
                v{value.schemaVersion}
              </span>
            )}
            {owner && (
              <span className="domain-detail-owner">
                Owned by <strong>{owner}</strong>
              </span>
            )}
            {node.kind === "aggregate" && context && <span>{context}</span>}
          </div>
        </header>
        {(node.kind === "event" || node.kind === "entity") &&
          Array.isArray(value.fields) && (
            <section className="inspector-section">
              <DeclaredFields fields={value.fields} model={model} />
            </section>
          )}
        {node.kind === "context" && (
          <ContextDeclarations context={node.path} data={data} model={model} />
        )}
        {node.kind === "aggregate" &&
          (root || runtimeType || Boolean(value.anchor)) && (
            <section className="inspector-section">
              <h3>Definition</h3>
              <dl className="payload-list">
                {root && (
                  <div className="payload-row">
                    <dt>Root entity</dt>
                    <dd>{root}</dd>
                  </div>
                )}
                {runtimeType && (
                  <div className="payload-row">
                    <dt>Runtime type</dt>
                    <dd>{runtimeType}</dd>
                  </div>
                )}
                {Boolean(value.anchor) && (
                  <div className="payload-row">
                    <dt>Role</dt>
                    <dd>Correlation anchor</dd>
                  </div>
                )}
              </dl>
            </section>
          )}
      </div>
    </InspectorFrame>
  )
}
