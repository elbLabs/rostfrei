// Test-only HTTP fixtures. Domain fixtures and expectations come from the actual
// bike-rental example; this module is never imported by the application bundle.
import fleet from "../../examples/bike-rental/fixtures/demo-fleet.json" with { type: "json" }
import rented from "../../examples/bike-rental/fixtures/rented-demo-fleet.json" with { type: "json" }
import empty from "../../examples/bike-rental/fixtures/no-rental-fleets.json" with { type: "json" }
import rent from "../../examples/bike-rental/tests/tracer/rent-available-bicycle.json" with { type: "json" }
import reject from "../../examples/bike-rental/tests/tracer/reject-unavailable-bicycle.json" with { type: "json" }
import returned from "../../examples/bike-rental/tests/tracer/return-rented-bicycle.json" with { type: "json" }
import add from "../../examples/bike-rental/tests/tracer/add-first-bicycle.json" with { type: "json" }
import duplicate from "../../examples/bike-rental/tests/tracer/reject-duplicate-bicycle.json" with { type: "json" }

export const SAMPLE_DEFINITIONS = Object.fromEntries(
  [rent, reject, returned, add, duplicate].map((definition) => [
    definition.id,
    { revision: "ui-fixture", definition },
  ])
)
export const SAMPLE_TESTS = Object.values(SAMPLE_DEFINITIONS).map(
  ({ definition, revision }) => ({
    id: definition.id,
    name: definition.name,
    revision,
    definitionHref: `/tests/${definition.id}`,
    runHref: `/tests/${definition.id}/runs`,
  })
)
export const SAMPLE_FIXTURE = fleet
export const SAMPLE_FIXTURES = {
  [fleet.id]: fleet,
  [rented.id]: rented,
  [empty.id]: empty,
}

const fixtureNodes = fleet.messages.map((message, index) => ({
  ...message,
  id: `fixture-event-0-${message.messageId}`,
  parentId: index
    ? `fixture-event-0-${fleet.messages[index - 1].messageId}`
    : undefined,
  context: "fixture",
  aggregateType: message.aggregate.type,
  aggregateId: message.aggregate.id,
  edgeRelationship: index ? "stream-order" : undefined,
  edgeFidelity: "exact",
}))
const identities = {
  subject: "command-rent",
  rented: "event-rented",
  started: "integration-started",
}
const expected = rent.expected.graphs[0].nodes
const idFor = (node) =>
  identities[node.key] ??
  (node.kind === "command"
    ? "command-rent"
    : node.kind === "domain-event"
      ? "event-rented"
      : "integration-started")
const ids = new Map(expected.map((node) => [node.key, idFor(node)]))
export const SAMPLE_GRAPH = [
  ...fixtureNodes,
  ...expected.map((node) => ({
    ...node,
    context: undefined,
    boundedContext: node.kind === "command" ? node.context : undefined,
    id: ids.get(node.key),
    parentId: node.parentKey
      ? ids.get(node.parentKey)
      : fixtureNodes.at(-1)?.id,
    edgeRelationship: node.parentKey ? "causation" : "context",
    edgeFidelity: "exact",
  })),
]
