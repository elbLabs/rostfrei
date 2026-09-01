import './styles.css';

import {
  ApiClient,
  ApiError,
  ResponseFormatError,
  formatJson,
  formatJsonText,
  parseJson,
  type StreamEvent,
} from './api.ts';

type JsonObject = Record<string, unknown>;

interface PersistedTest {
  definitionHref?: string;
  id: string;
  name: string;
  revision: string;
  runHref?: string;
}

type RequestKind = 'api' | 'infrastructure' | 'request';

const CORRELATION_QUIET_MILLIS = 1_800;
const CORRELATION_STARTUP_MILLIS = 10_000;

function element<T extends HTMLElement>(id: string, constructor: new () => T): T {
  const node = document.getElementById(id);
  if (!(node instanceof constructor)) {
    throw new Error(`Required element #${id} is missing`);
  }
  return node;
}

const connectionForm = element('connection-form', HTMLFormElement);
const apiBaseInput = element('api-base', HTMLInputElement);
const tokenInput = element('control-token', HTMLInputElement);
const connectButton = element('connect-button', HTMLButtonElement);
const connectionState = element('connection-state', HTMLParagraphElement);
const catalogVersion = element('catalog-version', HTMLElement);
const behaviorSupport = element('behavior-support', HTMLElement);
const persistedSection = element('persisted-section', HTMLElement);
const persistedSelect = element('persisted-tests', HTMLSelectElement);
const persistedNote = element('persisted-note', HTMLParagraphElement);
const editor = element('definition-editor', HTMLTextAreaElement);
const validateButton = element('validate-button', HTMLButtonElement);
const runButton = element('run-button', HTMLButtonElement);
const requestStatus = element('request-status', HTMLParagraphElement);
const errorPanel = element('error-panel', HTMLElement);
const errorTitle = element('error-title', HTMLElement);
const errorDetail = element('error-detail', HTMLPreElement);
const validationPanel = element('validation-panel', HTMLElement);
const validationResponse = element('validation-response', HTMLPreElement);
const reportSection = element('report-section', HTMLElement);
const reportVerdict = element('report-verdict', HTMLElement);
const reportRunId = element('report-run-id', HTMLElement);
const reportTestId = element('report-test-id', HTMLElement);
const reportRevision = element('report-revision', HTMLElement);
const comparisonStatus = element('comparison-status', HTMLElement);
const reportOperation = element('report-operation', HTMLPreElement);
const currentOperation = element('current-operation', HTMLPreElement);
const operationFollowStatus = element('operation-follow-status', HTMLElement);
const diagnosticCount = element('diagnostic-count', HTMLElement);
const diagnosticsList = element('diagnostics-list', HTMLOListElement);
const expectedJson = element('expected-json', HTMLPreElement);
const observedJson = element('observed-json', HTMLPreElement);
const commandOutcome = element('command-outcome', HTMLPreElement);
const operationStreamStatus = element('operation-stream-status', HTMLElement);
const correlationStreamStatus = element('correlation-stream-status', HTMLElement);
const operationEvents = element('operation-events', HTMLOListElement);
const correlationEvents = element('correlation-events', HTMLOListElement);
const rawReport = element('raw-report', HTMLPreElement);

let client: ApiClient | null = null;
let behavioralTest: JsonObject | null = null;
let persistedTests: PersistedTest[] = [];
let busy = false;
let activeStreamControllers: AbortController[] = [];

function isObject(value: unknown): value is JsonObject {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function stringMember(object: JsonObject | null, key: string): string | undefined {
  const value = object?.[key];
  return typeof value === 'string' ? value : undefined;
}

function displayValue(value: unknown, fallback = 'not returned'): string {
  if (typeof value === 'string') {
    return value;
  }
  if (typeof value === 'number' || typeof value === 'boolean') {
    return String(value);
  }
  return value === undefined ? fallback : formatJson(value);
}

function linkValue(value: unknown): string | undefined {
  if (typeof value === 'string') {
    return value;
  }
  return isObject(value) ? stringMember(value, 'href') : undefined;
}

function relatedHref(object: JsonObject, relation: 'definition' | 'self'): string | undefined {
  const direct =
    stringMember(object, `${relation}Href`) ??
    (relation === 'self' ? stringMember(object, 'href') : undefined);
  if (direct !== undefined) {
    return direct;
  }

  const links = object.links;
  if (isObject(links)) {
    return linkValue(links[relation]);
  }
  if (Array.isArray(links)) {
    for (const link of links) {
      if (isObject(link) && link.rel === relation) {
        const href = stringMember(link, 'href');
        if (href !== undefined) {
          return href;
        }
      }
    }
  }
  return undefined;
}

function selectedTest(): PersistedTest | undefined {
  if (persistedSelect.value === '') {
    return undefined;
  }
  const index = Number.parseInt(persistedSelect.value, 10);
  return persistedTests[index];
}

function setRequestStatus(message: string, state: 'active' | 'idle' | 'success' = 'idle'): void {
  requestStatus.textContent = message;
  requestStatus.dataset.state = state;
}

function setConnection(message: string, state: 'connected' | 'idle' | 'working'): void {
  connectionState.textContent = message;
  connectionState.dataset.state = state;
}

function hideError(): void {
  errorPanel.hidden = true;
  errorDetail.textContent = '';
}

function showError(kind: RequestKind, context: string, error: unknown): void {
  let detail: string;
  let title: string;

  if (error instanceof ApiError) {
    title = 'API error';
    detail = `${context}\n${error.status} ${error.statusText}\n${error.url}`;
    if (error.body !== '') {
      detail += `\n\n${error.body}`;
    }
  } else if (error instanceof ResponseFormatError) {
    title = 'Infrastructure response error';
    detail = `${context}\n${error.message}\n${error.url}`;
    if (error.body !== '') {
      detail += `\n\n${error.body}`;
    }
  } else {
    title =
      kind === 'request'
        ? 'Local request error'
        : kind === 'api'
          ? 'API error'
          : 'Infrastructure error';
    detail = `${context}\n${error instanceof Error ? error.message : String(error)}`;
  }

  errorTitle.textContent = title;
  errorDetail.textContent = detail;
  errorPanel.hidden = false;
}

function setBusy(nextBusy: boolean): void {
  busy = nextBusy;
  apiBaseInput.disabled = busy;
  tokenInput.disabled = busy;
  editor.disabled = busy;
  connectButton.disabled = busy;
  persistedSelect.disabled = busy || persistedTests.length === 0;
  validateButton.disabled =
    busy || client === null || stringMember(behavioralTest, 'validateHref') === undefined;
  const persisted = selectedTest();
  const runHref =
    persisted === undefined ? stringMember(behavioralTest, 'runHref') : persisted.runHref;
  runButton.disabled = busy || client === null || runHref === undefined;
}

function disconnectEditedConnection(): void {
  abortStreams();
  client = null;
  behavioralTest = null;
  persistedTests = [];
  persistedSection.hidden = true;
  persistedSelect.replaceChildren(new Option('Raw editor definition', ''));
  catalogVersion.textContent = 'not loaded';
  behaviorSupport.textContent = 'unknown';
  setConnection('Connection changed', 'idle');
  setRequestStatus('Reload the catalog to use this connection.');
  setBusy(false);
}

function requireRawJson(): string {
  const raw = editor.value;
  parseJson(raw);
  return raw;
}

function appendEvent(list: HTMLOListElement, event: StreamEvent): void {
  const item = document.createElement('li');
  const heading = document.createElement('div');
  const id = document.createElement('span');
  const name = document.createElement('strong');
  const payload = document.createElement('pre');
  heading.className = 'event-heading';
  id.textContent = event.id === '' ? '#-' : `#${event.id}`;
  name.textContent = event.event;
  payload.textContent = formatJsonText(event.data);
  heading.append(id, name);
  item.append(heading, payload);
  list.append(item);
}

function renderDiagnostics(comparison: unknown): void {
  diagnosticsList.replaceChildren();
  const diagnostics = isObject(comparison) ? comparison.diagnostics : undefined;
  if (!Array.isArray(diagnostics) || diagnostics.length === 0) {
    diagnosticCount.textContent = '0 returned';
    const item = document.createElement('li');
    item.className = 'empty-result';
    item.textContent = 'No comparison diagnostics returned.';
    diagnosticsList.append(item);
    return;
  }

  diagnosticCount.textContent = `${diagnostics.length} returned`;
  for (const diagnostic of diagnostics) {
    const object = isObject(diagnostic) ? diagnostic : null;
    const item = document.createElement('li');
    const heading = document.createElement('div');
    const code = document.createElement('strong');
    const path = document.createElement('code');
    const message = document.createElement('p');
    const values = document.createElement('div');
    heading.className = 'diagnostic-heading';
    code.textContent = displayValue(object?.code, 'diagnostic');
    path.textContent = displayValue(object?.path, 'path not returned');
    message.textContent = displayValue(object?.message, 'No diagnostic message returned.');
    values.className = 'diagnostic-values';
    heading.append(code, path);
    item.append(heading, message);

    for (const key of ['expected', 'observed'] as const) {
      if (object?.[key] !== undefined) {
        const valueBlock = document.createElement('div');
        const label = document.createElement('h4');
        const value = document.createElement('pre');
        label.textContent = key;
        value.textContent = formatJson(object[key]);
        valueBlock.append(label, value);
        values.append(valueBlock);
      }
    }
    if (values.childElementCount > 0) {
      item.append(values);
    }
    diagnosticsList.append(item);
  }
}

function renderReport(value: unknown, rawText: string): JsonObject {
  const report = isObject(value) ? value : {};
  const comparison = report.comparison;
  const comparisonObject = isObject(comparison) ? comparison : null;
  const status = displayValue(report.status, 'unknown').toLowerCase();

  reportSection.hidden = false;
  reportVerdict.textContent = status.toUpperCase();
  reportVerdict.dataset.verdict = status;
  reportRunId.textContent = displayValue(report.runId);
  reportTestId.textContent = displayValue(report.testId);
  reportRevision.textContent = displayValue(report.revision, 'inline');
  comparisonStatus.textContent = displayValue(comparisonObject?.status);
  reportOperation.textContent = formatJson(report.operation);
  currentOperation.textContent = 'Waiting for advertised operation link.';
  operationFollowStatus.textContent = 'Waiting';
  expectedJson.textContent = formatJson(report.expected);
  observedJson.textContent = formatJson(report.observed);
  commandOutcome.textContent = formatJson(report.commandOutcome);
  rawReport.textContent = rawText;
  renderDiagnostics(comparison);

  operationEvents.replaceChildren();
  correlationEvents.replaceChildren();
  operationStreamStatus.textContent = 'Waiting';
  correlationStreamStatus.textContent = 'Waiting';
  return report;
}

function abortStreams(): void {
  for (const controller of activeStreamControllers) {
    controller.abort();
  }
  activeStreamControllers = [];
}

function isAbortError(error: unknown): boolean {
  return error instanceof DOMException && error.name === 'AbortError';
}

async function followCurrentOperation(report: JsonObject, activeClient: ApiClient): Promise<void> {
  const href = stringMember(report, 'operationHref');
  if (href === undefined) {
    currentOperation.textContent = 'No operation href was advertised.';
    operationFollowStatus.textContent = 'Unavailable';
    return;
  }

  operationFollowStatus.textContent = 'Loading';
  try {
    const response = await activeClient.requestJson(href);
    currentOperation.textContent = formatJson(response.value);
    operationFollowStatus.textContent = 'Current snapshot loaded';
  } catch (error) {
    operationFollowStatus.textContent = 'Failed';
    showError('infrastructure', 'Could not follow the report operation href.', error);
  }
}

async function collectOperationEvents(report: JsonObject, activeClient: ApiClient): Promise<void> {
  const href = stringMember(report, 'operationEventsHref');
  if (href === undefined) {
    operationStreamStatus.textContent = 'No href advertised';
    return;
  }

  const controller = new AbortController();
  activeStreamControllers.push(controller);
  let count = 0;
  operationStreamStatus.textContent = 'Replaying / live';
  try {
    const result = await activeClient.consumeEventStream(
      href,
      controller.signal,
      (event) => {
        count += 1;
        appendEvent(operationEvents, event);
        operationStreamStatus.textContent = `${count} received`;
      },
      '0',
    );
    operationStreamStatus.textContent =
      result === 'no-content' ? 'Already complete; no new events' : `${count} received; closed`;
  } catch (error) {
    if (!isAbortError(error)) {
      operationStreamStatus.textContent = `${count} received; failed`;
      showError('infrastructure', 'Operation event stream failed.', error);
    }
  }
}

async function collectCorrelationEvents(
  report: JsonObject,
  activeClient: ApiClient,
): Promise<'incomplete' | 'settled'> {
  const href = stringMember(report, 'correlationEventsHref');
  if (href === undefined) {
    correlationStreamStatus.textContent = 'No href advertised';
    return 'incomplete';
  }

  const controller = new AbortController();
  activeStreamControllers.push(controller);
  let count = 0;
  const timerState: { abortReason: 'quiet' | 'startup' } = { abortReason: 'startup' };
  let quietTimer = window.setTimeout(() => {
    controller.abort();
  }, CORRELATION_STARTUP_MILLIS);
  const resetQuietTimer = (): void => {
    window.clearTimeout(quietTimer);
    timerState.abortReason = 'quiet';
    quietTimer = window.setTimeout(() => {
      controller.abort();
    }, CORRELATION_QUIET_MILLIS);
  };

  correlationStreamStatus.textContent = 'Replaying / live';
  try {
    await activeClient.consumeEventStream(
      href,
      controller.signal,
      (event) => {
        count += 1;
        appendEvent(correlationEvents, event);
        correlationStreamStatus.textContent = `${count} received; waiting for quiet`;
        resetQuietTimer();
      },
      '0',
    );
    correlationStreamStatus.textContent = `${count} received; server closed`;
    return 'settled';
  } catch (error) {
    if (isAbortError(error)) {
      if (timerState.abortReason === 'quiet') {
        correlationStreamStatus.textContent = `${count} received; quiet snapshot ended; later events may arrive`;
        return 'settled';
      }
      correlationStreamStatus.textContent = 'Stream startup timed out; snapshot incomplete';
    } else {
      correlationStreamStatus.textContent = `${count} received; failed`;
      showError('infrastructure', 'Correlation event stream failed.', error);
    }
    return 'incomplete';
  } finally {
    window.clearTimeout(quietTimer);
  }
}

async function followReport(
  report: JsonObject,
  activeClient: ApiClient,
): Promise<'incomplete' | 'settled'> {
  abortStreams();
  const [, , correlationResult] = await Promise.all([
    followCurrentOperation(report, activeClient),
    collectOperationEvents(report, activeClient),
    collectCorrelationEvents(report, activeClient),
  ]);
  activeStreamControllers = [];
  return correlationResult;
}

function extractPersistedTests(value: unknown): PersistedTest[] {
  if (!isObject(value) || !Array.isArray(value.items)) {
    return [];
  }
  const tests: PersistedTest[] = [];
  for (const [index, item] of value.items.entries()) {
    if (!isObject(item)) {
      continue;
    }
    const id = stringMember(item, 'id') ?? `item-${index + 1}`;
    const name = stringMember(item, 'name') ?? id;
    const revision = stringMember(item, 'revision') ?? 'revision not returned';
    const runHref = stringMember(item, 'runHref');
    const definitionHref = relatedHref(item, 'definition') ?? relatedHref(item, 'self');
    tests.push({
      id,
      name,
      revision,
      ...(runHref === undefined ? {} : { runHref }),
      ...(definitionHref === undefined ? {} : { definitionHref }),
    });
  }
  return tests;
}

function renderPersistedTests(): void {
  persistedSelect.replaceChildren(new Option('Raw editor definition', ''));
  for (const [index, test] of persistedTests.entries()) {
    const label = `${test.name} [${test.id}] @ ${test.revision.slice(0, 10)}`;
    persistedSelect.add(new Option(label, String(index)));
  }
  persistedNote.textContent =
    persistedTests.length === 0
      ? 'The advertised collection contains no definitions.'
      : `${persistedTests.length} definition${persistedTests.length === 1 ? '' : 's'} advertised.`;
}

async function loadPersistedCollection(href: string, activeClient: ApiClient): Promise<void> {
  persistedSection.hidden = false;
  persistedNote.textContent = 'Loading advertised collection...';
  try {
    const response = await activeClient.requestJson(href);
    persistedTests = extractPersistedTests(response.value);
    renderPersistedTests();
  } catch (error) {
    persistedTests = [];
    renderPersistedTests();
    persistedNote.textContent = 'The advertised collection could not be loaded.';
    showError('infrastructure', 'Could not load persisted test definitions.', error);
  }
}

async function connect(): Promise<void> {
  hideError();
  validationPanel.hidden = true;
  const base = apiBaseInput.value.trim();
  const token = tokenInput.value;
  if (base === '' || token === '') {
    showError('request', 'Connection fields are incomplete.', 'Enter an API base URL and control token.');
    setRequestStatus('Connection fields are required.');
    return;
  }

  abortStreams();
  setBusy(true);
  setConnection('Loading catalog', 'working');
  setRequestStatus('Requesting the authenticated catalog...', 'active');
  try {
    const nextClient = new ApiClient(base, token);
    const response = await nextClient.requestJson('/catalog');
    if (!isObject(response.value)) {
      throw new ResponseFormatError(response.url, response.rawText, 'Expected a catalog object');
    }
    client = nextClient;
    catalogVersion.textContent = displayValue(response.value.catalogVersion);
    behavioralTest = isObject(response.value.behavioralTest)
      ? response.value.behavioralTest
      : null;
    behaviorSupport.textContent = behavioralTest === null ? 'not advertised' : 'advertised';
    setConnection('Catalog loaded', 'connected');
    setRequestStatus(
      behavioralTest === null
        ? 'Connected. This catalog does not advertise behavioral tests.'
        : 'Connected. Raw validation and execution use advertised links.',
      'success',
    );

    persistedTests = [];
    const definitionsHref = stringMember(behavioralTest, 'definitionsHref');
    persistedSection.hidden = definitionsHref === undefined;
    if (definitionsHref !== undefined) {
      await loadPersistedCollection(definitionsHref, nextClient);
    }
  } catch (error) {
    client = null;
    behavioralTest = null;
    persistedTests = [];
    persistedSection.hidden = true;
    catalogVersion.textContent = 'not loaded';
    behaviorSupport.textContent = 'unknown';
    setConnection('Connection failed', 'idle');
    setRequestStatus('Catalog request failed.');
    showError('infrastructure', 'Could not load the Tracer catalog.', error);
  } finally {
    setBusy(false);
  }
}

async function loadSelectedDefinition(): Promise<void> {
  const test = selectedTest();
  if (test === undefined) {
    persistedNote.textContent = `${persistedTests.length} persisted definition${persistedTests.length === 1 ? '' : 's'} advertised. Raw editor selected.`;
    setBusy(false);
    return;
  }
  if (test.definitionHref === undefined) {
    persistedNote.textContent =
      'This item advertises a run link but no definition link. The editor is unchanged.';
    setRequestStatus('Persisted run selected; its advertised run link will be used.', 'success');
    setBusy(false);
    return;
  }
  if (client === null) {
    return;
  }

  hideError();
  setBusy(true);
  setRequestStatus(`Loading ${test.name} from its advertised definition link...`, 'active');
  try {
    const response = await client.requestJson(test.definitionHref);
    const wrapper = isObject(response.value) ? response.value : null;
    const definition = wrapper?.definition ?? response.value;
    editor.value = formatJson(definition);
    persistedNote.textContent = 'Definition loaded from its advertised link.';
    setRequestStatus('Server definition loaded. Editing it switches back to a raw inline run.', 'success');
  } catch (error) {
    setRequestStatus('Persisted definition request failed.');
    showError('infrastructure', 'Could not follow the persisted definition link.', error);
  } finally {
    setBusy(false);
  }
}

async function validateRawDefinition(): Promise<void> {
  if (client === null) {
    return;
  }
  const href = stringMember(behavioralTest, 'validateHref');
  if (href === undefined) {
    return;
  }

  hideError();
  validationPanel.hidden = true;
  let raw: string;
  try {
    raw = requireRawJson();
  } catch (error) {
    setRequestStatus('Raw definition is not valid JSON.');
    showError('request', 'The raw definition was not sent.', error);
    return;
  }

  setBusy(true);
  setRequestStatus('Validating raw bytes with the Tracer API...', 'active');
  try {
    const response = await client.requestJson(href, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: raw,
    });
    validationResponse.textContent = response.rawText;
    validationPanel.hidden = false;
    setRequestStatus('Validation response received.', 'success');
  } catch (error) {
    setRequestStatus('Validation request failed.');
    showError('api', 'The Tracer did not validate this definition.', error);
  } finally {
    setBusy(false);
  }
}

async function runTest(): Promise<void> {
  if (client === null) {
    return;
  }
  const activeClient = client;
  const persisted = selectedTest();
  const href =
    persisted === undefined ? stringMember(behavioralTest, 'runHref') : persisted.runHref;
  if (href === undefined) {
    showError('request', 'The test cannot be run.', 'No run href was advertised.');
    return;
  }

  hideError();
  validationPanel.hidden = true;
  abortStreams();
  let init: RequestInit = { method: 'POST' };
  if (persisted === undefined) {
    try {
      const raw = requireRawJson();
      init = {
        ...init,
        headers: { 'Content-Type': 'application/json' },
        body: raw,
      };
    } catch (error) {
      setRequestStatus('Raw definition is not valid JSON.');
      showError('request', 'The raw definition was not sent.', error);
      return;
    }
  }

  setBusy(true);
  reportSection.hidden = true;
  setRequestStatus(
    persisted === undefined
      ? 'Running the raw definition through the advertised endpoint...'
      : `Running persisted definition ${persisted.name} through its advertised endpoint...`,
    'active',
  );
  try {
    const response = await activeClient.requestJson(href, init);
    const report = renderReport(response.value, response.rawText);
    const status = displayValue(report.status, 'unknown');
    setRequestStatus(`Final report received: ${status}. Collecting linked traces...`, 'active');
    const traceResult = await followReport(report, activeClient);
    setRequestStatus(
      traceResult === 'settled'
        ? `Final report received: ${status}. Linked trace snapshot settled; later correlation events may arrive.`
        : `Final report received: ${status}. Linked trace snapshot is incomplete.`,
      traceResult === 'settled' ? 'success' : 'active',
    );
  } catch (error) {
    setRequestStatus('Test request failed.');
    showError('api', 'The Tracer did not return a test report.', error);
  } finally {
    setBusy(false);
  }
}

connectionForm.addEventListener('submit', (event) => {
  event.preventDefault();
  void connect();
});
apiBaseInput.addEventListener('input', disconnectEditedConnection);
tokenInput.addEventListener('input', disconnectEditedConnection);
persistedSelect.addEventListener('change', () => {
  void loadSelectedDefinition();
});
editor.addEventListener('input', () => {
  if (persistedSelect.value !== '') {
    persistedSelect.value = '';
    persistedNote.textContent = 'Editor changed. Raw inline run selected.';
    setBusy(busy);
  }
});
validateButton.addEventListener('click', () => {
  void validateRawDefinition();
});
runButton.addEventListener('click', () => {
  void runTest();
});

setBusy(false);
