import { useEffect, useRef, useState } from "react";
import type { CSSProperties } from "react";
import type { ErrorObject } from "ajv";
import Ajv2020 from "ajv/dist/2020";
import { printParseErrorCode, visit } from "jsonc-parser";
import {
  Activity,
  AlertTriangle,
  Box,
  Braces,
  Check,
  ChevronRight,
  CircleCheck,
  Clock3,
  Command,
  Copy,
  FileJson,
  GitBranch,
  Import,
  Network,
  Radio,
  RefreshCw,
  Search,
  ShieldCheck,
  Timer,
  X,
} from "lucide-react";
import definitionSchema from "../../crates/rostfrei-tracer/schema/message-series-definition-v1.schema.json";
import observationSchema from "../../crates/rostfrei-tracer/schema/observed-message-series-v1.schema.json";

type Mode = "expected" | "observed";
type NodeKind = "command" | "domain-event" | "integration-event";
type Severity = "error" | "warning";

type Aggregate = {
  type: string;
  id: string;
};

type ExpectedRejection = {
  rejected: {
    code: string;
    payload?: unknown;
  };
};

type ExpectedNode = {
  kind: NodeKind;
  key: string;
  parentKey?: string | null;
  name: string;
  schemaVersion: number;
  aggregate?: Aggregate;
  payload?: unknown;
  outcome?: "accepted" | ExpectedRejection;
};

type ExpectedGraph = {
  nodes: ExpectedNode[];
  within?: string;
  settleFor?: string;
};

type MessageSeriesDefinition = {
  within: string;
  settleFor: string;
  graphs: ExpectedGraph[];
};

type ObservedNode = {
  kind: NodeKind;
  messageId: string;
  correlationId: string;
  causationId?: string | null;
  name: string;
  schemaVersion: number;
  aggregate?: Aggregate;
  payload?: unknown;
};

type CommandOutcome = {
  responseMessageId: string;
  commandMessageId: string;
  correlationId: string;
  outcome:
    | { status: "accepted"; value?: null }
    | {
        status: "rejected";
        value: {
          classification: string;
          code: string;
          message: string;
          details?: unknown;
        };
      };
};

type ObservedMessageSeries = {
  messages: ObservedNode[];
  commandOutcomes: CommandOutcome[];
};

type ViewNode = {
  id: string;
  parentId?: string;
  kind: NodeKind;
  name: string;
  schemaVersion: number;
  ordinal: number;
  aggregate?: Aggregate;
  payload?: unknown;
  outcome?: ExpectedNode["outcome"] | CommandOutcome["outcome"];
  correlationId?: string;
  responseMessageId?: string;
  raw: ExpectedNode | ObservedNode;
};

type ContractIssue = {
  severity: Severity;
  code: string;
  message: string;
  path: string;
  nodeId?: string;
  graphIndex?: number;
};

const MAX_IMPORT_BYTES = 8 * 1024 * 1024;
const MAX_STUDIO_GRAPHS = 256;
const MAX_U32 = 4_294_967_295;
const MAX_JSON_DEPTH = 127;
const MAX_JSON_VALUES = 100_000;
const ajv = new Ajv2020({ allErrors: true, strict: false });
ajv.addFormat("uint32", {
  type: "number",
  validate: (value: number) => Number.isInteger(value) && value >= 0 && value <= MAX_U32,
});
const validateDefinitionSchema = ajv.compile(definitionSchema);
const validateObservationSchema = ajv.compile(observationSchema);
const graphIdentities = new WeakMap<ExpectedGraph, string>();
let nextGraphIdentity = 1;

const expectedSample: MessageSeriesDefinition = {
  within: "10s",
  settleFor: "500ms",
  graphs: [
    {
      settleFor: "1s",
      nodes: [
        {
          kind: "command",
          key: "rent",
          name: "rent-bicycle",
          schemaVersion: 1,
          aggregate: { type: "bike-rental/rental-fleet", id: "city-fleet" },
          payload: { bicycle_id: "bike-42", rider_id: "rider-8" },
          outcome: "accepted",
        },
        {
          kind: "domain-event",
          key: "rented",
          parentKey: "rent",
          name: "bicycle-rented",
          schemaVersion: 1,
          payload: { bicycle_id: "bike-42", rider_id: "rider-8" },
        },
        {
          kind: "integration-event",
          key: "rental-published",
          parentKey: "rented",
          name: "bicycle-rental-started",
          schemaVersion: 2,
          payload: { bicycle_id: "bike-42", station_id: "central" },
        },
        {
          kind: "command",
          key: "schedule-inspection",
          parentKey: "rented",
          name: "schedule-bicycle-inspection",
          schemaVersion: 1,
          aggregate: { type: "bike-maintenance/workshop", id: "central" },
          payload: { bicycle_id: "bike-42", due_after_rides: 25 },
          outcome: "accepted",
        },
        {
          kind: "domain-event",
          key: "inspection-scheduled",
          parentKey: "schedule-inspection",
          name: "bicycle-inspection-scheduled",
          schemaVersion: 1,
          payload: { bicycle_id: "bike-42", workshop_id: "central" },
        },
      ],
    },
    {
      within: "3s",
      nodes: [
        {
          kind: "command",
          key: "rent-unavailable",
          name: "rent-bicycle",
          schemaVersion: 1,
          aggregate: { type: "bike-rental/rental-fleet", id: "city-fleet" },
          payload: { bicycle_id: "bike-99", rider_id: "rider-8" },
          outcome: {
            rejected: {
              code: "BICYCLE_UNAVAILABLE",
              payload: { bicycle_id: "bike-99", state: "maintenance" },
            },
          },
        },
      ],
    },
  ],
};

const observedSample: ObservedMessageSeries = {
  messages: [
    {
      kind: "command",
      messageId: "cmd-01J8RNT4A2",
      correlationId: "cor-01J8RNT47Z",
      name: "rent-bicycle",
      schemaVersion: 1,
      aggregate: { type: "bike-rental/rental-fleet", id: "city-fleet" },
      payload: { bicycle_id: "bike-42", rider_id: "rider-8" },
    },
    {
      kind: "domain-event",
      messageId: "evt-01J8RNT4Q8",
      correlationId: "cor-01J8RNT47Z",
      causationId: "cmd-01J8RNT4A2",
      name: "bicycle-rented",
      schemaVersion: 1,
      payload: { bicycle_id: "bike-42", rider_id: "rider-8" },
    },
    {
      kind: "integration-event",
      messageId: "int-01J8RNT51C",
      correlationId: "cor-01J8RNT47Z",
      causationId: "evt-01J8RNT4Q8",
      name: "bicycle-rental-started",
      schemaVersion: 2,
      payload: { bicycle_id: "bike-42", station_id: "central" },
    },
    {
      kind: "command",
      messageId: "cmd-01J8RNT56P",
      correlationId: "cor-01J8RNT47Z",
      causationId: "evt-01J8RNT4Q8",
      name: "schedule-bicycle-inspection",
      schemaVersion: 1,
      aggregate: { type: "bike-maintenance/workshop", id: "central" },
      payload: { bicycle_id: "bike-42", due_after_rides: 25 },
    },
    {
      kind: "domain-event",
      messageId: "evt-01J8RNT5P4",
      correlationId: "cor-01J8RNT47Z",
      causationId: "cmd-01J8RNT56P",
      name: "bicycle-inspection-scheduled",
      schemaVersion: 1,
      payload: { bicycle_id: "bike-42", workshop_id: "central" },
    },
    {
      kind: "integration-event",
      messageId: "int-01J8RNT6B0",
      correlationId: "cor-01J8RNT47Z",
      causationId: "evt-awaiting-publish",
      name: "rental-audit-requested",
      schemaVersion: 1,
      payload: { bicycle_id: "bike-42" },
    },
  ],
  commandOutcomes: [
    {
      responseMessageId: "res-01J8RNT4KV",
      commandMessageId: "cmd-01J8RNT4A2",
      correlationId: "cor-01J8RNT47Z",
      outcome: { status: "accepted", value: null },
    },
    {
      responseMessageId: "res-01J8RNT5GY",
      commandMessageId: "cmd-01J8RNT56P",
      correlationId: "cor-01J8RNT47Z",
      outcome: { status: "accepted", value: null },
    },
  ],
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

type SerdeNumber =
  | { kind: "u64" | "i64"; value: string }
  | { kind: "f64"; value: number };

class JsonNumber {
  constructor(readonly source: string) {}
}

function isJsonNumber(value: unknown): value is JsonNumber {
  return value instanceof JsonNumber;
}

function isWithinUnsignedLimit(digits: string, limit: string) {
  return digits.length < limit.length || (digits.length === limit.length && digits <= limit);
}

function serdeNumber(value: string): SerdeNumber {
  if (!/[.eE]/.test(value)) {
    const negative = value.startsWith("-");
    const digits = negative ? value.slice(1) : value;
    if (!negative && isWithinUnsignedLimit(digits, "18446744073709551615")) {
      return { kind: "u64", value: digits };
    }
    if (
      negative &&
      digits !== "0" &&
      isWithinUnsignedLimit(digits, "9223372036854775808")
    ) {
      return { kind: "i64", value };
    }
  }

  const parsed = Number(value);
  if (!Number.isFinite(parsed)) {
    throw new Error(`JSON number \`${value.slice(0, 40)}\` is outside serde_json's range.`);
  }
  return { kind: "f64", value: parsed };
}

function jsonNumbersEqual(
  left: JsonNumber | number,
  right: JsonNumber | number,
) {
  const leftNumber = serdeNumber(
    isJsonNumber(left) ? left.source : String(left),
  );
  const rightNumber = serdeNumber(
    isJsonNumber(right) ? right.source : String(right),
  );
  return leftNumber.kind === rightNumber.kind && leftNumber.value === rightNumber.value;
}

function assertValidUnicode(value: string) {
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (code >= 0xd800 && code <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (index + 1 >= value.length || next < 0xdc00 || next > 0xdfff) {
        throw new Error("JSON strings cannot contain unpaired Unicode surrogates.");
      }
      index += 1;
    } else if (code >= 0xdc00 && code <= 0xdfff) {
      throw new Error("JSON strings cannot contain unpaired Unicode surrogates.");
    }
  }
}

type ContainerFrame =
  | { kind: "array"; value: unknown[] }
  | { kind: "object"; value: Record<string, unknown>; property?: string };

function parseJson(source: string): unknown {
  const stack: ContainerFrame[] = [];
  let result: unknown;
  let values = 0;
  let parseError = "";

  function reserveValue() {
    values += 1;
    if (values > MAX_JSON_VALUES) {
      throw new Error(
        `JSON contains more than ${MAX_JSON_VALUES.toLocaleString()} values.`,
      );
    }
  }

  function appendValue(value: unknown) {
    const parent = stack.at(-1);
    if (!parent) {
      result = value;
    } else if (parent.kind === "array") {
      parent.value.push(value);
    } else if (parent.property !== undefined) {
      Object.defineProperty(parent.value, parent.property, {
        configurable: true,
        enumerable: true,
        value,
        writable: true,
      });
      parent.property = undefined;
    }
  }

  function beginContainer(frame: ContainerFrame) {
    reserveValue();
    if (stack.length + 1 > MAX_JSON_DEPTH) {
      throw new Error(`JSON nesting exceeds ${MAX_JSON_DEPTH} levels.`);
    }
    appendValue(frame.value);
    stack.push(frame);
  }

  visit(
    source,
    {
      onObjectBegin: () => {
        beginContainer({ kind: "object", value: Object.create(null) });
      },
      onObjectProperty: (property) => {
        assertValidUnicode(property);
        const frame = stack.at(-1);
        if (frame?.kind === "object") frame.property = property;
      },
      onObjectEnd: () => {
        stack.pop();
      },
      onArrayBegin: () => {
        beginContainer({ kind: "array", value: [] });
      },
      onArrayEnd: () => {
        stack.pop();
      },
      onLiteralValue: (value, offset, length) => {
        reserveValue();
        if (typeof value === "string") {
          assertValidUnicode(value);
          appendValue(value);
        } else if (typeof value === "number") {
          const numberSource = source.slice(offset, offset + length);
          serdeNumber(numberSource);
          appendValue(new JsonNumber(numberSource));
        } else {
          appendValue(value);
        }
      },
      onError: (error, offset) => {
        if (!parseError) parseError = `${printParseErrorCode(error)} at character ${offset}`;
      },
    },
    { allowTrailingComma: false, disallowComments: true },
  );

  if (parseError) throw new SyntaxError(`Invalid JSON: ${parseError}.`);
  if (result === undefined) throw new SyntaxError("Invalid JSON: expected a value.");
  return result;
}

function stringifyJson(value: unknown, space = 0): string {
  const gap = " ".repeat(Math.min(Math.max(space, 0), 10));

  function serialize(current: unknown, depth: number): string | undefined {
    if (isJsonNumber(current)) return current.source;
    if (current === null || typeof current === "boolean" || typeof current === "string") {
      return JSON.stringify(current);
    }
    if (typeof current === "number") {
      if (!Number.isFinite(current)) throw new Error("Cannot serialize a non-finite number.");
      return JSON.stringify(current);
    }
    if (Array.isArray(current)) {
      if (current.length === 0) return "[]";
      const items = current.map((item) => serialize(item, depth + 1) ?? "null");
      if (!gap) return `[${items.join(",")}]`;
      const childIndent = gap.repeat(depth + 1);
      return `[\n${childIndent}${items.join(`,\n${childIndent}`)}\n${gap.repeat(depth)}]`;
    }
    if (isRecord(current)) {
      const entries: string[] = [];
      for (const key of Object.keys(current)) {
        const serialized = serialize(current[key], depth + 1);
        if (serialized === undefined) continue;
        const separator = gap ? ": " : ":";
        entries.push(`${JSON.stringify(key)}${separator}${serialized}`);
      }
      if (entries.length === 0) return "{}";
      if (!gap) return `{${entries.join(",")}}`;
      const childIndent = gap.repeat(depth + 1);
      return `{\n${childIndent}${entries.join(`,\n${childIndent}`)}\n${gap.repeat(depth)}}`;
    }
    return undefined;
  }

  return serialize(value, 0) ?? "null";
}

function normalizeSchemaVersions(parsed: Record<string, unknown>) {
  const collections: unknown[] = [];
  if (Array.isArray(parsed.messages)) collections.push(parsed.messages);
  if (Array.isArray(parsed.graphs)) {
    for (const graph of parsed.graphs) {
      if (isRecord(graph) && Array.isArray(graph.nodes)) collections.push(graph.nodes);
    }
  }

  for (const collection of collections) {
    if (!Array.isArray(collection)) continue;
    for (const node of collection) {
      if (!isRecord(node) || !isJsonNumber(node.schemaVersion)) continue;
      const source = node.schemaVersion.source;
      if (/^(0|[1-9]\d*)$/.test(source) && Number.isSafeInteger(Number(source))) {
        node.schemaVersion = Number(source);
      }
    }
  }
}

function schemaErrors(errors: ErrorObject[] | null | undefined) {
  return (errors ?? [])
    .slice(0, 3)
    .map((error) => `${error.instancePath || "/"} ${error.message ?? "is invalid"}`)
    .join("; ");
}

function graphIdentity(graph: ExpectedGraph) {
  const existing = graphIdentities.get(graph);
  if (existing) return existing;
  const identity = `graph-${nextGraphIdentity}`;
  nextGraphIdentity += 1;
  graphIdentities.set(graph, identity);
  return identity;
}

function jsonValuesEqual(left: unknown, right: unknown): boolean {
  const pending: Array<[unknown, unknown]> = [[left, right]];

  while (pending.length > 0) {
    const pair = pending.pop();
    if (!pair) break;
    const [leftValue, rightValue] = pair;

    if (Object.is(leftValue, rightValue)) continue;
    if (
      isJsonNumber(leftValue) ||
      isJsonNumber(rightValue) ||
      typeof leftValue === "number" ||
      typeof rightValue === "number"
    ) {
      if (
        (!isJsonNumber(leftValue) && typeof leftValue !== "number") ||
        (!isJsonNumber(rightValue) && typeof rightValue !== "number") ||
        !jsonNumbersEqual(leftValue, rightValue)
      ) {
        return false;
      }
      continue;
    }
    if (Array.isArray(leftValue) || Array.isArray(rightValue)) {
      if (
        !Array.isArray(leftValue) ||
        !Array.isArray(rightValue) ||
        leftValue.length !== rightValue.length
      ) {
        return false;
      }
      for (let index = 0; index < leftValue.length; index += 1) {
        pending.push([leftValue[index], rightValue[index]]);
      }
      continue;
    }
    if (isRecord(leftValue) || isRecord(rightValue)) {
      if (!isRecord(leftValue) || !isRecord(rightValue)) return false;
      const leftKeys = Object.keys(leftValue);
      if (leftKeys.length !== Object.keys(rightValue).length) return false;
      for (const key of leftKeys) {
        if (!Object.hasOwn(rightValue, key)) return false;
        pending.push([leftValue[key], rightValue[key]]);
      }
      continue;
    }
    return false;
  }

  return true;
}

function observedNodesEqual(left: ObservedNode, right: ObservedNode): boolean {
  return (
    left.kind === right.kind &&
    left.messageId === right.messageId &&
    left.correlationId === right.correlationId &&
    (left.causationId ?? null) === (right.causationId ?? null) &&
    left.name === right.name &&
    left.schemaVersion === right.schemaVersion &&
    jsonValuesEqual(left.aggregate, right.aggregate) &&
    jsonValuesEqual(left.payload ?? null, right.payload ?? null)
  );
}

function outcomesEqual(left: CommandOutcome, right: CommandOutcome): boolean {
  if (
    left.responseMessageId !== right.responseMessageId ||
    left.commandMessageId !== right.commandMessageId ||
    left.correlationId !== right.correlationId ||
    left.outcome.status !== right.outcome.status
  ) {
    return false;
  }
  if (left.outcome.status === "accepted") return true;
  if (right.outcome.status === "accepted") return false;
  return (
    left.outcome.value.classification === right.outcome.value.classification &&
    left.outcome.value.code === right.outcome.value.code &&
    left.outcome.value.message === right.outcome.value.message &&
    jsonValuesEqual(
      left.outcome.value.details ?? null,
      right.outcome.value.details ?? null,
    )
  );
}

function parseImport(value: string):
  | { mode: "expected"; document: MessageSeriesDefinition }
  | { mode: "observed"; document: ObservedMessageSeries } {
  const parsed = parseJson(value);
  if (!isRecord(parsed)) throw new Error("The document must be a JSON object.");
  normalizeSchemaVersions(parsed);

  if (Object.hasOwn(parsed, "graphs")) {
    if (Array.isArray(parsed.graphs) && parsed.graphs.length > MAX_STUDIO_GRAPHS) {
      throw new Error(`Studio supports up to ${MAX_STUDIO_GRAPHS} graphs in one document.`);
    }
    if (!validateDefinitionSchema(parsed)) {
      throw new Error(`Definition schema: ${schemaErrors(validateDefinitionSchema.errors)}`);
    }
    return {
      mode: "expected",
      document: parsed as MessageSeriesDefinition,
    };
  }

  if (Object.hasOwn(parsed, "messages") || Object.hasOwn(parsed, "commandOutcomes")) {
    if (!validateObservationSchema(parsed)) {
      throw new Error(`Observation schema: ${schemaErrors(validateObservationSchema.errors)}`);
    }
    return {
      mode: "observed",
      document: parsed as ObservedMessageSeries,
    };
  }

  throw new Error("Expected a MessageSeries definition or observed-series document.");
}

function expectedNodes(graph: ExpectedGraph): ViewNode[] {
  return graph.nodes.map((node, index) => ({
    id: node.key,
    parentId: node.parentKey ?? undefined,
    kind: node.kind,
    name: node.name,
    schemaVersion: node.schemaVersion,
    ordinal: index + 1,
    aggregate: node.aggregate,
    payload: node.payload,
    outcome: node.outcome,
    raw: node,
  }));
}

function observedNodes(document: ObservedMessageSeries): ViewNode[] {
  const outcomes = new Map<string, CommandOutcome>();
  for (const outcome of document.commandOutcomes) {
    if (!outcomes.has(outcome.commandMessageId)) outcomes.set(outcome.commandMessageId, outcome);
  }
  const identities = new Set<string>();
  const nodes: ViewNode[] = [];
  for (const [index, node] of document.messages.entries()) {
    if (identities.has(node.messageId)) continue;
    identities.add(node.messageId);
    const outcome = outcomes.get(node.messageId);
    nodes.push({
      id: node.messageId,
      parentId: node.causationId ?? undefined,
      kind: node.kind,
      name: node.name,
      schemaVersion: node.schemaVersion,
      ordinal: index + 1,
      aggregate: node.aggregate,
      payload: node.payload,
      outcome: outcome?.outcome,
      correlationId: node.correlationId,
      responseMessageId: outcome?.responseMessageId,
      raw: node,
    });
  }
  return nodes;
}

function cycleIssues(nodes: ViewNode[], pathPrefix: string): ContractIssue[] {
  const byId = new Map(nodes.map((node) => [node.id, node]));
  const complete = new Set<string>();
  const reported = new Set<string>();
  const issues: ContractIssue[] = [];

  for (const start of nodes) {
    if (complete.has(start.id)) continue;
    const path: string[] = [];
    const positions = new Map<string, number>();
    let current: ViewNode | undefined = start;
    while (current && !complete.has(current.id)) {
      const cycleStart = positions.get(current.id);
      if (cycleStart !== undefined) {
        const members = path.slice(cycleStart);
        const signature = JSON.stringify([...members].sort());
        if (!reported.has(signature)) {
          reported.add(signature);
          issues.push({
            severity: "error",
            code: "causation-cycle",
            message: `Causation cycle: ${members.join(" -> ")}`,
            path: `${pathPrefix}/${current.ordinal - 1}`,
            nodeId: current.id,
          });
        }
        break;
      }
      positions.set(current.id, path.length);
      path.push(current.id);
      current = current.parentId ? byId.get(current.parentId) : undefined;
    }
    for (const id of path) complete.add(id);
  }
  return issues;
}

function diagnoseExpected(graph: ExpectedGraph, graphIndex: number): ContractIssue[] {
  const nodes = expectedNodes(graph);
  const issues: ContractIssue[] = [];
  const firstById = new Map<string, ViewNode>();
  const prefix = `/graphs/${graphIndex}/nodes`;

  for (const node of nodes) {
    const path = `${prefix}/${node.ordinal - 1}`;
    if (!node.id.trim()) {
      issues.push({ severity: "error", code: "empty-node-key", message: "Node key is empty.", path, nodeId: node.id });
    }
    if (!node.name.trim()) {
      issues.push({ severity: "error", code: "empty-message-name", message: "Message name is empty.", path, nodeId: node.id });
    }
    if (node.schemaVersion < 1 || node.schemaVersion > MAX_U32) {
      issues.push({ severity: "error", code: "invalid-schema-version", message: "Schema version must be positive.", path, nodeId: node.id });
    }
    if (firstById.has(node.id)) {
      issues.push({ severity: "error", code: "duplicate-node-key", message: `Node key \`${node.id}\` is duplicated.`, path, nodeId: node.id });
    } else {
      firstById.set(node.id, node);
    }
    if (node.kind === "command" && node.outcome === undefined) {
      issues.push({ severity: "error", code: "missing-outcome", message: "Command requires an expected outcome.", path, nodeId: node.id });
    }
  }

  for (const node of nodes) {
    if (node.parentId && !firstById.has(node.parentId)) {
      issues.push({
        severity: "error",
        code: "unresolved-parent-key",
        message: `Parent key \`${node.parentId}\` does not identify a node in this graph.`,
        path: `${prefix}/${node.ordinal - 1}/parentKey`,
        nodeId: node.id,
      });
    }
  }

  const roots = nodes.filter((node) => !node.parentId);
  if (roots.length !== 1) {
    issues.push({ severity: "error", code: "invalid-root-count", message: `Expected one root, found ${roots.length}.`, path: prefix });
  } else {
    const root = roots[0];
    if (root.kind !== "command") {
      issues.push({ severity: "error", code: "root-not-command", message: "The graph root must be a command.", path: `${prefix}/${root.ordinal - 1}/kind`, nodeId: root.id });
    }
    if (root.kind === "command" && root.payload == null) {
      issues.push({ severity: "error", code: "missing-root-command-payload", message: "The root command requires its complete payload.", path: `${prefix}/${root.ordinal - 1}/payload`, nodeId: root.id });
    }
  }

  return [...issues, ...cycleIssues(nodes, prefix)].map((issue) => ({ ...issue, graphIndex }));
}

function diagnoseObserved(document: ObservedMessageSeries): ContractIssue[] {
  const nodes = observedNodes(document);
  const issues: ContractIssue[] = [];
  const firstRawById = new Map<string, ObservedNode>();

  for (const [index, node] of document.messages.entries()) {
    const existing = firstRawById.get(node.messageId);
    if (existing && !observedNodesEqual(existing, node)) {
      issues.push({
        severity: "error",
        code: "message-identity-conflict",
        message: `Message identity \`${node.messageId}\` has conflicting content.`,
        path: `/messages/${index}`,
        nodeId: node.messageId,
      });
    }
    if (!existing) firstRawById.set(node.messageId, node);
  }

  const firstById = new Map(nodes.map((node) => [node.id, node]));
  for (const node of nodes) {
    if (node.schemaVersion < 1 || node.schemaVersion > MAX_U32) {
      issues.push({
        severity: "error",
        code: "invalid-schema-version",
        message: "Schema version must be a positive uint32.",
        path: `/messages/${node.ordinal - 1}/schemaVersion`,
        nodeId: node.id,
      });
    }
    if (!node.parentId) continue;
    const parent = firstById.get(node.parentId);
    if (!parent) {
      issues.push({
        severity: "warning",
        code: "unresolved-parent",
        message: `Waiting for parent message \`${node.parentId}\`.`,
        path: `/messages/${node.ordinal - 1}/causationId`,
        nodeId: node.id,
      });
    } else if (parent.correlationId !== node.correlationId) {
      issues.push({
        severity: "warning",
        code: "cross-correlation",
        message: "Parent and child belong to different correlations.",
        path: `/messages/${node.ordinal - 1}/causationId`,
        nodeId: node.id,
      });
    }
  }

  const commandAttachments = new Map<string, CommandOutcome>();
  const responseAttachments = new Map<string, CommandOutcome>();
  for (const [index, outcome] of document.commandOutcomes.entries()) {
    const command = firstById.get(outcome.commandMessageId);
    if (!command) {
      issues.push({ severity: "warning", code: "missing-command", message: `Outcome is waiting for command \`${outcome.commandMessageId}\`.`, path: `/commandOutcomes/${index}/commandMessageId` });
    } else if (command.kind !== "command") {
      issues.push({ severity: "error", code: "outcome-target-not-command", message: "Outcome target is not a command.", path: `/commandOutcomes/${index}/commandMessageId`, nodeId: command.id });
    } else if (command.correlationId !== outcome.correlationId) {
      issues.push({ severity: "warning", code: "outcome-cross-correlation", message: "Outcome and command belong to different correlations.", path: `/commandOutcomes/${index}/correlationId`, nodeId: command.id });
    }
    const commandAttachment = commandAttachments.get(outcome.commandMessageId);
    const responseAttachment = responseAttachments.get(outcome.responseMessageId);
    let inserted = false;
    if (commandAttachment) {
      const same = outcomesEqual(commandAttachment, outcome);
      if (!same) {
        issues.push({
          severity: "error",
          code: "outcome-identity-conflict",
          message: "Command identity is attached to conflicting outcomes.",
          path: `/commandOutcomes/${index}`,
          nodeId: outcome.commandMessageId,
        });
      }
    } else if (responseAttachment) {
      issues.push({ severity: "error", code: "outcome-identity-conflict", message: "Response identity is attached to more than one command.", path: `/commandOutcomes/${index}`, nodeId: outcome.commandMessageId });
    } else {
      inserted = true;
    }
    if (inserted) {
      commandAttachments.set(outcome.commandMessageId, outcome);
      responseAttachments.set(outcome.responseMessageId, outcome);
    }

    if (outcome.outcome.status === "rejected") {
      const message = outcome.outcome.value.message;
      const tooLong = new TextEncoder().encode(message).length > 1024;
      let hasControlCharacter = false;
      if (!tooLong) {
        for (const character of message) {
          if (/\p{Cc}/u.test(character)) {
            hasControlCharacter = true;
            break;
          }
        }
      }
      if (tooLong || hasControlCharacter) {
        issues.push({
          severity: "error",
          code: "invalid-rejection-message",
          message: "Rejection message contains a control character or exceeds 1024 UTF-8 bytes.",
          path: `/commandOutcomes/${index}/outcome/value/message`,
          nodeId: outcome.commandMessageId,
        });
      }
    }
  }

  return [...issues, ...cycleIssues(nodes, "/messages")];
}

function kindLabel(kind: NodeKind) {
  if (kind === "domain-event") return "Domain event";
  if (kind === "integration-event") return "Integration event";
  return "Command";
}

function outcomeLabel(outcome: ViewNode["outcome"]) {
  if (outcome === "accepted") return "Accepted";
  if (isRecord(outcome) && "status" in outcome && outcome.status === "accepted") return "Accepted";
  if (isRecord(outcome) && ("rejected" in outcome || ("status" in outcome && outcome.status === "rejected"))) return "Rejected";
  return undefined;
}

function nodeIcon(kind: NodeKind) {
  if (kind === "domain-event") return <Box aria-hidden="true" />;
  if (kind === "integration-event") return <Radio aria-hidden="true" />;
  return <Command aria-hidden="true" />;
}

function JsonBlock({ value }: { value: unknown }) {
  return <pre className="json-block">{stringifyJson(value ?? null, 2)}</pre>;
}

function NodeCard({
  node,
  selected,
  matched,
  issues,
  onSelect,
}: {
  node: ViewNode;
  selected: boolean;
  matched: boolean;
  issues: ContractIssue[];
  onSelect: (id: string) => void;
}) {
  const outcome = outcomeLabel(node.outcome);
  const hasError = issues.some((issue) => issue.severity === "error");
  const hasWarning = issues.some((issue) => issue.severity === "warning");
  return (
    <button
      className={`node-card kind-${node.kind}${selected ? " is-selected" : ""}${matched ? "" : " is-dimmed"}${hasError ? " has-error" : ""}${hasWarning ? " has-warning" : ""}`}
      type="button"
      onClick={() => onSelect(node.id)}
      aria-pressed={selected}
    >
      <span className="node-card-topline">
        <span className="node-kind-icon">{nodeIcon(node.kind)}</span>
        <span className="node-kind">{kindLabel(node.kind)}</span>
        <span className="node-ordinal">{String(node.ordinal).padStart(2, "0")}</span>
      </span>
      <strong>{node.name}</strong>
      <span className="node-id">{node.id}</span>
      <span className="node-card-footer">
        <span>v{node.schemaVersion}</span>
        {outcome ? (
          <span className={`outcome outcome-${outcome.toLowerCase()}`}>{outcome}</span>
        ) : (
          <span>{node.correlationId ? "Observed" : "Expected"}</span>
        )}
      </span>
    </button>
  );
}

type FlatTreeRow =
  | { type: "node"; node: ViewNode; depth: number; key: string; continuations: number[] }
  | { type: "cycle"; nodeId: string; depth: number; key: string; continuations: number[] };

function flattenTree(
  starts: ViewNode[],
  childrenByParent: Map<string, ViewNode[]>,
): FlatTreeRow[] {
  type Frame =
    | { type: "enter"; node: ViewNode; depth: number; key: string; continuations: number[] }
    | { type: "exit"; nodeId: string };
  const rows: FlatTreeRow[] = [];
  const ancestors = new Set<string>();
  let nextKey = 1;
  const pending: Frame[] = [...starts]
    .reverse()
    .map((node) => ({ type: "enter", node, depth: 0, key: `tree-row-${nextKey++}`, continuations: [] }));

  while (pending.length > 0) {
    const frame = pending.pop();
    if (!frame) break;
    if (frame.type === "exit") {
      ancestors.delete(frame.nodeId);
      continue;
    }
    if (ancestors.has(frame.node.id)) {
      rows.push({ type: "cycle", nodeId: frame.node.id, depth: frame.depth, key: `${frame.key}-cycle`, continuations: frame.continuations });
      continue;
    }

    rows.push({ type: "node", node: frame.node, depth: frame.depth, key: frame.key, continuations: frame.continuations });
    ancestors.add(frame.node.id);
    pending.push({ type: "exit", nodeId: frame.node.id });
    const children = childrenByParent.get(frame.node.id) ?? [];
    for (let index = children.length - 1; index >= 0; index -= 1) {
      const childDepth = frame.depth + 1;
      const continuationDepth = Math.min(childDepth, 8);
      const continuations =
        index < children.length - 1 && !frame.continuations.includes(continuationDepth)
          ? [...frame.continuations, continuationDepth]
          : frame.continuations;
      pending.push({
        type: "enter",
        node: children[index],
        depth: childDepth,
        key: `tree-row-${nextKey++}`,
        continuations,
      });
    }
  }

  return rows;
}

function FlatTree({
  starts,
  childrenByParent,
  selectedId,
  query,
  issues,
  onSelect,
}: {
  starts: ViewNode[];
  childrenByParent: Map<string, ViewNode[]>;
  selectedId: string;
  query: string;
  issues: ContractIssue[];
  onSelect: (id: string) => void;
}) {
  const rows = flattenTree(starts, childrenByParent);

  return (
    <ul className="tree-rows">
      {rows.map((row) => {
        const visualDepth = Math.min(row.depth, 8);
        const style = { "--tree-depth": visualDepth } as CSSProperties;
        if (row.type === "cycle") {
          return (
            <li
              className="tree-row cycle-reference"
              key={row.key}
              style={style}
            >
              {row.continuations.map((depth) => (
                <span className="tree-continuation" key={depth} style={{ "--rail-depth": Math.min(depth, 8) } as CSSProperties} aria-hidden="true" />
              ))}
              <span className="sr-only">Causal level {row.depth + 1}.</span>
              <span className="cycle-stop"><AlertTriangle /> Loops to {row.nodeId}</span>
            </li>
          );
        }
        const nodeIssues = issues.filter((issue) => issue.nodeId === row.node.id);
        const haystack = `${row.node.name} ${row.node.id} ${row.node.kind}`.toLowerCase();
        const matched = !query || haystack.includes(query);
        return (
          <li
            className={`tree-row${row.depth === 0 ? " is-root" : ""}`}
            key={row.key}
            style={style}
          >
            {row.continuations.map((depth) => (
              <span className="tree-continuation" key={depth} style={{ "--rail-depth": Math.min(depth, 8) } as CSSProperties} aria-hidden="true" />
            ))}
            <span className="sr-only">
              Causal level {row.depth + 1}. {row.node.parentId ? `Parent ${row.node.parentId}.` : "Causal root."}
            </span>
            <NodeCard
              node={row.node}
              selected={selectedId === row.node.id}
              matched={matched}
              issues={nodeIssues}
              onSelect={onSelect}
            />
          </li>
        );
      })}
    </ul>
  );
}

function CausalTree({
  nodes,
  selectedId,
  query,
  issues,
  onSelect,
}: {
  nodes: ViewNode[];
  selectedId: string;
  query: string;
  issues: ContractIssue[];
  onSelect: (id: string) => void;
}) {
  const byId = new Map(nodes.map((node) => [node.id, node]));
  const childrenByParent = new Map<string, ViewNode[]>();
  const roots: ViewNode[] = [];
  const unresolved: ViewNode[] = [];

  for (const node of nodes) {
    if (!node.parentId) {
      roots.push(node);
    } else if (!byId.has(node.parentId)) {
      unresolved.push(node);
    } else {
      const children = childrenByParent.get(node.parentId) ?? [];
      children.push(node);
      childrenByParent.set(node.parentId, children);
    }
  }

  const covered = new Set<string>();
  function collectComponent(start: ViewNode) {
    const component = new Set<string>();
    const pending = [start];
    while (pending.length > 0) {
      const current = pending.pop();
      if (!current || component.has(current.id)) continue;
      component.add(current.id);
      if (current.parentId) {
        const parent = byId.get(current.parentId);
        if (parent) pending.push(parent);
      }
      pending.push(...(childrenByParent.get(current.id) ?? []));
    }
    return component;
  }
  function markComponent(start: ViewNode) {
    for (const id of collectComponent(start)) covered.add(id);
  }
  for (const node of [...roots, ...unresolved]) markComponent(node);
  const detached: ViewNode[] = [];
  for (const node of nodes) {
    if (covered.has(node.id)) continue;
    const component = collectComponent(node);
    for (const id of component) covered.add(id);
    const cycleId = issues.find(
      (issue) => issue.code === "causation-cycle" && issue.nodeId && component.has(issue.nodeId),
    )?.nodeId;
    detached.push((cycleId && byId.get(cycleId)) || node);
  }

  return (
    <div className="causal-layout">
      <section className="causal-tree" aria-label="Causal message graph">
        <FlatTree starts={roots} childrenByParent={childrenByParent} selectedId={selectedId} query={query} issues={issues} onSelect={onSelect} />
      </section>
      {unresolved.length > 0 ? (
        <section className="unresolved-zone" aria-label="Unresolved messages">
          <div className="unresolved-heading">
            <AlertTriangle aria-hidden="true" />
            <span>Awaiting parent</span>
            <span>{unresolved.length}</span>
          </div>
          <div className="causal-tree unresolved-cards">
            <FlatTree starts={unresolved} childrenByParent={childrenByParent} selectedId={selectedId} query={query} issues={issues} onSelect={onSelect} />
          </div>
        </section>
      ) : null}
      {detached.length > 0 ? (
        <section className="detached-zone" aria-label="Cyclic or detached message components">
          <div className="unresolved-heading">
            <AlertTriangle aria-hidden="true" />
            <span>Cyclic or detached components</span>
            <span>{detached.length}</span>
          </div>
          <div className="causal-tree detached-components">
            <FlatTree starts={detached} childrenByParent={childrenByParent} selectedId={selectedId} query={query} issues={issues} onSelect={onSelect} />
          </div>
        </section>
      ) : null}
    </div>
  );
}

function App() {
  const [mode, setMode] = useState<Mode>("expected");
  const [definition, setDefinition] = useState(expectedSample);
  const [observation, setObservation] = useState(observedSample);
  const [activeGraph, setActiveGraph] = useState(0);
  const [selectedId, setSelectedId] = useState(expectedSample.graphs[0]?.nodes[0]?.key ?? "");
  const [query, setQuery] = useState("");
  const [importOpen, setImportOpen] = useState(false);
  const [importDraft, setImportDraft] = useState(stringifyJson(expectedSample, 2));
  const [importError, setImportError] = useState("");
  const [copied, setCopied] = useState(false);
  const fileInput = useRef<HTMLInputElement>(null);
  const importDialog = useRef<HTMLDialogElement>(null);
  const importTextarea = useRef<HTMLTextAreaElement>(null);

  const graph = definition.graphs[activeGraph] ?? definition.graphs[0];
  const observationNodes = observedNodes(observation);
  const nodes = mode === "expected" && graph ? expectedNodes(graph) : observationNodes;
  const issues = mode === "expected"
    ? definition.graphs.flatMap((item, index) => diagnoseExpected(item, index))
    : diagnoseObserved(observation);
  const displayIssues = mode === "expected"
    ? issues.filter((issue) => issue.graphIndex === activeGraph)
    : issues;
  const selected = nodes.find((node) => node.id === selectedId) ?? nodes[0];
  const totalNodes = mode === "expected"
    ? definition.graphs.reduce((total, item) => total + item.nodes.length, 0)
    : observationNodes.length;
  const totalCommands = mode === "expected"
    ? definition.graphs.reduce((total, item) => total + item.nodes.filter((node) => node.kind === "command").length, 0)
    : observationNodes.filter((node) => node.kind === "command").length;
  const errors = issues.filter((issue) => issue.severity === "error");
  const warnings = issues.filter((issue) => issue.severity === "warning");
  const effectiveWithin = graph?.within ?? definition.within;
  const effectiveSettle = graph?.settleFor ?? definition.settleFor;
  const activeDocument = mode === "expected" ? definition : observation;
  const correlations = new Set(observation.messages.map((node) => node.correlationId));

  useEffect(() => {
    const dialog = importDialog.current;
    if (!dialog) return;
    if (importOpen && !dialog.open) {
      dialog.showModal();
      window.requestAnimationFrame(() => importTextarea.current?.focus());
    } else if (!importOpen && dialog.open) {
      dialog.close();
    }
  }, [importOpen]);

  function switchMode(nextMode: Mode) {
    setMode(nextMode);
    setActiveGraph(0);
    setQuery("");
    const firstId = nextMode === "expected"
      ? definition.graphs[0]?.nodes[0]?.key
      : observation.messages[0]?.messageId;
    setSelectedId(firstId ?? "");
  }

  function selectGraph(index: number) {
    setActiveGraph(index);
    setSelectedId(definition.graphs[index]?.nodes[0]?.key ?? "");
  }

  function openImport() {
    setImportDraft(stringifyJson(activeDocument, 2));
    setImportError("");
    setImportOpen(true);
  }

  function applyImport() {
    try {
      if (new TextEncoder().encode(importDraft).length > MAX_IMPORT_BYTES) {
        throw new Error("Studio supports JSON documents up to 8 MiB.");
      }
      const imported = parseImport(importDraft);
      if (imported.mode === "expected") {
        setDefinition(imported.document);
        setMode("expected");
        setActiveGraph(0);
        setSelectedId(imported.document.graphs[0]?.nodes[0]?.key ?? "");
      } else {
        setObservation(imported.document);
        setMode("observed");
        setSelectedId(imported.document.messages[0]?.messageId ?? "");
      }
      setQuery("");
      setImportOpen(false);
      setImportError("");
    } catch (error) {
      setImportError(error instanceof Error ? error.message : "The document could not be loaded.");
    }
  }

  async function loadFile(file: File | undefined) {
    if (!file) return;
    if (file.size > MAX_IMPORT_BYTES) {
      setImportDraft("");
      setImportError("Studio supports JSON documents up to 8 MiB.");
      setImportOpen(true);
      if (fileInput.current) fileInput.current.value = "";
      return;
    }
    try {
      const bytes = await file.arrayBuffer();
      setImportDraft(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
      setImportError("");
      setImportOpen(true);
    } catch {
      setImportDraft("");
      setImportError("The selected file could not be read as valid UTF-8.");
      setImportOpen(true);
    } finally {
      if (fileInput.current) fileInput.current.value = "";
    }
  }

  function resetSample() {
    if (mode === "expected") {
      setDefinition(expectedSample);
      setActiveGraph(0);
      setSelectedId(expectedSample.graphs[0]?.nodes[0]?.key ?? "");
    } else {
      setObservation(observedSample);
      setSelectedId(observedSample.messages[0]?.messageId ?? "");
    }
    setQuery("");
  }

  async function copySelected() {
    if (!selected) return;
    try {
      await navigator.clipboard.writeText(stringifyJson(selected.raw, 2));
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1400);
    } catch {
      setCopied(false);
    }
  }

  function selectIssue(issue: ContractIssue) {
    if (issue.graphIndex !== undefined) setActiveGraph(issue.graphIndex);
    if (issue.nodeId) setSelectedId(issue.nodeId);
  }

  return (
    <div className="app-shell">
      <header className="topbar">
        <div className="brand">
          <span className="brand-mark" aria-hidden="true"><GitBranch /></span>
          <span>
            <strong>ROSTFREI</strong>
            <small>Message Series</small>
          </span>
        </div>
        <fieldset className="mode-switch">
          <legend className="sr-only">Contract view</legend>
          <button className={mode === "expected" ? "is-active" : ""} type="button" aria-pressed={mode === "expected"} onClick={() => switchMode("expected")}>
            <ShieldCheck /> Expected
          </button>
          <button className={mode === "observed" ? "is-active" : ""} type="button" aria-pressed={mode === "observed"} onClick={() => switchMode("observed")}>
            <Activity /> Observed
          </button>
        </fieldset>
        <div className="header-actions">
          <span className={`contract-state${errors.length ? " is-error" : warnings.length ? " is-partial" : ""}`} role="status" aria-live="polite" aria-atomic="true">
            {errors.length ? <AlertTriangle /> : warnings.length ? <Clock3 /> : <CircleCheck />}
            {errors.length ? `${errors.length} invalid` : warnings.length ? "Partial series" : "Contract valid"}
          </span>
          <input
            ref={fileInput}
            className="file-input"
            type="file"
            accept="application/json,.json"
            onChange={(event) => void loadFile(event.target.files?.[0])}
          />
          <button className="button button-secondary" type="button" onClick={() => fileInput.current?.click()}>
            <Import /> Import
          </button>
          <button className="button button-primary" type="button" onClick={openImport}>
            <Braces /> Edit JSON
          </button>
        </div>
      </header>

      <main className="workspace">
        <aside className="panel series-panel">
          <div className="panel-heading">
            <span className="eyebrow">Contract source</span>
            <h1>{mode === "expected" ? "Expected flow" : "Observed flow"}</h1>
            <p>{mode === "expected" ? "Ordered assertions grouped into independently rooted graphs." : "Insertion order retained while causality resolves independently."}</p>
          </div>

          {mode === "expected" ? (
            <nav className="graph-list" aria-label="Message graphs">
              {definition.graphs.map((item, index) => {
                const root = item.nodes.find((node) => !node.parentKey);
                const graphIssues = diagnoseExpected(item, index);
                return (
                  <button
                    key={graphIdentity(item)}
                    className={activeGraph === index ? "graph-item is-active" : "graph-item"}
                    type="button"
                    aria-pressed={activeGraph === index}
                    onClick={() => selectGraph(index)}
                  >
                    <span className="graph-index">{String(index + 1).padStart(2, "0")}</span>
                    <span className="graph-copy">
                      <strong>{root?.name ?? "Unrooted graph"}</strong>
                      <small>{item.nodes.length} messages</small>
                    </span>
                    {graphIssues.length ? <AlertTriangle className="graph-alert" /> : <ChevronRight />}
                  </button>
                );
              })}
            </nav>
          ) : (
            <div className="correlation-card">
              <span className="correlation-signal"><Network /></span>
              <span>
                <small>{correlations.size === 1 ? "Correlation" : "Observed series"}</small>
                <strong>{correlations.size === 1 ? observation.messages[0]?.correlationId : `${correlations.size} correlations`}</strong>
              </span>
              <span className="live-pulse">local</span>
            </div>
          )}

          <section className="metrics" aria-label="Series summary">
            <div><strong>{mode === "expected" ? definition.graphs.length : 1}</strong><span>{mode === "expected" ? "graphs" : "series"}</span></div>
            <div><strong>{totalNodes}</strong><span>messages</span></div>
            <div><strong>{totalCommands}</strong><span>commands</span></div>
            <div><strong>{issues.length}</strong><span>issues</span></div>
          </section>

          <section className="legend">
            <span className="eyebrow">Message kinds</span>
            <div><span className="legend-swatch command-swatch"><Command /></span><span>Command</span></div>
            <div><span className="legend-swatch domain-swatch"><Box /></span><span>Domain event</span></div>
            <div><span className="legend-swatch integration-swatch"><Radio /></span><span>Integration event</span></div>
          </section>

          <button className="reset-button" type="button" onClick={resetSample}>
            <RefreshCw /> Restore sample
          </button>
        </aside>

        <section className="panel graph-panel">
          <div className="graph-toolbar">
            <div>
              <span className="eyebrow">{mode === "expected" ? `Graph ${String(activeGraph + 1).padStart(2, "0")}` : "Correlation topology"}</span>
              <h2>{mode === "expected" ? (graph?.nodes.find((node) => !node.parentKey)?.name ?? "Message graph") : "Observed causal topology"}</h2>
            </div>
            <div className="timing-row">
              {mode === "expected" ? (
                <>
                  <span><Timer /> within <strong>{effectiveWithin}</strong>{graph?.within ? <i>override</i> : null}</span>
                  <span><Clock3 /> settle <strong>{effectiveSettle}</strong>{graph?.settleFor ? <i>override</i> : null}</span>
                </>
              ) : (
                <span><Activity /> arrival order <strong>preserved</strong></span>
              )}
            </div>
            <label className="search-box">
              <Search aria-hidden="true" />
              <span className="sr-only">Find a message</span>
              <input value={query} onChange={(event) => setQuery(event.target.value.toLowerCase())} placeholder="Find message" />
              {query ? <button type="button" onClick={() => setQuery("")} aria-label="Clear search"><X /></button> : null}
            </label>
          </div>

          <div className="graph-stage">
            <div className="stage-ruler">
              <span>causal root</span>
              <span>source ordinal remains visible</span>
              <span>downstream effects</span>
            </div>
            {nodes.length ? (
              <CausalTree nodes={nodes} selectedId={selected?.id ?? ""} query={query} issues={displayIssues} onSelect={setSelectedId} />
            ) : (
              <div className="empty-state"><FileJson /><strong>No messages</strong><span>Import a document containing at least one node.</span></div>
            )}
          </div>

          {issues.length ? (
            <div className="issue-strip">
              <div className="issue-strip-heading">
                <AlertTriangle />
                <span>{errors.length ? "Contract diagnostics" : "Partial observation"}</span>
                <strong>{issues.length}</strong>
              </div>
              <div className="issue-list">
                {issues.map((issue) => (
                  <button key={`${issue.code}-${issue.path}-${issue.nodeId ?? "series"}`} type="button" onClick={() => selectIssue(issue)}>
                    <span className={`issue-dot ${issue.severity}`} />
                    <span><strong>{issue.code}</strong><small>{issue.message}</small></span>
                    <code>{issue.path}</code>
                  </button>
                ))}
              </div>
            </div>
          ) : (
            <div className="valid-strip"><CircleCheck /><span>The full document satisfies its schema and causal contract.</span><code>{mode === "expected" ? "message-series-definition-v1" : "observed-message-series-v1"}</code></div>
          )}
        </section>

        <aside className="panel inspector-panel">
          <div className="inspector-heading">
            <div>
              <span className="eyebrow">Inspector</span>
              <h2>{selected?.name ?? "No selection"}</h2>
            </div>
            <button className="icon-button" type="button" onClick={() => void copySelected()} disabled={!selected} aria-label="Copy selected node JSON">
              {copied ? <Check /> : <Copy />}
            </button>
          </div>

          {selected ? (
            <>
              <div className={`inspector-kind kind-${selected.kind}`}>
                <span>{nodeIcon(selected.kind)}</span>
                <span><small>Message kind</small><strong>{kindLabel(selected.kind)}</strong></span>
                <span className="ordinal-large">{String(selected.ordinal).padStart(2, "0")}</span>
              </div>

              <dl className="detail-list">
                <div><dt>{mode === "expected" ? "Symbolic key" : "Message ID"}</dt><dd>{selected.id}</dd></div>
                <div><dt>{mode === "expected" ? "Parent key" : "Causation ID"}</dt><dd>{selected.parentId ?? "Causal root"}</dd></div>
                {selected.correlationId ? <div><dt>Correlation ID</dt><dd>{selected.correlationId}</dd></div> : null}
                <div><dt>Schema</dt><dd>{selected.name} · v{selected.schemaVersion}</dd></div>
                {selected.aggregate ? <div><dt>Aggregate</dt><dd>{selected.aggregate.type}<br />{selected.aggregate.id}</dd></div> : null}
                {selected.responseMessageId ? <div><dt>Response ID</dt><dd>{selected.responseMessageId}</dd></div> : null}
              </dl>

              {selected.outcome ? (
                <section className="inspector-section">
                  <div className="section-title"><span>Command outcome</span><span className={`outcome outcome-${outcomeLabel(selected.outcome)?.toLowerCase()}`}>{outcomeLabel(selected.outcome)}</span></div>
                  <JsonBlock value={selected.outcome} />
                </section>
              ) : null}

              <section className="inspector-section payload-section">
                <div className="section-title"><span>Payload</span><code>JSON</code></div>
                <JsonBlock value={selected.payload} />
              </section>
            </>
          ) : (
            <div className="empty-inspector"><Network /><span>Select a message to inspect its contract.</span></div>
          )}
        </aside>
      </main>

      <dialog ref={importDialog} className="import-dialog" aria-labelledby="import-title" onClose={() => setImportOpen(false)}>
          <section className="import-modal">
            <div className="modal-heading">
              <div><span className="eyebrow">Local contract source</span><h2 id="import-title">Load MessageSeries JSON</h2></div>
              <button className="icon-button" type="button" onClick={() => setImportOpen(false)} aria-label="Close import dialog"><X /></button>
            </div>
            <p>Paste a definition or observed-series document. Schema and causal checks run locally, and imported documents are not sent to a service.</p>
            <textarea ref={importTextarea} value={importDraft} onChange={(event) => { setImportDraft(event.target.value); setImportError(""); }} spellCheck={false} aria-label="MessageSeries JSON" aria-invalid={Boolean(importError)} aria-describedby={importError ? "import-error" : undefined} />
            {importError ? <div id="import-error" className="import-error" role="alert"><AlertTriangle />{importError}</div> : null}
            <div className="modal-footer">
              <span><FileJson /> JSON document</span>
              <div>
                <button className="button button-secondary" type="button" onClick={() => setImportOpen(false)}>Cancel</button>
                <button className="button button-primary" type="button" onClick={applyImport}>Load document</button>
              </div>
            </div>
          </section>
      </dialog>
    </div>
  );
}

export default App;
