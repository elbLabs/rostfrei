import { useEffect, useRef, useState } from "react";
import type { FormEvent } from "react";
import {
  Activity,
  Beaker,
  Braces,
  Cable,
  Check,
  CircleCheck,
  CircleX,
  Copy,
  Database,
  Eye,
  EyeOff,
  Play,
  Radio,
  Rocket,
  RotateCcw,
  Send,
  Server,
  Settings2,
  ShieldCheck,
  Sparkles,
} from "lucide-react";
import {
  fetchAggregateInstances,
  fetchCatalog,
  fetchCommandInputs,
  resetTestScenario,
  streamCorrelationEvents,
  submitOperation,
  waitForOperation,
  type AggregateInstance,
  type CatalogAggregate,
  type CatalogCommand,
  type CatalogContext,
  type CommandInputDocument,
  type CorrelationCommandResultEvent,
  type CorrelationDomainEvent,
  type CorrelationEvent,
  type CorrelationIntegrationEvent,
  type TracerCatalog,
  type OperationInput,
  type OperationMode,
  type OperationSnapshot,
} from "./api";
import { Badge } from "./components/ui/badge";
import { Button } from "./components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "./components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "./components/ui/dialog";
import { Input } from "./components/ui/input";
import { Label } from "./components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "./components/ui/select";
import { Tabs, TabsList, TabsTrigger } from "./components/ui/tabs";
import { Textarea } from "./components/ui/textarea";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "./components/ui/tooltip";

type RunState = "idle" | "submitting" | "running" | "accepted" | "rejected" | "failed" | "indeterminate";
type ConnectionState = "loading" | "ready" | "error";
type InstanceState = "idle" | "loading" | "ready" | "manual";
type SelectedOutcome =
  | "command"
  | "published"
  | "responded"
  | `predicted-${number}`
  | `correlation-${number}`;

type PublicationEvidence = {
  type: "command.published";
  commandMessageId?: string;
  duplicate?: boolean;
};

type ResponseEvidence = {
  type: "command.responded";
  responseMessageId: string;
};

type SubmittedRequest = {
  mode: OperationMode;
  aggregateId: string;
  command: string;
  schemaVersion: number;
  payload: Record<string, unknown>;
};

function JsonView({ value }: { value: unknown }) {
  const json = JSON.stringify(value, null, 2) ?? "null";
  const highlighted = json
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/("(?:\\.|[^"\\])*")(?=\s*:)/g, '<span class="json-key">$1</span>')
    .replace(/:\s*("(?:\\.|[^"\\])*")/g, ': <span class="json-string">$1</span>')
    .replace(/\b(true|false|null)\b/g, '<span class="json-literal">$1</span>')
    .replace(/\b(-?\d+(?:\.\d+)?)\b/g, '<span class="json-number">$1</span>');
  return <code dangerouslySetInnerHTML={{ __html: highlighted }} />;
}

function payloadRecord(template: unknown): Record<string, unknown> {
  if (template && typeof template === "object" && !Array.isArray(template)) {
    return { ...(template as Record<string, unknown>) };
  }
  return {};
}

function payloadText(template: unknown) {
  return JSON.stringify(payloadRecord(template), null, 2);
}

function parsePayload(value: string): { payload: Record<string, unknown> | null; error: string } {
  try {
    const parsed = JSON.parse(value) as unknown;
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      return { payload: null, error: "The command payload must be a JSON object." };
    }
    return { payload: parsed as Record<string, unknown>, error: "" };
  } catch (caught) {
    return {
      payload: null,
      error: caught instanceof Error ? caught.message : "The command payload is not valid JSON.",
    };
  }
}

function optionValue(value: unknown) {
  return typeof value === "string" ? value : (JSON.stringify(value) ?? "null");
}

function latestVersion(command: CatalogCommand | undefined) {
  return command?.versions.at(-1);
}

function scalarName(scalar: string | { representation?: string } | undefined) {
  return typeof scalar === "string" ? scalar : scalar?.representation;
}

function isNumericScalar(scalar: string | undefined) {
  return scalar !== undefined && [
    "f32", "f64", "i8", "i16", "i32", "i64", "i128", "isize",
    "u8", "u16", "u32", "u64", "u128", "usize",
  ].includes(scalar);
}

function modeName(mode: OperationMode) {
  if (mode === "test") return "Isolated test";
  if (mode === "dispatch") return "Production dispatch";
  return "Simulation preview";
}

function App() {
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [showToken, setShowToken] = useState(false);
  const [showDispatchToken, setShowDispatchToken] = useState(false);
  const [baseUrl, setBaseUrl] = useState("");
  const [bearerToken, setBearerToken] = useState("");
  const [dispatchToken, setDispatchToken] = useState("");
  const [connectionState, setConnectionState] = useState<ConnectionState>("loading");
  const [connectionError, setConnectionError] = useState("");
  const [catalog, setCatalog] = useState<TracerCatalog | null>(null);
  const [contextId, setContextId] = useState("");
  const [aggregateId, setAggregateId] = useState("");
  const [commandId, setCommandId] = useState("");
  const [schemaVersion, setSchemaVersion] = useState(0);
  const [instances, setInstances] = useState<AggregateInstance[]>([]);
  const [targetId, setTargetId] = useState("");
  const [instanceState, setInstanceState] = useState<InstanceState>("idle");
  const [payloadJson, setPayloadJson] = useState("{}");
  const [commandInputs, setCommandInputs] = useState<CommandInputDocument | null>(null);
  const [inputsLoading, setInputsLoading] = useState(false);
  const [inputsError, setInputsError] = useState("");
  const [runState, setRunState] = useState<RunState>("idle");
  const [operation, setOperation] = useState<OperationSnapshot | null>(null);
  const [submittedRequest, setSubmittedRequest] = useState<SubmittedRequest | null>(null);
  const [correlationEvents, setCorrelationEvents] = useState<CorrelationEvent[]>([]);
  const [selectedOutcome, setSelectedOutcome] = useState<SelectedOutcome>("command");
  const [streamError, setStreamError] = useState("");
  const [streamActive, setStreamActive] = useState(false);
  const [mode, setMode] = useState<OperationMode>("test");
  const [operationMode, setOperationMode] = useState<OperationMode>("test");
  const [durationMs, setDurationMs] = useState<number | null>(null);
  const [resetting, setResetting] = useState(false);
  const [scenarioRevision, setScenarioRevision] = useState(0);
  const [error, setError] = useState("");
  const [copied, setCopied] = useState(false);
  const operationAbortRef = useRef<AbortController | null>(null);
  const streamAbortRef = useRef<AbortController | null>(null);
  const discoveryAbortRef = useRef<AbortController | null>(null);
  const inputsAbortRef = useRef<AbortController | null>(null);

  useEffect(() => {
    void loadConnection("", "", true);
    return () => {
      discoveryAbortRef.current?.abort();
      inputsAbortRef.current?.abort();
      operationAbortRef.current?.abort();
      streamAbortRef.current?.abort();
    };
  }, []);

  const contexts = catalog?.contexts ?? [];
  const selectedContext = contexts.find((context) => context.id === contextId);
  const selectedAggregate = selectedContext?.aggregates.find((aggregate) => aggregate.id === aggregateId);
  const selectedCommand = selectedAggregate?.commands.find((command) => command.id === commandId);
  const selectedVersion = selectedCommand?.versions.find((version) => version.schemaVersion === schemaVersion);
  const operationHref = selectedVersion
    ? {
        simulate: selectedVersion.simulateHrefTemplate,
        test: selectedVersion.testHrefTemplate,
        dispatch: selectedVersion.dispatchHrefTemplate,
      }[mode]
    : undefined;
  const busy = runState === "submitting" || runState === "running";
  const connected = connectionState === "ready" && Boolean(catalog);
  const parsedPayload = parsePayload(payloadJson);
  const commandFields = selectedVersion?.fields ?? [];
  const commandInputComplete = parsedPayload.payload !== null && commandFields.every((field) => {
    const value = parsedPayload.payload?.[field.name];
    return value !== undefined && value !== null && value !== "";
  });
  const instance = instances.find((item) => item.aggregateId === targetId);
  const result = operation?.result;
  const predictedEvents = result?.decision === "accepted" ? result.predictedEvents : [];
  const correlationCommand = correlationEvents.find((event) => event.type === "command");
  const businessEvents = correlationEvents.filter((event): event is CorrelationDomainEvent | CorrelationIntegrationEvent =>
    event.type === "integration-event" ||
    (event.type === "domain-event" && submittedRequest?.mode !== "simulate")
  );
  const correlationResult = [...correlationEvents]
    .reverse()
    .find((event): event is CorrelationCommandResultEvent => event.type === "command-result");
  const publicationEvidence: PublicationEvidence | undefined = result?.published === true
    ? {
        type: "command.published",
        commandMessageId: result.commandMessageId,
        duplicate: result.duplicate,
      }
    : operation?.failure?.commandMessageId
      ? {
          type: "command.published",
          commandMessageId: operation.failure.commandMessageId,
          duplicate: operation.failure.duplicate ?? false,
        }
      : undefined;
  const responseEvidence: ResponseEvidence | undefined = result?.responseMessageId
    ? { type: "command.responded", responseMessageId: result.responseMessageId }
    : undefined;
  const selectedCorrelationEvent = selectedOutcome === "command"
    ? undefined
    : correlationEvents.find((event) => selectedOutcome === `correlation-${event.id}`);
  const selectedPredictedEvent = selectedOutcome.startsWith("predicted-")
    ? predictedEvents.find((event) => selectedOutcome === `predicted-${event.ordinal}`)
    : undefined;
  const selectedTransportEvidence = selectedOutcome === "published"
    ? publicationEvidence
    : selectedOutcome === "responded"
      ? responseEvidence
      : undefined;
  const selectedDomainEvent = selectedCorrelationEvent?.type === "domain-event"
    ? selectedCorrelationEvent
    : undefined;
  const selectedIntegrationEvent = selectedCorrelationEvent?.type === "integration-event"
    ? selectedCorrelationEvent
    : undefined;
  const selectedCorrelationCommand = selectedCorrelationEvent?.type === "command"
    ? selectedCorrelationEvent
    : undefined;
  const selectedCommandResult = selectedCorrelationEvent?.type === "command-result"
    ? selectedCorrelationEvent
    : undefined;

  useEffect(() => {
    inputsAbortRef.current?.abort();
    setCommandInputs(null);
    setInputsError("");
    setInputsLoading(false);
    if (!connected || !selectedVersion || !targetId) return;

    const controller = new AbortController();
    inputsAbortRef.current = controller;
    setInputsLoading(true);
    void fetchCommandInputs(
      baseUrl,
      bearerToken,
      selectedVersion.inputsHrefTemplate,
      targetId,
      controller.signal,
    )
      .then((document) => {
        setCommandInputs(document);
        setPayloadJson((current) => {
          const parsed = parsePayload(current).payload ?? payloadRecord(selectedVersion.payloadTemplate);
          let changed = false;
          for (const field of document.fields) {
            if ((parsed[field.name] === "" || parsed[field.name] == null) && field.options[0]) {
              parsed[field.name] = field.options[0].value;
              changed = true;
            }
          }
          return changed ? JSON.stringify(parsed, null, 2) : current;
        });
      })
      .catch((caught: unknown) => {
        if (!controller.signal.aborted) {
          setInputsError(caught instanceof Error ? caught.message : String(caught));
        }
      })
      .finally(() => {
        if (!controller.signal.aborted) setInputsLoading(false);
      });
    return () => controller.abort();
  }, [baseUrl, bearerToken, connected, scenarioRevision, selectedVersion, targetId]);

  useEffect(() => {
    if (mode === "test" && !selectedVersion?.testHrefTemplate) setMode("simulate");
    if (mode === "dispatch" && !selectedVersion?.dispatchHrefTemplate) {
      setMode(selectedVersion?.testHrefTemplate ? "test" : "simulate");
    }
  }, [mode, selectedVersion]);

  function clearOperation() {
    operationAbortRef.current?.abort();
    streamAbortRef.current?.abort();
    setOperation(null);
    setSubmittedRequest(null);
    setCorrelationEvents([]);
    setSelectedOutcome("command");
    setStreamError("");
    setStreamActive(false);
    setDurationMs(null);
    setRunState("idle");
    setError("");
  }

  function applyCommand(command: CatalogCommand) {
    const version = latestVersion(command);
    setCommandId(command.id);
    setSchemaVersion(version?.schemaVersion ?? 0);
    setPayloadJson(payloadText(version?.payloadTemplate));
    setMode(version?.testHrefTemplate ? "test" : "simulate");
    clearOperation();
  }

  function setPayloadField(name: string, value: unknown) {
    const current = parsePayload(payloadJson).payload ?? payloadRecord(selectedVersion?.payloadTemplate);
    setPayloadJson(JSON.stringify({ ...current, [name]: value }, null, 2));
  }

  async function loadInstances(
    aggregate: CatalogAggregate,
    origin = baseUrl,
    token = bearerToken,
    preferredTarget = targetId,
  ) {
    discoveryAbortRef.current?.abort();
    const controller = new AbortController();
    discoveryAbortRef.current = controller;
    setInstanceState("loading");
    try {
      const discovered = await fetchAggregateInstances(
        origin,
        token,
        aggregate.instancesHref,
        controller.signal,
      );
      setInstances(discovered.items);
      setTargetId(preferredTarget || discovered.items[0]?.aggregateId || "");
      setInstanceState(discovered.items.length ? "ready" : "manual");
    } catch (caught) {
      if (controller.signal.aborted) return;
      setInstances([]);
      setInstanceState("manual");
      setConnectionError(
        `The API catalog loaded, but instances could not be listed: ${caught instanceof Error ? caught.message : String(caught)}`,
      );
    }
  }

  async function applyAggregate(context: CatalogContext, aggregate: CatalogAggregate) {
    const command = aggregate.commands[0];
    setContextId(context.id);
    setAggregateId(aggregate.id);
    if (command) applyCommand(command);
    else {
      setCommandId("");
      setSchemaVersion(0);
      setPayloadJson("{}");
      clearOperation();
    }
    await loadInstances(aggregate, baseUrl, bearerToken, "");
  }

  async function loadConnection(origin: string, token: string, openOnError: boolean) {
    discoveryAbortRef.current?.abort();
    clearOperation();
    const controller = new AbortController();
    discoveryAbortRef.current = controller;
    setConnectionState("loading");
    setConnectionError("");
    try {
      const discovered = await fetchCatalog(origin, token, controller.signal);
      const context = discovered.contexts[0];
      const aggregate = context?.aggregates[0];
      const command = aggregate?.commands[0];
      const version = latestVersion(command);
      if (!context || !aggregate || !command || !version) {
        throw new Error("Tracer did not advertise any executable commands");
      }
      const discoveredInstances = await fetchAggregateInstances(
        origin,
        token,
        aggregate.instancesHref,
        controller.signal,
      ).catch((caught: unknown) => {
        if (controller.signal.aborted) throw caught;
        setConnectionError(
          `Connected, but aggregate instances are not discoverable: ${caught instanceof Error ? caught.message : String(caught)}`,
        );
        return { items: [] };
      });
      setCatalog(discovered);
      setContextId(context.id);
      setAggregateId(aggregate.id);
      setCommandId(command.id);
      setSchemaVersion(version.schemaVersion);
      setPayloadJson(payloadText(version.payloadTemplate));
      setMode(version.testHrefTemplate ? "test" : "simulate");
      setInstances(discoveredInstances.items);
      setTargetId(discoveredInstances.items[0]?.aggregateId ?? "");
      setInstanceState(discoveredInstances.items.length ? "ready" : "manual");
      setConnectionState("ready");
      setSettingsOpen(false);
      clearOperation();
    } catch (caught) {
      if (controller.signal.aborted) return;
      setCatalog(null);
      setConnectionState("error");
      setConnectionError(caught instanceof Error ? caught.message : String(caught));
      if (openOnError) setSettingsOpen(true);
    }
  }

  async function connect(event: FormEvent) {
    event.preventDefault();
    if (baseUrl && !bearerToken) {
      setConnectionError("Remote Tracer instances require a bearer capability from their operator.");
      return;
    }
    await loadConnection(baseUrl, bearerToken, true);
  }

  async function runCommand(event: FormEvent) {
    event.preventDefault();
    setError("");
    if (!connected || !selectedVersion || !selectedCommand) {
      setSettingsOpen(true);
      setError("Connect to Tracer and load its command catalog first.");
      return;
    }
    if (!targetId) {
      setError("Select or enter an aggregate instance before running the command.");
      return;
    }
    if (!operationHref) {
      setError(`Tracer does not advertise ${mode} for this command.`);
      return;
    }
    if (!parsedPayload.payload) {
      setError(parsedPayload.error);
      return;
    }
    if (mode === "dispatch" && baseUrl && !dispatchToken) {
      setSettingsOpen(true);
      setError("Remote production execution requires the separate dispatch bearer capability.");
      return;
    }
    if (mode === "dispatch" && !window.confirm(
      `Run ${selectedCommand.label} against production aggregate ${targetId}?`,
    )) {
      return;
    }

    operationAbortRef.current?.abort();
    streamAbortRef.current?.abort();
    const controller = new AbortController();
    const streamController = new AbortController();
    operationAbortRef.current = controller;
    streamAbortRef.current = streamController;
    const startedAt = performance.now();
    const request: SubmittedRequest = {
      mode,
      aggregateId: targetId,
      command: selectedCommand.id,
      schemaVersion: selectedVersion.schemaVersion,
      payload: parsedPayload.payload,
    };
    setOperation(null);
    setSubmittedRequest(request);
    setCorrelationEvents([]);
    setSelectedOutcome("command");
    setStreamError("");
    setStreamActive(false);
    setDurationMs(null);
    setRunState("submitting");
    setOperationMode(mode);

    const input: OperationInput = {
      hrefTemplate: operationHref,
      aggregateId: targetId,
      schemaVersion: selectedVersion.schemaVersion,
      payload: parsedPayload.payload,
    };
    const operationToken = mode === "dispatch" ? dispatchToken : bearerToken;
    let correlationStarted = false;
    try {
      const submitted = await submitOperation(
        baseUrl,
        operationToken,
        mode,
        input,
        controller.signal,
      );
      correlationStarted = true;
      setStreamActive(true);
      void streamCorrelationEvents(
        baseUrl,
        operationToken,
        submitted.correlationId,
        streamController.signal,
        (correlationEvent) => {
          if (streamController.signal.aborted) return;
          setCorrelationEvents((current) => current.some((item) =>
            item.id === correlationEvent.id && item.correlationId === correlationEvent.correlationId
          )
            ? current
            : [...current, correlationEvent].sort((left, right) => left.id - right.id));
        },
      ).catch((caught: unknown) => {
        if (!streamController.signal.aborted) {
          setStreamActive(false);
          setStreamError(caught instanceof Error ? caught.message : String(caught));
        }
      });
      const completed = await waitForOperation(
        baseUrl,
        operationToken,
        submitted,
        controller.signal,
        (current) => {
          setOperation(current);
          if (current.status === "queued" || current.status === "running") setRunState("running");
        },
      );
      setOperation(completed);
      setDurationMs(Math.max(0, Math.round(performance.now() - startedAt)));
      if (completed.status === "indeterminate") setRunState("indeterminate");
      else if (completed.status === "failed") setRunState("failed");
      else if (completed.result?.decision === "accepted") setRunState("accepted");
      else if (completed.result?.decision === "rejected") setRunState("rejected");
      else setRunState("failed");

      if (mode === "test" && selectedAggregate) {
        await loadInstances(selectedAggregate, baseUrl, bearerToken, targetId);
        setScenarioRevision((revision) => revision + 1);
      }
    } catch (caught) {
      if (!correlationStarted) {
        streamController.abort();
        setStreamActive(false);
      }
      if (controller.signal.aborted) return;
      setDurationMs(Math.max(0, Math.round(performance.now() - startedAt)));
      setError(caught instanceof Error ? caught.message : String(caught));
      setRunState("failed");
    }
  }

  async function resetScenario() {
    const resetHref = catalog?.testScenario?.resetHref;
    if (!resetHref || !selectedAggregate || busy || resetting) return;
    setError("");
    streamAbortRef.current?.abort();
    setStreamActive(false);
    setResetting(true);
    const controller = new AbortController();
    try {
      await resetTestScenario(baseUrl, bearerToken, resetHref, controller.signal);
      clearOperation();
      await loadInstances(selectedAggregate, baseUrl, bearerToken, targetId);
      setScenarioRevision((revision) => revision + 1);
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : String(caught));
    } finally {
      setResetting(false);
    }
  }

  async function copyInspection() {
    const value = selectedCorrelationEvent ?? selectedPredictedEvent ?? selectedTransportEvidence ?? (operation
      ? {
          request: submittedRequest,
          status: operation.status,
          result: operation.result,
          failure: operation.failure,
          correlation: {
            correlationId: operation.correlationId,
            streamStatus: streamActive ? "active" : streamError ? "disconnected" : "stopped",
            commandResult: correlationResult,
            observedEvents: correlationEvents,
          },
        }
      : submittedRequest);
    if (!value) return;
    await navigator.clipboard.writeText(JSON.stringify(value, null, 2));
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1200);
  }

  const statusLabel = {
    idle: "Idle",
    submitting: "Submitting",
    running: "Running",
    accepted: "Accepted",
    rejected: "Rejected",
    failed: "Failed",
    indeterminate: "Outcome unknown",
  }[runState];
  const connectionLabel = {
    loading: "Loading catalog",
    ready: "Tracer connected",
    error: "Connection failed",
  }[connectionState];
  const modeCopy = {
    test: {
      title: "Transported test command",
      detail: "Publishes through the isolated test transport and waits for a durable application response.",
    },
    dispatch: {
      title: "Transported production command",
      detail: "Publishes to production using the separate dispatch capability and waits for a durable application response.",
    },
    simulate: {
      title: "Read-only preview",
      detail: "Predicts a decision from isolated test history without appending anything.",
    },
  }[mode];
  const resultStreamVersion = result?.baseStreamVersion;
  const appendEvidenceLabel = result?.appended === true
    ? "Confirmed"
    : result?.appended === false
      ? "Not appended"
      : "Not reported";
  const publishedEvidenceLabel = result?.published === true
    || operation?.failure?.commandMessageId
    ? "Yes"
    : result?.published === false
      ? "No"
      : "Not reported";
  const acceptedResultCopy = submittedRequest?.mode === "simulate"
    ? result?.appended === true
      ? "The operation reports that domain events were appended."
      : result?.appended === false
        ? "Domain events were predicted only; nothing was appended."
        : "The preview completed, but append evidence was not reported."
    : streamActive
      ? "The command was published and durably answered. The correlation stream remains active for asynchronous application effects."
      : "The command was published and durably answered. Event effects are observed through the application pipeline rather than predicted by Tracer.";
  return (
    <TooltipProvider delayDuration={250}>
      <main className="app-shell">
        <div className="ambient ambient-one" />
        <div className="ambient ambient-two" />

        <header className="app-header metal-surface">
          <div className="brand-lockup">
            <div className="brand-mark" aria-hidden="true"><span>R</span></div>
            <div><div className="brand-name">rostfrei</div><div className="brand-product">Studio</div></div>
          </div>
          <div className="environment-pill" aria-label="Tracer connection state">
            <span className={`live-orb ${connected ? "is-live" : ""}`} />
            <span>{connectionLabel}</span>
            {selectedContext && <><span className="environment-divider" /><strong>{selectedContext.label}</strong></>}
          </div>
          <div className="header-actions">
            <Tooltip>
              <TooltipTrigger asChild>
                <Button variant="ghost" size="icon" type="button" aria-label="Clear operation" onClick={clearOperation}>
                  <RotateCcw size={17} />
                </Button>
              </TooltipTrigger>
              <TooltipContent>Clear operation</TooltipContent>
            </Tooltip>
            <Dialog open={settingsOpen} onOpenChange={setSettingsOpen}>
              <DialogTrigger asChild>
                <Button variant="outline" type="button"><Settings2 size={16} /><span className="connection-label">Connection</span></Button>
              </DialogTrigger>
              <DialogContent>
                <DialogHeader>
                  <div className="dialog-icon"><Server size={20} /></div>
                  <div>
                    <DialogTitle>Tracer connection</DialogTitle>
                    <DialogDescription>Local development connects automatically. Configure capabilities here for a remote Tracer instance.</DialogDescription>
                  </div>
                </DialogHeader>
                <form onSubmit={connect}>
                  <div className="dialog-fields">
                    <div className="field-group">
                      <Label htmlFor="api-origin">API origin</Label>
                      <div className="input-with-icon">
                        <Server size={16} />
                        <Input id="api-origin" value={baseUrl} onChange={(event) => setBaseUrl(event.target.value)} placeholder="Same origin" />
                      </div>
                      <p className="field-help">Leave empty to use the authenticated local development proxy.</p>
                    </div>
                    <div className="field-group">
                      <Label htmlFor="bearer-token">Test capability for remote API</Label>
                      <div className="input-with-icon token-input">
                        <ShieldCheck size={16} />
                        <Input id="bearer-token" type={showToken ? "text" : "password"} value={bearerToken} onChange={(event) => setBearerToken(event.target.value)} placeholder="Enter capability" autoComplete="off" />
                        <Button variant="ghost" size="icon" type="button" onClick={() => setShowToken((show) => !show)} aria-label={`${showToken ? "Hide" : "Show"} bearer capability`}>
                          {showToken ? <EyeOff size={16} /> : <Eye size={16} />}
                        </Button>
                      </div>
                      <p className="field-help">Used for discovery, isolated tests, reset, and simulation. Kept only in this tab.</p>
                    </div>
                    <div className="field-group">
                      <Label htmlFor="dispatch-token">Production capability for remote API</Label>
                      <div className="input-with-icon token-input">
                        <Rocket size={16} />
                        <Input id="dispatch-token" type={showDispatchToken ? "text" : "password"} value={dispatchToken} onChange={(event) => setDispatchToken(event.target.value)} placeholder="Required only for production" autoComplete="off" />
                        <Button variant="ghost" size="icon" type="button" onClick={() => setShowDispatchToken((show) => !show)} aria-label={`${showDispatchToken ? "Hide" : "Show"} dispatch capability`}>
                          {showDispatchToken ? <EyeOff size={16} /> : <Eye size={16} />}
                        </Button>
                      </div>
                      <p className="field-help">Kept separate so test access cannot execute production commands.</p>
                    </div>
                    {connectionError && <div className="form-error"><CircleX size={16} /><span>{connectionError}</span></div>}
                  </div>
                  <DialogFooter>
                    <Button variant="steel" type="submit" disabled={connectionState === "loading"}>
                      {connectionState === "loading" ? <Activity size={16} /> : <Cable size={16} />}
                      {connectionState === "loading" ? "Loading catalog" : "Connect"}
                    </Button>
                  </DialogFooter>
                </form>
              </DialogContent>
            </Dialog>
          </div>
        </header>

        <div className="workspace">
          <Card className="composer-panel">
            <CardHeader>
              <div>
                <span className="eyebrow">Runtime catalog</span>
                <CardTitle>{selectedCommand?.label ?? "No command loaded"}</CardTitle>
                <CardDescription>
                  {selectedAggregate && selectedContext
                    ? `${selectedContext.label} / ${selectedAggregate.label}`
                    : "Connect to discover executable commands."}
                </CardDescription>
              </div>
              {selectedVersion && <Badge variant="accent"><Sparkles size={13} /> v{selectedVersion.schemaVersion}</Badge>}
            </CardHeader>
            <CardContent>
              {!connected ? (
                <div className="connection-empty">
                  <div>{connectionState === "loading" ? <Activity size={25} /> : <Cable size={25} />}</div>
                  <h3>{connectionState === "loading" ? "Connecting automatically" : "Connection required"}</h3>
                  <p>{connectionState === "loading" ? "Loading commands and aggregate instances from local Tracer." : "Automatic connection failed. Open connection settings for a remote API or custom capability."}</p>
                  {connectionState !== "loading" && <Button variant="steel" type="button" onClick={() => setSettingsOpen(true)}><Settings2 size={16} /> Open connection</Button>}
                </div>
              ) : (
                <form
                  onSubmit={runCommand}
                  onKeyDown={(event) => {
                    if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
                      event.preventDefault();
                      event.currentTarget.requestSubmit();
                    }
                  }}
                >
                  <div className={`mode-section mode-${mode}`}>
                    <div className="mode-heading">
                      <div><Label>Workflow</Label><span>Transport actions and secondary preview advertised by Tracer</span></div>
                      {catalog?.testScenario && (
                        <Button variant="outline" size="sm" type="button" onClick={resetScenario} disabled={busy || resetting}>
                          <RotateCcw size={14} /> {resetting ? "Resetting" : "Reset test data"}
                        </Button>
                      )}
                    </div>
                    <Tabs value={mode} onValueChange={(value) => { setMode(value as OperationMode); clearOperation(); }}>
                      <TabsList className="mode-tabs" aria-label="Execution environment">
                        <TabsTrigger className="mode-primary" value="test" disabled={!selectedVersion?.testHrefTemplate}><Beaker size={14} /> Test</TabsTrigger>
                        <TabsTrigger className="mode-primary" value="dispatch" disabled={!selectedVersion?.dispatchHrefTemplate}><Rocket size={14} /> Dispatch</TabsTrigger>
                        <TabsTrigger className="mode-secondary" value="simulate"><Play size={13} /> Simulate</TabsTrigger>
                      </TabsList>
                    </Tabs>
                  </div>

                  <div className="catalog-source">
                    <span><Radio size={14} /> Tracer catalog v{catalog?.catalogVersion}</span>
                    <strong>{selectedAggregate?.aggregateType}</strong>
                  </div>
                  <div className="field-grid two-up">
                    <div className="field-group">
                      <Label>Context</Label>
                      <Select
                        value={contextId}
                        onValueChange={(value) => {
                          const context = contexts.find((item) => item.id === value);
                          const aggregate = context?.aggregates[0];
                          if (context && aggregate) void applyAggregate(context, aggregate);
                        }}
                      >
                        <SelectTrigger><SelectValue /></SelectTrigger>
                        <SelectContent>{contexts.map((context) => <SelectItem key={context.id} value={context.id}>{context.label}</SelectItem>)}</SelectContent>
                      </Select>
                    </div>
                    <div className="field-group">
                      <Label>Aggregate</Label>
                      <Select
                        value={aggregateId}
                        onValueChange={(value) => {
                          const aggregate = selectedContext?.aggregates.find((item) => item.id === value);
                          if (selectedContext && aggregate) void applyAggregate(selectedContext, aggregate);
                        }}
                      >
                        <SelectTrigger><SelectValue /></SelectTrigger>
                        <SelectContent>{selectedContext?.aggregates.map((aggregate) => <SelectItem key={aggregate.id} value={aggregate.id}>{aggregate.label}</SelectItem>)}</SelectContent>
                      </Select>
                    </div>
                  </div>

                  <div className="field-group">
                    <Label htmlFor="aggregate-instance">Aggregate instance</Label>
                    <div className="input-with-icon">
                      <Database size={16} />
                      <Input
                        id="aggregate-instance"
                        list="aggregate-instances"
                        value={targetId}
                        onChange={(event) => setTargetId(event.target.value)}
                        placeholder={instanceState === "loading" ? "Loading instances..." : "Select or enter aggregate ID"}
                        disabled={instanceState === "loading"}
                      />
                      <datalist id="aggregate-instances">
                        {instances.map((item) => <option key={item.aggregateId} value={item.aggregateId}>stream v{item.streamVersion}</option>)}
                      </datalist>
                    </div>
                    <p className="field-help">{instance ? `Test discovery currently reports stream v${instance.streamVersion}.` : "You can enter an ID not present in test discovery, including a production-only aggregate."}</p>
                  </div>

                  <div className="field-group">
                    <Label>Command</Label>
                    <div className="command-select-wrap">
                      <span className="command-glyph"><Send size={14} /></span>
                      <Select
                        value={commandId}
                        onValueChange={(value) => {
                          const command = selectedAggregate?.commands.find((item) => item.id === value);
                          if (command) applyCommand(command);
                        }}
                      >
                        <SelectTrigger><SelectValue /></SelectTrigger>
                        <SelectContent>{selectedAggregate?.commands.map((command) => <SelectItem key={command.id} value={command.id}>{command.label}</SelectItem>)}</SelectContent>
                      </Select>
                    </div>
                  </div>

                  <div className="payload-section">
                    <div className="payload-heading">
                      <div><Label>Command input</Label><span className="payload-hint">Schema and runtime choices supplied by the API</span></div>
                      <div className="schema-field">
                        <Label>Schema</Label>
                        <Select
                          value={String(schemaVersion)}
                          onValueChange={(value) => {
                            const version = selectedCommand?.versions.find((item) => item.schemaVersion === Number(value));
                            if (!version) return;
                            setSchemaVersion(version.schemaVersion);
                            setPayloadJson(payloadText(version.payloadTemplate));
                            setMode(version.testHrefTemplate ? "test" : "simulate");
                            clearOperation();
                          }}
                        >
                          <SelectTrigger><SelectValue /></SelectTrigger>
                          <SelectContent>{selectedCommand?.versions.map((version) => <SelectItem key={version.schemaVersion} value={String(version.schemaVersion)}>v{version.schemaVersion}</SelectItem>)}</SelectContent>
                        </Select>
                      </div>
                    </div>
                    <div className="generated-inputs">
                      {inputsLoading && <div className="input-loading"><Activity size={16} /> Loading valid choices from {targetId}</div>}
                      {!inputsLoading && commandFields.length === 0 && <div className="input-empty"><Check size={16} /> This command has no payload fields.</div>}
                      {!inputsLoading && commandFields.map((field) => {
                        const discovered = commandInputs?.fields.find((item) => item.name === field.name);
                        const options = discovered?.options ?? [];
                        const currentValue = parsedPayload.payload?.[field.name];
                        const selectedOption = options.find((option) => optionValue(option.value) === optionValue(currentValue));
                        const scalar = scalarName(field.value.scalar);
                        return (
                          <div className="field-group generated-field" key={field.name}>
                            <Label>{discovered?.label ?? field.name}</Label>
                            {options.length ? (
                              <Select
                                value={optionValue(currentValue)}
                                onValueChange={(value) => {
                                  const option = options.find((item) => optionValue(item.value) === value);
                                  if (option) setPayloadField(field.name, option.value);
                                }}
                              >
                                <SelectTrigger><SelectValue placeholder={`Select ${discovered?.label ?? field.name}`} /></SelectTrigger>
                                <SelectContent>{options.map((option) => <SelectItem key={optionValue(option.value)} value={optionValue(option.value)}>{option.label}</SelectItem>)}</SelectContent>
                              </Select>
                            ) : scalar === "bool" ? (
                              <Select value={String(currentValue)} onValueChange={(value) => setPayloadField(field.name, value === "true")}>
                                <SelectTrigger><SelectValue /></SelectTrigger>
                                <SelectContent><SelectItem value="true">true</SelectItem><SelectItem value="false">false</SelectItem></SelectContent>
                              </Select>
                            ) : field.value.kind === "scalar" && isNumericScalar(scalar) ? (
                              <Input type="number" value={typeof currentValue === "number" ? currentValue : ""} onChange={(event) => setPayloadField(field.name, event.target.value === "" ? "" : Number(event.target.value))} />
                            ) : field.value.kind === "scalar" || field.value.kind === "identity" ? (
                              <Input value={typeof currentValue === "string" ? currentValue : ""} onChange={(event) => setPayloadField(field.name, event.target.value)} />
                            ) : (
                              <div className="input-unavailable"><Braces size={16} /> Edit this structured value in the JSON payload below.</div>
                            )}
                            {selectedOption?.description && <span className="input-description"><CircleCheck size={13} /> {selectedOption.description}</span>}
                            {discovered && options.length === 0 && <span className="input-description"><Braces size={13} /> No runtime choices are available; manual JSON remains enabled for rejection testing.</span>}
                          </div>
                        );
                      })}
                    </div>
                    <div className="code-editor">
                      <div className="editor-chrome"><span /><span /><strong>command.json</strong></div>
                      <Textarea aria-label="Command JSON payload" spellCheck={false} value={payloadJson} onChange={(event) => setPayloadJson(event.target.value)} />
                    </div>
                    {parsedPayload.error && <div className="form-error"><CircleX size={16} /><span>{parsedPayload.error}</span></div>}
                    {inputsError && <div className="form-error"><CircleX size={16} /><span>Could not load runtime choices: {inputsError}</span></div>}
                  </div>

                  {error && <div className="form-error"><CircleX size={16} /><span>{error}</span></div>}
                  <div className="simulation-note"><ShieldCheck size={17} /><div><strong>{modeCopy.title}</strong><span>{modeCopy.detail}</span>{mode !== "simulate" && <span>Event effects are observed through the application pipeline rather than predicted by Tracer.</span>}{mode === "dispatch" && <span>Inputs and discovered stream versions come from test history; production validates against its own current state.</span>}</div></div>
                  <Button className="run-button" variant="steel" size="lg" type="submit" disabled={busy || inputsLoading || !targetId || !selectedVersion || !operationHref || !commandInputComplete}>
                    <span className="run-icon">{busy ? <Activity size={17} /> : mode === "test" ? <Beaker size={17} /> : mode === "dispatch" ? <Rocket size={16} /> : <Play size={16} fill="currentColor" />}</span>
                    <span>{busy ? statusLabel : mode === "test" ? `Publish ${selectedCommand?.label ?? "command"} to test` : mode === "dispatch" ? `Dispatch ${selectedCommand?.label ?? "command"} to production` : `Preview ${selectedCommand?.label ?? "command"}`}</span><kbd>⌘ ↵</kbd>
                  </Button>
                </form>
              )}
            </CardContent>
          </Card>

          <Card className="trace-panel outcome-panel">
            <CardHeader className="trace-header">
              <div className="trace-title">
                <div className="trace-pulse">{operationMode === "test" ? <Beaker size={18} /> : operationMode === "dispatch" ? <Rocket size={18} /> : <Play size={17} />}</div>
                <div><span className="eyebrow">Command outcome</span><CardTitle>{operation?.operationId || "No operation"}</CardTitle><CardDescription>{modeName(operation?.mode ?? operationMode)} evidence from Tracer</CardDescription></div>
              </div>
              <Badge variant={operationMode === "dispatch" ? "accent" : "outline"}>{modeName(operation?.mode ?? operationMode)}</Badge>
            </CardHeader>
            <div className="trace-summary">
              <div className="summary-status"><span className={`status-orb state-${runState}`} /><div><span>Status</span><strong>{statusLabel}</strong></div></div>
              <div><span>Environment</span><strong>{modeName(operation?.mode ?? operationMode)}</strong></div>
              <div><span>Base stream</span><strong>{resultStreamVersion === undefined ? "--" : `v${resultStreamVersion}`}</strong></div>
              <div><span>Duration</span><strong>{busy ? "running" : durationMs === null ? "--" : `${durationMs} ms`}</strong></div>
            </div>
            <CardContent className="outcome-content" aria-live="polite">
              {!submittedRequest && (
                <div className="empty-trace"><div><Radio size={24} /></div><h3>No command flow</h3><p>{connected ? "Run an API-discovered command to inspect transport evidence or preview effects." : "Connect to Tracer to begin."}</p></div>
              )}
              {submittedRequest && (
                <div className="command-flow">
                  <button
                    className={`flow-command flow-${runState} ${selectedOutcome === "command" || (correlationCommand && selectedOutcome === `correlation-${correlationCommand.id}`) ? "selected" : ""}`}
                    type="button"
                    onClick={() => setSelectedOutcome(correlationCommand ? `correlation-${correlationCommand.id}` : "command")}
                  >
                    <span className="flow-glyph">{runState === "accepted" ? <CircleCheck size={21} /> : runState === "rejected" || runState === "failed" ? <CircleX size={21} /> : <Send size={19} />}</span>
                    <span className="flow-copy"><small>{correlationCommand ? "Correlated command" : "Submitted command"}</small><strong>{correlationCommand?.command ?? submittedRequest.command}</strong><span>{correlationCommand?.aggregateId ?? submittedRequest.aggregateId} · schema v{correlationCommand?.schemaVersion ?? submittedRequest.schemaVersion}</span></span>
                    <Badge variant="outline">{statusLabel}</Badge>
                  </button>

                  {publicationEvidence && (
                    <div className="domain-event-step transport-event-step">
                      <span className="flow-connector" aria-hidden="true"><span /></span>
                      <button
                        className={`flow-transport-event ${selectedOutcome === "published" ? "selected" : ""}`}
                        type="button"
                        onClick={() => setSelectedOutcome("published")}
                      >
                        <span className="flow-glyph"><Send size={19} /></span>
                        <span className="flow-copy"><small>Command published</small><strong>{publicationEvidence.commandMessageId ?? "Message ID not reported"}</strong><span>Duplicate: {publicationEvidence.duplicate === true ? "yes" : publicationEvidence.duplicate === false ? "no" : "not reported"}</span></span>
                        <Badge variant={publicationEvidence.duplicate === true ? "accent" : "outline"}>{publicationEvidence.duplicate === true ? "Duplicate" : publicationEvidence.duplicate === false ? "New" : "Unknown"}</Badge>
                      </button>
                    </div>
                  )}

                  {responseEvidence && (
                    <div className="domain-event-step transport-event-step">
                      <span className="flow-connector" aria-hidden="true"><span /></span>
                      <button
                        className={`flow-transport-event ${selectedOutcome === "responded" ? "selected" : ""}`}
                        type="button"
                        onClick={() => setSelectedOutcome("responded")}
                      >
                        <span className="flow-glyph"><ShieldCheck size={19} /></span>
                        <span className="flow-copy"><small>Command responded</small><strong>{responseEvidence.responseMessageId}</strong><span>Durable application response</span></span>
                        <Badge variant="success">Answered</Badge>
                      </button>
                    </div>
                  )}

                  {predictedEvents.map((event) => (
                    <div className="domain-event-step predicted-event-step" key={`predicted-${event.ordinal}`}>
                      <span className="flow-connector" aria-hidden="true"><span /></span>
                      <button
                        className={`flow-domain-event flow-predicted-event ${selectedOutcome === `predicted-${event.ordinal}` ? "selected" : ""}`}
                        type="button"
                        onClick={() => setSelectedOutcome(`predicted-${event.ordinal}`)}
                      >
                        <span className="flow-glyph"><Sparkles size={19} /></span>
                        <span className="flow-copy"><small>Simulated prediction</small><strong>{event.eventType}</strong><span>schema v{event.schemaVersion} · predicted stream v{event.predictedStreamVersion}</span></span>
                        <Badge variant="accent">Predicted</Badge>
                      </button>
                    </div>
                  ))}

                  {businessEvents.map((event) => event.type === "domain-event" ? (
                    <div className="domain-event-step" key={event.id}>
                      <span className="flow-connector" aria-hidden="true"><span /></span>
                      <button
                        className={`flow-domain-event ${selectedOutcome === `correlation-${event.id}` ? "selected" : ""}`}
                        type="button"
                        onClick={() => setSelectedOutcome(`correlation-${event.id}`)}
                      >
                        <span className="flow-glyph"><Sparkles size={19} /></span>
                        <span className="flow-copy"><small>Committed domain event</small><strong>{event.eventType}</strong><span>schema v{event.schemaVersion} · {event.streamVersion === undefined ? "stream version not reported" : `stream v${event.streamVersion}`}</span></span>
                        <Badge variant="success">Committed</Badge>
                      </button>
                    </div>
                  ) : (
                    <div className="domain-event-step transport-event-step" key={event.id}>
                      <span className="flow-connector" aria-hidden="true"><span /></span>
                      <button
                        className={`flow-transport-event ${selectedOutcome === `correlation-${event.id}` ? "selected" : ""}`}
                        type="button"
                        onClick={() => setSelectedOutcome(`correlation-${event.id}`)}
                      >
                        <span className="flow-glyph"><Radio size={19} /></span>
                        <span className="flow-copy"><small>Public integration event</small><strong>{event.eventType}</strong><span>{event.subject ?? `schema v${event.schemaVersion}`}</span></span>
                        <Badge variant="accent">Public</Badge>
                      </button>
                    </div>
                  ))}

                  {correlationResult && (
                    <div className="domain-event-step result-event-step">
                      <span className="flow-connector" aria-hidden="true"><span /></span>
                      <button
                        className={`flow-transport-event flow-command-result flow-result-${correlationResult.outcome} ${selectedOutcome === `correlation-${correlationResult.id}` ? "selected" : ""}`}
                        type="button"
                        onClick={() => setSelectedOutcome(`correlation-${correlationResult.id}`)}
                      >
                        <span className="flow-glyph">{correlationResult.outcome === "accepted" ? <CircleCheck size={19} /> : <CircleX size={19} />}</span>
                        <span className="flow-copy"><small>Correlated command result</small><strong>{correlationResult.outcome}</strong><span>{correlationResult.operationId}</span></span>
                        <Badge variant={correlationResult.outcome === "accepted" ? "success" : "danger"}>{correlationResult.outcome}</Badge>
                      </button>
                    </div>
                  )}

                  {busy && businessEvents.length === 0 && predictedEvents.length === 0 && (
                    <div className="domain-event-step flow-waiting"><span className="flow-connector" aria-hidden="true"><span /></span><div><Activity size={17} /> {submittedRequest.mode === "simulate" ? "Waiting for simulated predictions" : "Waiting for correlated business events"}</div></div>
                  )}
                  {runState === "accepted" && submittedRequest.mode === "simulate" && predictedEvents.length === 0 && (
                    <div className="domain-event-step"><span className="flow-connector" aria-hidden="true"><span /></span><div className="flow-no-events"><Check size={17} /> Preview predicted no domain events</div></div>
                  )}
                  {runState === "accepted" && submittedRequest.mode !== "simulate" && businessEvents.length === 0 && (
                    <div className="transport-observation"><Radio size={16} /> {streamActive ? "No business events observed yet; the correlation stream remains active for asynchronous effects." : "No business events were observed before the correlation stream stopped."}</div>
                  )}
                  {(runState === "rejected" || runState === "failed") && businessEvents.length === 0 && (
                    <div className="flow-stopped"><CircleX size={16} /> {runState === "rejected" && submittedRequest.mode !== "simulate" ? "The durable response rejected the command" : "Flow stopped at the command result"}</div>
                  )}
                  {streamError && <div className="stream-warning"><Radio size={15} /> Correlation SSE disconnected; terminal status came from operation polling.</div>}
                </div>
              )}
            </CardContent>
          </Card>

          <Card className="inspector-panel">
            <CardHeader>
              <div><span className="eyebrow">Inspector</span><CardTitle>{selectedTransportEvidence ? "Transport evidence" : selectedPredictedEvent ? "Predicted domain event" : selectedIntegrationEvent ? "Integration event" : selectedDomainEvent ? "Committed domain event" : selectedCorrelationCommand ? "Correlated command" : "Command result"}</CardTitle><CardDescription>Select a node in the correlated command flow</CardDescription></div>
              <Tooltip>
                <TooltipTrigger asChild><Button variant="ghost" size="icon" type="button" onClick={copyInspection} disabled={!submittedRequest} aria-label="Copy selected JSON">{copied ? <Check size={17} /> : <Copy size={17} />}</Button></TooltipTrigger>
                <TooltipContent>{copied ? "Copied" : "Copy selected JSON"}</TooltipContent>
              </Tooltip>
            </CardHeader>
            {submittedRequest ? (
              <CardContent className="inspector-content">
                {selectedTransportEvidence ? (
                  <>
                    <div className="selected-event-heading">
                      <span className="large-event-glyph tone-accent">{selectedTransportEvidence.type === "command.published" ? <Send size={20} /> : <ShieldCheck size={20} />}</span>
                      <div>
                        <Badge variant="outline">{selectedTransportEvidence.type}</Badge>
                        <h3>{selectedTransportEvidence.type === "command.published" ? selectedTransportEvidence.commandMessageId ?? "Message ID not reported" : selectedTransportEvidence.responseMessageId}</h3>
                        <p>{selectedTransportEvidence.type === "command.published" ? "Durable command publication evidence" : "Durable application response evidence"}</p>
                      </div>
                    </div>
                    <dl className="event-metadata">
                      <div><dt>Event</dt><dd>{selectedTransportEvidence.type}</dd></div>
                      {selectedTransportEvidence.type === "command.published" && <div><dt>Duplicate</dt><dd>{selectedTransportEvidence.duplicate === true ? "Yes" : selectedTransportEvidence.duplicate === false ? "No" : "Not reported"}</dd></div>}
                      <div><dt>Environment</dt><dd>{modeName(submittedRequest.mode)}</dd></div>
                      <div><dt>Operation</dt><dd>{operation?.operationId ?? "Pending"}</dd></div>
                    </dl>
                    <div className="json-section"><div className="json-heading"><span>Transport event</span><Braces size={16} /></div><pre><JsonView value={selectedTransportEvidence} /></pre></div>
                  </>
                ) : selectedPredictedEvent ? (
                  <>
                    <div className="selected-event-heading">
                      <span className="large-event-glyph tone-accent"><Sparkles size={20} /></span>
                      <div><Badge variant="accent">Simulated prediction</Badge><h3>{selectedPredictedEvent.eventType}</h3><p>Read-only prediction from {submittedRequest.command}</p></div>
                    </div>
                    <dl className="event-metadata">
                      <div><dt>Schema</dt><dd>v{selectedPredictedEvent.schemaVersion}</dd></div>
                      <div><dt>Ordinal</dt><dd>{selectedPredictedEvent.ordinal}</dd></div>
                      <div><dt>Predicted stream</dt><dd>v{selectedPredictedEvent.predictedStreamVersion}</dd></div>
                      <div><dt>Environment</dt><dd>{modeName(submittedRequest.mode)}</dd></div>
                    </dl>
                    <div className="json-section"><div className="json-heading"><span>Predicted domain event</span><Braces size={16} /></div><pre><JsonView value={selectedPredictedEvent} /></pre></div>
                  </>
                ) : selectedIntegrationEvent ? (
                  <>
                    <div className="selected-event-heading">
                      <span className="large-event-glyph tone-accent"><Radio size={20} /></span>
                      <div>
                        <Badge variant="accent">Public integration event</Badge>
                        <h3>{selectedIntegrationEvent.eventType}</h3>
                        <p>Public event observed after {submittedRequest.command}</p>
                      </div>
                    </div>
                    <dl className="event-metadata">
                      <div><dt>Schema</dt><dd>v{selectedIntegrationEvent.schemaVersion}</dd></div>
                      <div><dt>Sequence</dt><dd>{selectedIntegrationEvent.id}</dd></div>
                      <div><dt>Message</dt><dd>{selectedIntegrationEvent.messageId ?? "Not reported"}</dd></div>
                      <div><dt>Environment</dt><dd>{modeName(submittedRequest.mode)}</dd></div>
                    </dl>
                    <div className="json-section"><div className="json-heading"><span>Integration event</span><Braces size={16} /></div><pre><JsonView value={selectedIntegrationEvent} /></pre></div>
                  </>
                ) : selectedDomainEvent ? (
                  <>
                    <div className="selected-event-heading">
                      <span className="large-event-glyph tone-success"><Sparkles size={20} /></span>
                      <div><Badge variant="success">Committed domain event</Badge><h3>{selectedDomainEvent.eventType}</h3><p>Observed after {submittedRequest.command}</p></div>
                    </div>
                    <dl className="event-metadata">
                      <div><dt>Schema</dt><dd>v{selectedDomainEvent.schemaVersion}</dd></div>
                      <div><dt>Sequence</dt><dd>{selectedDomainEvent.id}</dd></div>
                      <div><dt>Stream</dt><dd>{selectedDomainEvent.streamVersion === undefined ? "Not reported" : `v${selectedDomainEvent.streamVersion}`}</dd></div>
                      <div><dt>Environment</dt><dd>{modeName(submittedRequest.mode)}</dd></div>
                    </dl>
                    <div className="json-section"><div className="json-heading"><span>Committed domain event</span><Braces size={16} /></div><pre><JsonView value={selectedDomainEvent} /></pre></div>
                  </>
                ) : selectedCorrelationCommand ? (
                  <>
                    <div className="selected-event-heading">
                      <span className="large-event-glyph tone-accent"><Send size={20} /></span>
                      <div><Badge variant="outline">Correlated command</Badge><h3>{selectedCorrelationCommand.command}</h3><p>{selectedCorrelationCommand.aggregateType} · {selectedCorrelationCommand.aggregateId}</p></div>
                    </div>
                    <dl className="event-metadata">
                      <div><dt>Sequence</dt><dd>{selectedCorrelationCommand.id}</dd></div>
                      <div><dt>Correlation</dt><dd>{selectedCorrelationCommand.correlationId}</dd></div>
                      <div><dt>Operation</dt><dd>{selectedCorrelationCommand.operationId}</dd></div>
                      <div><dt>Schema</dt><dd>v{selectedCorrelationCommand.schemaVersion}</dd></div>
                    </dl>
                    <div className="json-section"><div className="json-heading"><span>Correlation event</span><Braces size={16} /></div><pre><JsonView value={selectedCorrelationCommand} /></pre></div>
                  </>
                ) : selectedCommandResult ? (
                  <>
                    <div className="selected-event-heading">
                      <span className={`large-event-glyph ${selectedCommandResult.outcome === "accepted" ? "tone-success" : "tone-danger"}`}>{selectedCommandResult.outcome === "accepted" ? <CircleCheck size={20} /> : <CircleX size={20} />}</span>
                      <div><Badge variant={selectedCommandResult.outcome === "accepted" ? "success" : "danger"}>Correlated command result</Badge><h3>{selectedCommandResult.outcome}</h3><p>{selectedCommandResult.operationId}</p></div>
                    </div>
                    <dl className="event-metadata">
                      <div><dt>Sequence</dt><dd>{selectedCommandResult.id}</dd></div>
                      <div><dt>Correlation</dt><dd>{selectedCommandResult.correlationId}</dd></div>
                      <div><dt>Outcome</dt><dd>{selectedCommandResult.outcome}</dd></div>
                      <div><dt>Environment</dt><dd>{modeName(submittedRequest.mode)}</dd></div>
                    </dl>
                    <div className="json-section"><div className="json-heading"><span>Command-result event</span><Braces size={16} /></div><pre><JsonView value={selectedCommandResult} /></pre></div>
                  </>
                ) : (
                  <>
                    <div className="selected-event-heading">
                      <span className={`large-event-glyph ${runState === "accepted" ? "tone-success" : runState === "rejected" || runState === "failed" ? "tone-danger" : "tone-accent"}`}>{runState === "accepted" ? <CircleCheck size={20} /> : runState === "rejected" || runState === "failed" ? <CircleX size={20} /> : <Activity size={20} />}</span>
                      <div><Badge variant="outline">Command</Badge><h3>{submittedRequest.command}</h3><p>{modeName(submittedRequest.mode)} · {submittedRequest.aggregateId}</p></div>
                    </div>
                    {runState === "accepted" && <div className="command-result command-result-success"><CircleCheck size={21} /><div><span>Success</span><strong>Command accepted</strong><p>{acceptedResultCopy}</p></div></div>}
                    {(runState === "rejected" || runState === "failed") && <div className="command-result command-result-failure"><CircleX size={21} /><div><span>Failure</span><strong>{runState === "rejected" ? "Command rejected" : "Command failed"}</strong><p>{operation?.failure?.message ?? "The command did not produce a successful decision."}</p></div></div>}
                    {runState === "indeterminate" && <div className="command-result command-result-pending"><Activity size={21} /><div><span>Published</span><strong>Business outcome unknown</strong><p>{operation?.failure?.message ?? "The broker stored the command, but no durable response was available before the operation ended."}</p></div></div>}
                    {busy && <div className="command-result command-result-pending"><Activity size={21} /><div><span>Pending</span><strong>{statusLabel}</strong><p>Waiting for the command result.</p></div></div>}
                    <dl className="event-metadata">
                      <div><dt>Environment</dt><dd>{modeName(submittedRequest.mode)}</dd></div>
                      <div><dt>Aggregate</dt><dd>{submittedRequest.aggregateId}</dd></div>
                      <div><dt>Schema</dt><dd>v{submittedRequest.schemaVersion}</dd></div>
                      <div><dt>Status</dt><dd>{operation?.status ?? runState}</dd></div>
                      <div><dt>Published</dt><dd>{publishedEvidenceLabel}</dd></div>
                      <div><dt>Append evidence</dt><dd>{appendEvidenceLabel}</dd></div>
                    </dl>
                    <div className="json-section"><div className="json-heading"><span>Command result</span><Braces size={16} /></div><pre><JsonView value={operation?.failure ?? operation?.result ?? { status: operation?.status ?? runState }} /></pre></div>
                    <div className="json-section"><div className="json-heading"><span>Submitted command</span><Braces size={16} /></div><pre><JsonView value={submittedRequest} /></pre></div>
                  </>
                )}
              </CardContent>
            ) : (
              <CardContent className="inspector-empty"><Braces size={26} /><p>Run a command, then select it to inspect success or failure.</p></CardContent>
            )}
            <CardFooter><span><span className={`footer-dot state-${runState}`} /> {busy ? "Live command flow" : operation ? "Command settled" : "No command"}</span><Badge variant="outline">{streamActive ? "Correlation SSE active" : streamError ? "Correlation SSE disconnected" : "Correlation events via SSE"}</Badge></CardFooter>
          </Card>
        </div>
      </main>
    </TooltipProvider>
  );
}

export default App;
