import { MACROS } from "@/components/macro-explorer/macro-data"

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
  "DecisionOutcome",
  "EntityLifecycle",
  "domain_action",
  "domain_query",
  "domain_decision",
  "domain_invariant",
  "domain_*_test",
  "domain_model!",
] as const

export type MacroExampleName = (typeof macroExampleNames)[number]

export function getMacroCode(name: MacroExampleName) {
  const macro = MACROS.find((candidate) => candidate.name === name)

  if (!macro) {
    throw new Error(`Unknown macro example: ${name}`)
  }

  return macro.authored
}
