interface ProjectStructureExample {
  readonly root: string
  readonly directory: string
  readonly activeFile: string
  readonly files: readonly string[]
}

const domainDirectory = "src/domain"
const contextDirectory = `${domainDirectory}/bike_rental`
const fleetDirectory = `${contextDirectory}/rental_fleet`
const bicycleDirectory = `${fleetDirectory}/bicycle`
const registrationDirectory = `${bicycleDirectory}/registration_number`

export const projectStructureExamples = {
  "domain-root": {
    root: "domain",
    directory: domainDirectory,
    activeFile: "model.rs",
    files: ["mod.rs", "model.rs"],
  },
  "bounded-context": {
    root: "bike_rental",
    directory: contextDirectory,
    activeFile: "context.rs",
    files: ["mod.rs", "context.rs"],
  },
  aggregate: {
    root: "rental_fleet",
    directory: fleetDirectory,
    activeFile: "aggregate.rs",
    files: ["mod.rs", "aggregate.rs", "root.rs", "event_set.rs"],
  },
  entity: {
    root: "bicycle",
    directory: bicycleDirectory,
    activeFile: "entity.rs",
    files: ["mod.rs", "entity.rs", "identity.rs"],
  },
  "simple-value-object": {
    root: "condition",
    directory: `${bicycleDirectory}/condition`,
    activeFile: "value.rs",
    files: ["mod.rs", "value.rs"],
  },
  "behaviorful-value-object": {
    root: "registration_number",
    directory: registrationDirectory,
    activeFile: "value.rs",
    files: [
      "mod.rs",
      "value.rs",
      "normalize/mod.rs",
      "normalize/action.rs",
      "normalize/execute.rs",
      "choose_format/mod.rs",
      "choose_format/policy.rs",
      "choose_format/outcome.rs",
      "choose_format/evaluate.rs",
      "validity/mod.rs",
      "validity/contract.rs",
      "validity/evaluate.rs",
    ],
  },
  "domain-service": {
    root: "bicycle_transfer",
    directory: `${contextDirectory}/bicycle_transfer`,
    activeFile: "service.rs",
    files: [
      "mod.rs",
      "service.rs",
      "command.rs",
      "handler.rs",
      "rejection.rs",
      "transfer/action.rs",
      "transfer/execute.rs",
    ],
  },
  "aggregate-action": {
    root: "rent_bicycle",
    directory: `${fleetDirectory}/rent_bicycle`,
    activeFile: "action.rs",
    files: ["mod.rs", "action.rs", "execute.rs"],
  },
  "value-object-action": {
    root: "normalize",
    directory: `${registrationDirectory}/normalize`,
    activeFile: "action.rs",
    files: ["mod.rs", "action.rs", "execute.rs"],
  },
  query: {
    root: "bicycle_availability",
    directory: `${fleetDirectory}/bicycle_availability`,
    activeFile: "query.rs",
    files: ["mod.rs", "query.rs", "execute.rs", "output.rs"],
  },
  "entity-policy": {
    root: "assess_rental_eligibility",
    directory: `${bicycleDirectory}/assess_rental_eligibility`,
    activeFile: "policy.rs",
    files: ["mod.rs", "policy.rs", "outcome.rs", "evaluate.rs"],
  },
  "value-object-policy": {
    root: "choose_format",
    directory: `${registrationDirectory}/choose_format`,
    activeFile: "policy.rs",
    files: ["mod.rs", "policy.rs", "outcome.rs", "evaluate.rs"],
  },
  "aggregate-invariant": {
    root: "fleet_consistency",
    directory: `${fleetDirectory}/fleet_consistency`,
    activeFile: "contract.rs",
    files: ["mod.rs", "contract.rs", "evaluate.rs"],
  },
  "value-object-invariant": {
    root: "validity",
    directory: `${registrationDirectory}/validity`,
    activeFile: "contract.rs",
    files: ["mod.rs", "contract.rs", "evaluate.rs"],
  },
  "entity-lifecycle": {
    root: "rental_status",
    directory: `${bicycleDirectory}/rental_status`,
    activeFile: "lifecycle.rs",
    files: ["mod.rs", "lifecycle.rs", "transition.rs"],
  },
} as const satisfies Record<string, ProjectStructureExample>

export type ProjectStructureExampleName = keyof typeof projectStructureExamples
