const sources = import.meta.glob<string>("../../../docs/domain-macros/*.mdx", {
  eager: true,
  query: "?raw",
  import: "default",
})

const examples = new Map(
  Object.values(sources).flatMap((source) => {
    const name = /^# `([^`]+)`/m.exec(source)?.[1]
    const code = /^```rust\n([\s\S]*?)\n```/m.exec(source)?.[1]
    return name && code ? [[name, code] as const] : []
  })
)

export const macroExampleNames = [
  "install_macro_support!",
  "BoundedContext",
  "Aggregate",
  "AggregateEvents",
  "Entity",
  "DomainIdentity",
  "ValueObject",
  "DomainService",
  "Command",
  "DomainEvent",
  "DomainError",
  "PolicyOutcome",
  "EntityLifecycle",
  "StateTransition",
  "domain_action",
  "domain_query",
  "domain_policy",
  "domain_invariant",
  "domain_*_test",
  "domain_model!",
] as const

export type MacroExampleName = (typeof macroExampleNames)[number]

export function getMacroCode(name: MacroExampleName) {
  const code = examples.get(name)

  if (!code) {
    throw new Error(`Unknown macro example: ${name}`)
  }

  return code
}
