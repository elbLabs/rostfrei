interface ProjectStructureExample {
  readonly root: string
  readonly activeFile: string
  readonly files: readonly string[]
}

export const projectStructureExamples = {
  "domain-root": {
    root: "domain",
    activeFile: "model.rs",
    files: ["mod.rs", "model.rs"],
  },
  "bounded-context": {
    root: "bike_rental",
    activeFile: "context.rs",
    files: ["mod.rs", "context.rs"],
  },
  aggregate: {
    root: "rental_fleet",
    activeFile: "aggregate.rs",
    files: ["mod.rs", "aggregate.rs", "root.rs", "event_set.rs"],
  },
  entity: {
    root: "bicycle",
    activeFile: "entity.rs",
    files: ["mod.rs", "entity.rs", "identity.rs"],
  },
  "simple-value-object": {
    root: "condition",
    activeFile: "value.rs",
    files: ["mod.rs", "value.rs"],
  },
  "behaviorful-value-object": {
    root: "registration_number",
    activeFile: "value.rs",
    files: [
      "mod.rs",
      "value.rs",
      "normalize/action.rs",
      "normalize/execute.rs",
      "choose_format/policy.rs",
      "choose_format/outcome.rs",
      "choose_format/evaluate.rs",
      "validity/contract.rs",
      "validity/evaluate.rs",
    ],
  },
  "domain-service": {
    root: "fleet_planning",
    activeFile: "service.rs",
    files: [
      "mod.rs",
      "service.rs",
      "rebalance_fleet/action.rs",
      "rebalance_fleet/execute.rs",
      "assess_demand/policy.rs",
      "assess_demand/evaluate.rs",
    ],
  },
  "aggregate-action": {
    root: "rent_bicycle",
    activeFile: "action.rs",
    files: ["mod.rs", "action.rs", "execute.rs"],
  },
  "entity-action": {
    root: "change_condition",
    activeFile: "action.rs",
    files: ["mod.rs", "action.rs", "execute.rs"],
  },
  "value-object-action": {
    root: "normalize",
    activeFile: "action.rs",
    files: ["mod.rs", "action.rs", "execute.rs"],
  },
  query: {
    root: "bicycle_availability",
    activeFile: "query.rs",
    files: ["mod.rs", "query.rs", "execute.rs", "output.rs"],
  },
  "aggregate-policy": {
    root: "plan_fleet_capacity",
    activeFile: "policy.rs",
    files: ["mod.rs", "policy.rs", "outcome.rs", "evaluate.rs"],
  },
  "entity-policy": {
    root: "assess_rental_eligibility",
    activeFile: "policy.rs",
    files: ["mod.rs", "policy.rs", "outcome.rs", "evaluate.rs"],
  },
  "value-object-policy": {
    root: "choose_format",
    activeFile: "policy.rs",
    files: ["mod.rs", "policy.rs", "outcome.rs", "evaluate.rs"],
  },
  "aggregate-invariant": {
    root: "fleet_consistency",
    activeFile: "contract.rs",
    files: ["mod.rs", "contract.rs", "evaluate.rs"],
  },
  "value-object-invariant": {
    root: "validity",
    activeFile: "contract.rs",
    files: ["mod.rs", "contract.rs", "evaluate.rs"],
  },
  "entity-lifecycle": {
    root: "rental_status",
    activeFile: "lifecycle.rs",
    files: ["mod.rs", "lifecycle.rs", "transition.rs"],
  },
} as const satisfies Record<string, ProjectStructureExample>

export type ProjectStructureExampleName = keyof typeof projectStructureExamples
