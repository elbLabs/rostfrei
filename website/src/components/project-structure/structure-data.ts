import type { StructureNode } from "./types"

type NodeCopy = Pick<
  StructureNode,
  "role" | "summary" | "allowed" | "guarantee"
>

function file(
  id: string,
  name: string,
  path: string,
  copy: NodeCopy
): StructureNode {
  return { id, name, path, kind: "file", ...copy }
}

function moduleFile(id: string, path: string): StructureNode {
  return file(`${id}-module`, "mod.rs", `${path}/mod.rs`, {
    role: "Module composition",
    summary:
      "Declares and re-exports the focused files and child modules in this directory.",
    allowed: ["Module declarations", "Scoped re-exports", "No domain behavior"],
    guarantee:
      "Composition stays separate from declarations and implementation logic.",
  })
}

function directory(
  id: string,
  name: string,
  path: string,
  copy: NodeCopy,
  children: StructureNode[]
): StructureNode {
  return {
    id,
    name,
    path,
    kind: "directory",
    ...copy,
    children: [moduleFile(id, path), ...children],
  }
}

const rentalStatus = directory(
  "rental-status",
  "rental_status",
  "src/domain/bike_rental/rental_fleet/bicycle/rental_status",
  {
    role: "Lifecycle",
    summary: "Declares Bicycle states and legal transitions.",
    allowed: ["lifecycle.rs", "transition.rs", "mod.rs"],
    guarantee:
      "Lifecycle state and transition IDs remain stable without mixing business policy into the Entity file.",
  },
  [
    file(
      "lifecycle",
      "lifecycle.rs",
      "src/domain/bike_rental/rental_fleet/bicycle/rental_status/lifecycle.rs",
      {
        role: "Lifecycle declaration",
        summary:
          "Declares the owner-independent lifecycle, initial state, and state metadata.",
        allowed: ["One EntityLifecycle enum", "State metadata", "Imports"],
        guarantee:
          "The lifecycle enum declares one initial state and an ID for each state.",
      }
    ),
    file(
      "transition",
      "transition.rs",
      "src/domain/bike_rental/rental_fleet/bicycle/rental_status/transition.rs",
      {
        role: "State transition declaration",
        summary:
          "Declares each logical trigger and its deterministic source/target edges.",
        allowed: [
          "One StateTransition enum",
          "Transition and edge metadata",
          "Imports",
        ],
        guarantee:
          "Transition topology is explicit and can be evaluated without side effects.",
      }
    ),
  ]
)

const bicycleCondition = directory(
  "bicycle-condition",
  "condition",
  "src/domain/bike_rental/rental_fleet/bicycle/condition",
  {
    role: "Simple Value Object",
    summary:
      "Uses the same module shape as a behaviorful Value Object with no extra capability directories.",
    allowed: ["value.rs", "mod.rs"],
    guarantee:
      "A simple Value Object can grow behavior without changing its module identity or moving its declaration later.",
  },
  [
    file(
      "condition-value",
      "value.rs",
      "src/domain/bike_rental/rental_fleet/bicycle/condition/value.rs",
      {
        role: "Value Object declaration",
        summary: "Defines BicycleCondition and its stable semantic metadata.",
        allowed: [
          "One ValueObject declaration",
          "Direct inherent implementations",
          "Imports",
        ],
        guarantee:
          "The checker requires exactly one Value Object in the module anchor.",
      }
    ),
  ]
)

const normalizeRegistrationNumber = directory(
  "normalize-registration-number",
  "normalize",
  "src/domain/bike_rental/rental_fleet/bicycle/registration_number/normalize",
  {
    role: "Value Object action",
    summary: "Normalizes one Registration Number through an ordinary trait.",
    allowed: ["action.rs", "execute.rs", "mod.rs"],
    guarantee:
      "The action implementation must directly target RegistrationNumber from the parent value.rs.",
  },
  [
    file(
      "registration-action",
      "action.rs",
      "src/domain/bike_rental/rental_fleet/bicycle/registration_number/normalize/action.rs",
      {
        role: "Value Object action contract",
        summary: "Declares NormalizeRegistrationNumber and its semantic ID.",
        allowed: [
          "One #[domain_action] trait",
          "Its method signature",
          "Imports",
        ],
        guarantee: "The action contract remains ordinary callable Rust.",
      }
    ),
    file(
      "registration-action-execute",
      "execute.rs",
      "src/domain/bike_rental/rental_fleet/bicycle/registration_number/normalize/execute.rs",
      {
        role: "Value Object action implementation",
        summary: "Implements normalization directly for RegistrationNumber.",
        allowed: [
          "One matching trait implementation",
          "Private helpers",
          "Imports",
        ],
        guarantee:
          "The checker rejects another owner, qualified aliases, and duplicate implementations.",
      }
    ),
  ]
)

const registrationNumberValidity = directory(
  "registration-number-validity",
  "validity",
  "src/domain/bike_rental/rental_fleet/bicycle/registration_number/validity",
  {
    role: "Value Object invariant",
    summary: "Names and evaluates the Registration Number validity rule.",
    allowed: ["contract.rs", "evaluate.rs", "mod.rs"],
    guarantee:
      "The invariant implementation must directly target RegistrationNumber.",
  },
  [
    file(
      "registration-invariant",
      "contract.rs",
      "src/domain/bike_rental/rental_fleet/bicycle/registration_number/validity/contract.rs",
      {
        role: "Value Object invariant contract",
        summary: "Declares RegistrationNumberValidity and its semantic ID.",
        allowed: [
          "One #[domain_invariant] trait",
          "Its validation signature",
          "Imports",
        ],
        guarantee: "Validity remains explicit and independently testable.",
      }
    ),
    file(
      "registration-invariant-evaluate",
      "evaluate.rs",
      "src/domain/bike_rental/rental_fleet/bicycle/registration_number/validity/evaluate.rs",
      {
        role: "Value Object invariant implementation",
        summary: "Evaluates validity directly on RegistrationNumber.",
        allowed: [
          "One matching trait implementation",
          "Private helpers",
          "Imports",
        ],
        guarantee:
          "The evaluator cannot silently attach to an aggregate or entity instead.",
      }
    ),
  ]
)

const chooseRegistrationNumberFormat = directory(
  "choose-registration-number-format",
  "choose_format",
  "src/domain/bike_rental/rental_fleet/bicycle/registration_number/choose_format",
  {
    role: "Value Object policy",
    summary: "Chooses one closed Registration Number format outcome.",
    allowed: ["policy.rs", "outcome.rs", "evaluate.rs", "mod.rs"],
    guarantee:
      "The policy implementation must directly target RegistrationNumber.",
  },
  [
    file(
      "registration-policy",
      "policy.rs",
      "src/domain/bike_rental/rental_fleet/bicycle/registration_number/choose_format/policy.rs",
      {
        role: "Value Object policy contract",
        summary:
          "Declares ChooseRegistrationNumberFormatPolicy and its semantic ID.",
        allowed: [
          "One #[domain_policy] trait",
          "Its method signature",
          "Imports",
        ],
        guarantee: "The policy stays separate from its outcome vocabulary.",
      }
    ),
    file(
      "registration-outcome",
      "outcome.rs",
      "src/domain/bike_rental/rental_fleet/bicycle/registration_number/choose_format/outcome.rs",
      {
        role: "Policy outcome",
        summary: "Defines compact and segmented Registration Number formats.",
        allowed: ["One PolicyOutcome enum", "Tagged variants", "Imports"],
        guarantee: "Outcome IDs and labels remain stable and exhaustive.",
      }
    ),
    file(
      "registration-policy-evaluate",
      "evaluate.rs",
      "src/domain/bike_rental/rental_fleet/bicycle/registration_number/choose_format/evaluate.rs",
      {
        role: "Value Object policy implementation",
        summary: "Evaluates the format directly for RegistrationNumber.",
        allowed: [
          "One matching trait implementation",
          "Private helpers",
          "Imports",
        ],
        guarantee:
          "The checker binds the policy to the Value Object declared by value.rs.",
      }
    ),
  ]
)

const registrationNumber = directory(
  "registration-number",
  "registration_number",
  "src/domain/bike_rental/rental_fleet/bicycle/registration_number",
  {
    role: "Behaviorful Value Object",
    summary:
      "Keeps one semantic value and its action, invariant, and policy capabilities together.",
    allowed: [
      "value.rs",
      "Action directories",
      "Invariant directories",
      "Policy directories",
      "mod.rs",
    ],
    guarantee:
      "Value Object behavior stays nested with its semantic owner instead of being flattened beside the Entity.",
  },
  [
    file(
      "registration-value",
      "value.rs",
      "src/domain/bike_rental/rental_fleet/bicycle/registration_number/value.rs",
      {
        role: "Value Object declaration",
        summary: "Defines the opaque RegistrationNumber value and accessors.",
        allowed: [
          "One ValueObject declaration",
          "Direct inherent implementations",
          "Imports",
        ],
        guarantee:
          "The stable value anchor supplies the owner name for every child capability.",
      }
    ),
    normalizeRegistrationNumber,
    registrationNumberValidity,
    chooseRegistrationNumberFormat,
  ]
)

const eligibilityPolicy = directory(
  "eligibility-policy",
  "assess_rental_eligibility",
  "src/domain/bike_rental/rental_fleet/bicycle/assess_rental_eligibility",
  {
    role: "Entity policy capability",
    summary:
      "Evaluates rental status and bicycle condition to determine eligibility.",
    allowed: ["policy.rs", "outcome.rs", "evaluate.rs", "mod.rs"],
    guarantee:
      "Eligibility remains pure and reusable by commands and queries without duplicating lifecycle rules.",
  },
  [
    file(
      "policy",
      "policy.rs",
      "src/domain/bike_rental/rental_fleet/bicycle/assess_rental_eligibility/policy.rs",
      {
        role: "Policy contract",
        summary: "Declares the rental-eligibility policy on Bicycle.",
        allowed: [
          "One #[domain_policy] trait",
          "Its method signature",
          "Imports",
        ],
        guarantee:
          "Policy metadata is global while filesystem placement binds its implementation owner.",
      }
    ),
    file(
      "outcome",
      "outcome.rs",
      "src/domain/bike_rental/rental_fleet/bicycle/assess_rental_eligibility/outcome.rs",
      {
        role: "Policy outcome",
        summary: "Declares the possible rental-eligibility outcomes.",
        allowed: [
          "One PolicyOutcome enum",
          "Tagged variants",
          "Ordinary payload fields",
        ],
        guarantee: "The enum lets callers match every outcome exhaustively.",
      }
    ),
    file(
      "evaluate",
      "evaluate.rs",
      "src/domain/bike_rental/rental_fleet/bicycle/assess_rental_eligibility/evaluate.rs",
      {
        role: "Entity policy implementation",
        summary:
          "Implements the policy for Bicycle by composing its lifecycle and condition.",
        allowed: [
          "One matching trait implementation",
          "Pure lifecycle evaluation",
          "Bicycle condition checks",
          "Imports",
        ],
        guarantee:
          "The evaluator has no state mutation, event recording, or persistence responsibility.",
      }
    ),
  ]
)

const bicycle = directory(
  "bicycle",
  "bicycle",
  "src/domain/bike_rental/rental_fleet/bicycle",
  {
    role: "Nested Entity",
    summary:
      "Groups Bicycle identity, state concepts, lifecycle, and Entity-local behavior.",
    allowed: [
      "entity.rs and identity.rs",
      "Value Object modules",
      "Lifecycle, policy, and action directories",
    ],
    guarantee:
      "Related concepts stay nested with their owner instead of being flattened into type buckets.",
  },
  [
    file(
      "entity",
      "entity.rs",
      "src/domain/bike_rental/rental_fleet/bicycle/entity.rs",
      {
        role: "Entity declaration",
        summary:
          "Defines Bicycle state and its explicit EntityDefinition identity accessor.",
        allowed: [
          "One Entity declaration",
          "Its EntityDefinition implementation",
        ],
        guarantee:
          "Owner and identity types are compiler-checked while generic systems can read identity().",
      }
    ),
    file(
      "entity-identity",
      "identity.rs",
      "src/domain/bike_rental/rental_fleet/bicycle/identity.rs",
      {
        role: "Domain identity",
        summary: "Defines the opaque BicycleId marker type.",
        allowed: [
          "One DomainIdentity declaration",
          "Identity-specific constructors or accessors",
        ],
        guarantee:
          "Identity representation stays private to the type rather than inferred by metadata.",
      }
    ),
    bicycleCondition,
    registrationNumber,
    rentalStatus,
    eligibilityPolicy,
  ]
)

const rentBicycle = directory(
  "rent-bicycle",
  "rent_bicycle",
  "src/domain/bike_rental/rental_fleet/rent_bicycle",
  {
    role: "Aggregate action capability",
    summary:
      "Composes one complete command-to-event behavior slice without creating a large aggregate file.",
    allowed: [
      "Action and execution",
      "Command and handler",
      "Event, apply, rejection, and input companions",
    ],
    guarantee:
      "Each file has one job and execute.rs must target AggregateInstance<RentalFleetAggregate>.",
  },
  [
    file(
      "aggregate-action",
      "action.rs",
      "src/domain/bike_rental/rental_fleet/rent_bicycle/action.rs",
      {
        role: "Action contract",
        summary: "Declares the single Rent Bicycle behavior trait.",
        allowed: [
          "One #[domain_action] trait",
          "Its method signature",
          "Imports",
        ],
        guarantee:
          "The behavior contract is stable, small, and independently reviewable.",
      }
    ),
    file(
      "aggregate-execute",
      "execute.rs",
      "src/domain/bike_rental/rental_fleet/rent_bicycle/execute.rs",
      {
        role: "Aggregate action implementation",
        summary: "Evaluates state and raises the corresponding domain event.",
        allowed: [
          "One matching AggregateInstance implementation",
          "Private helpers",
          "Imports",
        ],
        guarantee:
          "The filesystem checker verifies both trait and aggregate implementor shapes.",
      }
    ),
    file(
      "command",
      "command.rs",
      "src/domain/bike_rental/rental_fleet/rent_bicycle/command.rs",
      {
        role: "Command",
        summary: "Declares a Bike Rental command with fleet and bicycle IDs.",
        allowed: ["One Command declaration", "Payload fields", "Imports"],
        guarantee:
          "The command declares its bounded context; runtime registration selects its handler.",
      }
    ),
    file(
      "handler",
      "handler.rs",
      "src/domain/bike_rental/rental_fleet/rent_bicycle/handler.rs",
      {
        role: "Command handler",
        summary: "Loads the fleet and calls its rental action.",
        allowed: ["One CommandHandler implementation", "Imports"],
        guarantee:
          "The handler declares its rejection type and loads participants through CommandExecution.",
      }
    ),
    file(
      "event",
      "event.rs",
      "src/domain/bike_rental/rental_fleet/rent_bicycle/event.rs",
      {
        role: "Domain event",
        summary: "Captures the persisted Bicycle Rented fact.",
        allowed: [
          "One DomainEvent declaration",
          "Serializable fact fields",
          "Imports",
        ],
        guarantee:
          "Membership in event_set.rs supplies aggregate-scoped stream identity.",
      }
    ),
    file(
      "apply",
      "apply.rs",
      "src/domain/bike_rental/rental_fleet/rent_bicycle/apply.rs",
      {
        role: "Event application",
        summary: "Applies BicycleRented to aggregate state.",
        allowed: ["One Apply implementation", "Imports"],
        guarantee:
          "State transition code is isolated from command evaluation and transport.",
      }
    ),
    file(
      "rejection",
      "rejection.rs",
      "src/domain/bike_rental/rental_fleet/rent_bicycle/rejection.rs",
      {
        role: "Domain rejection",
        summary:
          "Defines the stable business rejection returned by the action.",
        allowed: [
          "One DomainError declaration",
          "Serializable context fields",
          "Imports",
        ],
        guarantee: "Code and message remain stable without owner annotations.",
      }
    ),
  ]
)

const availabilityQuery = directory(
  "availability-query",
  "bicycle_availability",
  "src/domain/bike_rental/rental_fleet/bicycle_availability",
  {
    role: "Query capability",
    summary: "Pairs one read contract with its returned view type.",
    allowed: ["query.rs", "execute.rs", "output.rs", "mod.rs"],
    guarantee:
      "The Query stays owner-independent while structure binds its implementation to the aggregate root.",
  },
  [
    file(
      "query",
      "query.rs",
      "src/domain/bike_rental/rental_fleet/bicycle_availability/query.rs",
      {
        role: "Query contract",
        summary: "An ordinary trait carrying the stable query ID and label.",
        allowed: [
          "One #[domain_query] trait",
          "Its query signature",
          "Imports",
        ],
        guarantee:
          "The query is a directly callable trait on the aggregate root.",
      }
    ),
    file(
      "query-execute",
      "execute.rs",
      "src/domain/bike_rental/rental_fleet/bicycle_availability/execute.rs",
      {
        role: "Query implementation",
        summary: "Implements BicycleAvailabilityQuery for the aggregate root.",
        allowed: [
          "One matching trait implementation",
          "Private helpers",
          "Imports",
        ],
        guarantee:
          "The checker requires the direct aggregate-root implementation selected by the enclosing AggregateDefinition.",
      }
    ),
    file(
      "query-output",
      "output.rs",
      "src/domain/bike_rental/rental_fleet/bicycle_availability/output.rs",
      {
        role: "Query view",
        summary: "Defines the ordinary Rust view returned by the Query.",
        allowed: ["Ordinary output DTOs", "View-specific helpers"],
        guarantee:
          "Output types describe the query result without domain-value metadata.",
      }
    ),
  ]
)

const fleetInvariant = directory(
  "fleet-invariant",
  "fleet_consistency",
  "src/domain/bike_rental/rental_fleet/fleet_consistency",
  {
    role: "Invariant capability",
    summary: "Names and evaluates one rule that must hold for Rental Fleet.",
    allowed: ["contract.rs", "evaluate.rs", "mod.rs"],
    guarantee:
      "Stable invariant metadata and executable validation remain independently testable.",
  },
  [
    file(
      "invariant",
      "contract.rs",
      "src/domain/bike_rental/rental_fleet/fleet_consistency/contract.rs",
      {
        role: "Invariant contract",
        summary: "Declares the Fleet Consistency trait and semantic metadata.",
        allowed: [
          "One #[domain_invariant] trait",
          "Its validation signature",
          "Imports",
        ],
        guarantee:
          "The invariant has a semantic ID and an application-defined validation signature.",
      }
    ),
    file(
      "invariant-evaluate",
      "evaluate.rs",
      "src/domain/bike_rental/rental_fleet/fleet_consistency/evaluate.rs",
      {
        role: "Invariant evaluation",
        summary: "Returns an InvariantViolation when fleet consistency fails.",
        allowed: [
          "One matching trait implementation",
          "Private helpers",
          "Imports",
        ],
        guarantee:
          "Validation behavior stays isolated from its semantic declaration.",
      }
    ),
  ]
)

const rentalFleet = directory(
  "rental-fleet",
  "rental_fleet",
  "src/domain/bike_rental/rental_fleet",
  {
    role: "Aggregate boundary",
    summary:
      "Owns Rental Fleet state, event membership, nested objects, and focused capabilities.",
    allowed: [
      "Aggregate anchor files",
      "Entity directories",
      "Action, Query, Policy, Invariant, and Lifecycle capabilities",
    ],
    guarantee:
      "Aggregate relationships are explicit in Rust and visible in the filesystem.",
  },
  [
    file(
      "aggregate",
      "aggregate.rs",
      "src/domain/bike_rental/rental_fleet/aggregate.rs",
      {
        role: "Aggregate declaration",
        summary: "Pairs minimal metadata with AggregateDefinition.",
        allowed: [
          "One Aggregate declaration",
          "Its AggregateDefinition implementation",
        ],
        guarantee:
          "Context, root, and event set are compiler-checked in one focused file.",
      }
    ),
    file(
      "event-set",
      "event_set.rs",
      "src/domain/bike_rental/rental_fleet/event_set.rs",
      {
        role: "Closed event set",
        summary: "Lists the event types that belong to Rental Fleet.",
        allowed: ["One AggregateEvents enum", "One typed variant per event"],
        guarantee:
          "Only registered event types can be recorded, decoded, and replayed.",
      }
    ),
    file("root", "root.rs", "src/domain/bike_rental/rental_fleet/root.rs", {
      role: "Aggregate root Entity",
      summary: "Defines Rental Fleet state reconstructed from domain events.",
      allowed: [
        "One Entity declaration",
        "Its EntityDefinition implementation",
      ],
      guarantee:
        "Root identity is available generically without field-name metadata.",
    }),
    file(
      "aggregate-identity",
      "identity.rs",
      "src/domain/bike_rental/rental_fleet/identity.rs",
      {
        role: "Aggregate identity type",
        summary: "Defines the opaque FleetId used by the root Entity.",
        allowed: [
          "One DomainIdentity declaration",
          "Identity-specific behavior",
        ],
        guarantee:
          "The representation stays encapsulated behind a typed identity.",
      }
    ),
    file(
      "initialize",
      "initialize.rs",
      "src/domain/bike_rental/rental_fleet/initialize.rs",
      {
        role: "Aggregate initialization",
        summary: "Builds the Rental Fleet root for an empty event stream.",
        allowed: ["One Initialize implementation", "Imports"],
        guarantee:
          "The event-sourcing runtime has one deterministic initial-state path.",
      }
    ),
    file(
      "stream",
      "stream.rs",
      "src/domain/bike_rental/rental_fleet/stream.rs",
      {
        role: "Stream identity boundary",
        summary:
          "Translates the Fleet identity into its runtime stream address.",
        allowed: ["Stream construction helpers", "Imports"],
        guarantee:
          "Storage addressing remains isolated from the aggregate declaration and behavior.",
      }
    ),
    bicycle,
    rentBicycle,
    availabilityQuery,
    fleetInvariant,
  ]
)

const transferBicycle = directory(
  "transfer-bicycle",
  "transfer",
  "src/domain/bike_rental/bicycle_transfer/transfer",
  {
    role: "Domain service action",
    summary:
      "Transfers a bicycle between source and destination fleets supplied by the handler.",
    allowed: ["action.rs", "execute.rs", "mod.rs"],
    guarantee:
      "The handler loads both fleets; the domain service validates and changes them.",
  },
  [
    file(
      "transfer-bicycle-action",
      "action.rs",
      "src/domain/bike_rental/bicycle_transfer/transfer/action.rs",
      {
        role: "Domain service action contract",
        summary: "Declares BicycleTransferAction and its semantic ID.",
        allowed: [
          "One #[domain_action] trait",
          "Its method signature",
          "Imports",
        ],
        guarantee:
          "The service operation remains ordinary callable Rust with domain-shaped inputs.",
      }
    ),
    file(
      "transfer-bicycle-execute",
      "execute.rs",
      "src/domain/bike_rental/bicycle_transfer/transfer/execute.rs",
      {
        role: "Domain service action implementation",
        summary: "Implements validation and transfer for BicycleTransfer.",
        allowed: [
          "One matching trait implementation",
          "Private helpers",
          "Imports",
        ],
        guarantee:
          "The checker binds the action to the service marker declared in service.rs.",
      }
    ),
  ]
)

const bicycleTransfer = directory(
  "bicycle-transfer",
  "bicycle_transfer",
  "src/domain/bike_rental/bicycle_transfer",
  {
    role: "Domain service",
    summary: "Coordinates a bicycle transfer between two fleet aggregates.",
    allowed: [
      "service.rs",
      "command.rs, handler.rs, and rejection.rs",
      "Action directories",
      "Policy directories",
      "mod.rs",
    ],
    guarantee:
      "DomainServiceDefinition binds the service to Bike Rental; the runtime commits both fleets atomically.",
  },
  [
    file(
      "service",
      "service.rs",
      "src/domain/bike_rental/bicycle_transfer/service.rs",
      {
        role: "Domain service declaration",
        summary:
          "Pairs minimal service metadata with its bounded-context definition.",
        allowed: [
          "One DomainService declaration",
          "Its DomainServiceDefinition implementation",
        ],
        guarantee:
          "Context ownership is compiler-checked without implicit behavior attachments.",
      }
    ),
    transferBicycle,
  ]
)

const bikeRental = directory(
  "bike-rental",
  "bike_rental",
  "src/domain/bike_rental",
  {
    role: "Bounded context",
    summary:
      "Owns one language boundary and its Aggregate and Domain Service modules.",
    allowed: [
      "context.rs",
      "Aggregate directories",
      "Domain Service directories",
      "mod.rs",
    ],
    guarantee:
      "Context boundaries remain visible in paths and generated aggregate identities.",
  },
  [
    file("context", "context.rs", "src/domain/bike_rental/context.rs", {
      role: "Bounded-context declaration",
      summary: "Declares the stable Bike Rental ID and label.",
      allowed: ["One BoundedContext declaration", "Imports"],
      guarantee: "The bounded context has one semantic source of truth.",
    }),
    rentalFleet,
    bicycleTransfer,
  ]
)

export const STRUCTURE: StructureNode[] = [
  directory(
    "domain",
    "domain",
    "src/domain",
    {
      role: "Typed domain root",
      summary: "The single entry point for the model and bounded contexts.",
      allowed: [
        "model.rs",
        "Bounded-context directories",
        "A composition-only mod.rs",
      ],
      guarantee:
        "Every modeled concept starts from one predictable, reviewable root.",
    },
    [
      file("model", "model.rs", "src/domain/model.rs", {
        role: "Domain model inventory",
        summary:
          "Composes only the stable declarations Rostfrei can verify deterministically.",
        allowed: [
          "One domain_model! composition",
          "Imports needed by that composition",
        ],
        guarantee:
          "The compiled model remains explicit and free of runtime-only relationships.",
      }),
      bikeRental,
    ]
  ),
]

export const DEFAULT_SELECTED_ID = "aggregate-action"
export const DEFAULT_EXPANDED_IDS = [
  "domain",
  "bike-rental",
  "rental-fleet",
  "rent-bicycle",
]

export function findStructureNode(
  nodes: StructureNode[],
  id: string
): StructureNode | undefined {
  for (const node of nodes) {
    if (node.id === id) return node
    const child = node.children
      ? findStructureNode(node.children, id)
      : undefined
    if (child) return child
  }
  return undefined
}
