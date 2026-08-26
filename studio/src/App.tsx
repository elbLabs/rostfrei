import { Fragment, type CSSProperties, type ReactNode } from 'react'
import { AlertCircle, ArrowLeft, ArrowRight, Check, FolderOpen, PanelRight, Play, RotateCcw, XCircle } from 'lucide-react'

import { AppSidebar } from '@/components/app-sidebar'
import { DomainGraphSidebar } from '@/components/domain-graph-sidebar'
import { DisplayTypeView, PresentationFields } from '@/components/presentation-fields'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import {
  Breadcrumb,
  BreadcrumbItem,
  BreadcrumbLink,
  BreadcrumbList,
  BreadcrumbPage,
  BreadcrumbSeparator,
} from '@/components/ui/breadcrumb'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from '@/components/ui/empty'
import { Field, FieldGroup, FieldLabel, FieldTitle } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Separator } from '@/components/ui/separator'
import { SidebarInset, SidebarProvider, SidebarTrigger } from '@/components/ui/sidebar'
import { Spinner } from '@/components/ui/spinner'

import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { TooltipProvider } from '@/components/ui/tooltip'
import {
  getBreadcrumbTrail,
  type DataDefinition,
  type DisplayType,
  type DomainIndex,
  type DomainKey,
  type PresentationAction,
  type PresentationActionOutcomeLink,
  type PresentationBehavior,
  type PresentationDecision,
  type PresentationDomainError,
  type PresentationDomainEvent,
  type PresentationLifecycle,
  type PresentationQuery,
  type PresentationSelection,
  type PresentationVariant,
  type SelectionKind,
} from '@/domain/index'
import { useSelectionHistory } from '@/hooks/use-selection-history'
import { useWorkspace, type WorkspaceState } from '@/hooks/use-workspace'
import type { Diagnostic } from '@/lib/compiler'
import { useSidebarLayoutStore } from '@/stores/sidebar-layout'

function App() {
  const { state, chooseWorkspace, check, retry } = useWorkspace()
  const sidebarWidth = useSidebarLayoutStore((layout) => layout.width)
  const graphOpen = useSidebarLayoutStore((layout) => layout.graphOpen)
  const toggleGraph = useSidebarLayoutStore((layout) => layout.toggleGraph)
  const index = 'index' in state ? state.index : undefined
  const workspaceId = 'workspacePath' in state ? state.workspacePath : undefined
  const navigation = useSelectionHistory(index, workspaceId)
  const activeKey = navigation.activeKey

  async function checkCurrentWorkspace() {
    await check()
  }

  if (state.status === 'noWorkspace') {
    return <LaunchScreen onOpen={() => void chooseWorkspace()} />
  }

  if (state.status === 'loading' || (state.status === 'error' && !state.index)) {
    return (
      <LaunchScreen
        loading={state.status === 'loading'}
        workspaceName={state.workspaceName}
        error={state.status === 'error' ? state.message : undefined}
        onOpen={() => void chooseWorkspace()}
        onRetry={() => void retry()}
      />
    )
  }

  if (!index || !activeKey) {
    return <LaunchScreen error="The compiled model does not contain a selectable bounded context." onOpen={() => void chooseWorkspace()} />
  }

  const selected = index.selections.get(activeKey)!
  const counts = {
    contexts: index.contexts.length,
    aggregates: index.aggregates.length,
    objects: index.entities.length + index.valueObjects.length + index.domainServices.length,
  }

  return (
    <TooltipProvider delay={300}>
      <SidebarProvider className="dark bg-[#0b0d0e] text-zinc-100" style={{ '--sidebar-width': `${sidebarWidth}px` } as CSSProperties}>
        <AppSidebar tree={index.sidebar} selected={activeKey} onSelect={navigation.navigate} workspaceName={state.workspaceName!} counts={counts} />
        <Workspace
          state={state}
          index={index}
          selected={selected}
          onSelect={navigation.navigate}
          onBack={navigation.back}
          onForward={navigation.forward}
          canGoBack={navigation.canGoBack}
          canGoForward={navigation.canGoForward}
          graphOpen={graphOpen}
          onToggleGraph={toggleGraph}
          onCheck={() => void checkCurrentWorkspace()}
          onRetry={() => void retry()}
        />
        <DomainGraphSidebar index={index} selectedKey={activeKey} onNavigate={navigation.navigate} />
      </SidebarProvider>
    </TooltipProvider>
  )
}

function LaunchScreen({ loading, workspaceName, error, onOpen, onRetry }: {
  loading?: boolean
  workspaceName?: string
  error?: string
  onOpen: () => void
  onRetry?: () => void
}) {
  return (
    <main className="dark grid min-h-svh place-items-center overflow-hidden bg-[#0b0d0e] p-6 text-zinc-100">
      <div className="pointer-events-none absolute inset-0 bg-[radial-gradient(circle_at_50%_25%,rgba(190,242,100,0.08),transparent_35%)]" />
      <Empty className="relative max-w-xl border border-white/10 bg-[#101314]/90 px-8 py-14 shadow-2xl shadow-black/40">
        <EmptyHeader>
          <EmptyMedia variant="icon" className="size-11 bg-lime-300 text-zinc-950">
            {loading ? <Spinner className="size-5" /> : <FolderOpen className="size-5" />}
          </EmptyMedia>
          <EmptyTitle role="heading" aria-level={1} className="text-xl text-zinc-100">{loading ? `Loading ${workspaceName}` : 'Open a domain workspace'}</EmptyTitle>
          <EmptyDescription className="max-w-md text-zinc-500">
            {loading ? 'Compiling the selected package and building its domain index.' : 'Choose a Cargo workspace to inspect its compiled domain model and validate changes.'}
          </EmptyDescription>
        </EmptyHeader>
        <EmptyContent>
          {error && <Alert variant="destructive" className="border-red-400/20 bg-red-400/5"><AlertCircle /><AlertTitle>Workspace unavailable</AlertTitle><AlertDescription>{error}</AlertDescription></Alert>}
          {!loading && (
            <div className="flex gap-2">
              {error && onRetry && <Button variant="outline" onClick={onRetry}><RotateCcw /> Retry</Button>}
              <Button onClick={onOpen} className="bg-lime-300 text-zinc-950 hover:bg-lime-200"><FolderOpen /> Open workspace</Button>
            </div>
          )}
        </EmptyContent>
      </Empty>
    </main>
  )
}

function Workspace({ state, index, selected, onSelect, onBack, onForward, canGoBack, canGoForward, graphOpen, onToggleGraph, onCheck, onRetry }: {
  state: Exclude<WorkspaceState, { status: 'noWorkspace' } | { status: 'loading' }>
  index: DomainIndex
  selected: PresentationSelection
  onSelect: (key: DomainKey) => void
  onBack: () => void
  onForward: () => void
  canGoBack: boolean
  canGoForward: boolean
  graphOpen: boolean
  onToggleGraph: () => void
  onCheck: () => void
  onRetry: () => void
}) {
  const trail = getBreadcrumbTrail(index, selected.key)
  const checking = state.status === 'checking'

  return (
    <SidebarInset className="h-svh min-w-0 overflow-hidden bg-[#0d0f10] text-zinc-100">
      <header aria-label="Workspace header" className="flex h-14 shrink-0 items-center justify-between border-b border-white/10 px-3 lg:px-5">
        <div className="flex min-w-0 items-center gap-2">
          <SidebarTrigger className="-ml-1 text-zinc-400" />
          <div className="flex items-center">
            <Button variant="ghost" size="icon-sm" disabled={!canGoBack} onClick={onBack} aria-label="Go back" title="Go back">
              <ArrowLeft />
            </Button>
            <Button variant="ghost" size="icon-sm" disabled={!canGoForward} onClick={onForward} aria-label="Go forward" title="Go forward">
              <ArrowRight />
            </Button>
          </div>
          <Separator orientation="vertical" className="h-4 bg-white/10" />
          <span className="hidden max-w-36 truncate text-xs font-medium text-zinc-500 sm:inline" title={state.workspaceName}>{state.workspaceName}</span>
          <Separator orientation="vertical" className="mr-1 h-4 bg-white/10" />
          <Breadcrumb>
            <BreadcrumbList className="flex-nowrap overflow-hidden text-xs">
              {trail.map((item, indexInTrail) => {
                const current = indexInTrail === trail.length - 1
                return (
                  <Fragment key={item.key}>
                    {indexInTrail > 0 && <BreadcrumbSeparator className="shrink-0 text-zinc-700" />}
                    <BreadcrumbItem className="min-w-0">
                      {current ? (
                        <BreadcrumbPage title={item.label} className="max-w-56 truncate text-zinc-300">{item.label}</BreadcrumbPage>
                      ) : (
                        <BreadcrumbLink render={<button type="button" aria-label={item.label} title={`Open ${item.label}`} onClick={() => onSelect(item.key)} className="max-w-48 truncate rounded-sm text-zinc-500 outline-none hover:text-zinc-200 focus-visible:ring-2 focus-visible:ring-lime-300/50" />}>{item.label}</BreadcrumbLink>
                      )}
                    </BreadcrumbItem>
                  </Fragment>
                )
              })}
            </BreadcrumbList>
          </Breadcrumb>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={onToggleGraph}
            aria-label={graphOpen ? 'Hide domain graph' : 'Show domain graph'}
            aria-pressed={graphOpen}
            title={graphOpen ? 'Hide domain graph' : 'Show domain graph'}
            className="hidden md:inline-flex"
          >
            <PanelRight />
          </Button>
          <StatusBadge status={state.status} />
          <Button size="sm" disabled={checking} onClick={onCheck} className="bg-lime-300 text-zinc-950 hover:bg-lime-200">
            {checking ? <Spinner data-icon="inline-start" /> : <Play data-icon="inline-start" />}
            {checking ? 'Checking' : 'Compile'}
          </Button>
        </div>
      </header>
      <Inspector index={index} selected={selected} diagnostics={state.diagnostics} error={state.status === 'error' ? state.message : undefined} onRetry={onRetry} onNavigate={onSelect} />
    </SidebarInset>
  )
}

function StatusBadge({ status }: { status: WorkspaceState['status'] }) {
  if (status === 'checking') return <Badge className="hidden border-amber-300/20 bg-amber-300/10 text-amber-200 sm:inline-flex"><Spinner className="size-3" /> Checking</Badge>
  if (status === 'invalid') return <Badge className="hidden border-red-400/20 bg-red-400/10 text-red-300 sm:inline-flex"><XCircle className="size-3" /> Invalid</Badge>
  if (status === 'error') return <Badge className="hidden border-red-400/20 bg-red-400/10 text-red-300 sm:inline-flex"><AlertCircle className="size-3" /> Error</Badge>
  return <Badge className="hidden border-emerald-400/20 bg-emerald-400/10 text-emerald-300 sm:inline-flex"><Check className="size-3" /> Valid</Badge>
}

function Inspector({ index, selected, diagnostics, error, onRetry, onNavigate }: {
  index: DomainIndex
  selected: PresentationSelection
  diagnostics: Diagnostic[]
  error?: string
  onRetry: () => void
  onNavigate: (selection: DomainKey) => void
}) {
  return (
    <aside className="flex size-full min-h-0 flex-col bg-[#101314]">
      <ScrollArea className="min-h-0 flex-1">
        <div className="mx-auto w-full max-w-5xl p-5 lg:p-10">
          {(error || diagnostics.length > 0) && <DiagnosticsPanel diagnostics={diagnostics} error={error} onRetry={onRetry} />}
          <div className="mb-10">
            <div className="mb-4 flex flex-wrap items-center gap-2">
              <Badge className="border-lime-300/20 bg-lime-300/10 text-lime-300">{kindLabel(selected.kind, selected.root)}</Badge>
              {selected.ownerLabel && <><span className="text-xs text-zinc-600">owned by</span><Badge variant="outline" className="border-white/10 text-zinc-400">{selected.ownerLabel}</Badge></>}
            </div>
            <h1 className="text-2xl font-semibold tracking-tight text-zinc-100 lg:text-3xl">{selected.label}</h1>
            <p className="mt-2 font-mono text-sm text-zinc-500">{selected.rustName ?? 'Unavailable in compiled model'}</p>
          </div>
          <Tabs defaultValue="definition">
            <TabsList variant="line" className="mb-7 border-b border-white/10">
              <TabsTrigger value="definition" className="px-3">Data definition</TabsTrigger>
              <TabsTrigger value="behavior" className="px-3">Behavior <Badge className="h-4 min-w-4 bg-white/8 px-1 text-[9px] text-zinc-400">{behaviorCount(selected.behavior)}</Badge></TabsTrigger>
            </TabsList>
            <TabsContent value="definition"><DefinitionView index={index} selected={selected} onNavigate={onNavigate} /></TabsContent>
            <TabsContent value="behavior"><BehaviorView behavior={selected.behavior} lifecycle={selected.lifecycle} onNavigate={onNavigate} /></TabsContent>
          </Tabs>
        </div>
      </ScrollArea>
    </aside>
  )
}

function DiagnosticsPanel({ diagnostics, error, onRetry }: { diagnostics: Diagnostic[]; error?: string; onRetry: () => void }) {
  return (
    <Alert variant="destructive" className="mb-8 border-red-400/20 bg-red-400/5 p-4">
      <AlertCircle />
      <AlertTitle>{error ? 'Workspace check failed' : 'Workspace diagnostics'}</AlertTitle>
      <AlertDescription className="space-y-2">
        {error && <p>{error}</p>}
        {diagnostics.slice(0, 8).map((diagnostic, index) => (
          <p key={`${diagnostic.message}-${index}`}><span className="font-medium uppercase">{diagnostic.level}</span> {diagnostic.message}{diagnostic.file ? ` · ${diagnostic.file}${diagnostic.line ? `:${diagnostic.line}` : ''}` : ''}</p>
        ))}
        {diagnostics.length > 8 && <p>{diagnostics.length - 8} more diagnostics</p>}
        {error && <Button size="sm" variant="outline" onClick={onRetry} className="mt-2 border-red-400/20"><RotateCcw /> Retry</Button>}
      </AlertDescription>
    </Alert>
  )
}

function DefinitionView({ index, selected, onNavigate }: { index: DomainIndex; selected: PresentationSelection; onNavigate: (selection: DomainKey) => void }) {
  return (
    <>
      <section aria-labelledby="data-definition-title">
        <div className="mb-4 flex items-end justify-between gap-4">
          <div><h2 id="data-definition-title" className="text-base font-medium text-zinc-100">Data definition</h2><p className="mt-1 text-sm text-zinc-500">State and relationships represented by this domain object.</p></div>
          <Badge variant="outline" className="border-white/10 font-mono text-[10px] uppercase text-zinc-500">{selected.data.kind}</Badge>
        </div>
        <DefinitionCard definition={selected.data} index={index} onNavigate={onNavigate} />
      </section>
      <Separator className="my-10 bg-white/10" />
      <section aria-labelledby="metadata-title">
        <div className="mb-5"><h2 id="metadata-title" className="text-base font-medium text-zinc-100">Metadata</h2><p className="mt-1 text-sm text-zinc-500">Stable compiler identity and display information.</p></div>
        <FieldGroup className="grid gap-5 md:grid-cols-2">
          <Field><FieldLabel htmlFor="domain-label" className="text-[10px] uppercase tracking-[0.14em] text-zinc-600">Label</FieldLabel><Input id="domain-label" value={selected.label} readOnly className="border-white/10 bg-black/20 text-sm" /></Field>
          <Field><FieldLabel htmlFor="stable-id" className="text-[10px] uppercase tracking-[0.14em] text-zinc-600">Stable ID</FieldLabel><Input id="stable-id" value={selected.stableId} readOnly className="border-white/10 bg-black/20 font-mono text-xs" /></Field>
          <Field><FieldLabel htmlFor="rust-name" className="text-[10px] uppercase tracking-[0.14em] text-zinc-600">Rust name</FieldLabel><Input id="rust-name" value={selected.rustName ?? 'Unavailable in compiled model'} readOnly className="border-white/10 bg-black/20 font-mono text-xs" /></Field>
          <Field><FieldTitle className="text-[10px] uppercase tracking-[0.14em] text-zinc-600">Owner</FieldTitle><Input value={selected.ownerLabel ?? 'Workspace root'} readOnly aria-label="Owner" className="border-white/10 bg-black/20 text-sm" /></Field>
        </FieldGroup>
      </section>
    </>
  )
}

function DefinitionCard({ definition, index, onNavigate }: { definition: DataDefinition; index: DomainIndex; onNavigate: (selection: DomainKey) => void }) {
  if (definition.kind === 'enum') {
    return <Card className="gap-0 bg-black/20 py-0 ring-white/10"><CardHeader className="border-b border-white/10 py-4"><CardTitle className="font-mono text-sm text-zinc-300">enum</CardTitle></CardHeader><CardContent className="divide-y divide-white/10 p-0">{definition.variants.map((variant, index) => <EnumVariant key={`${variant.name}-${index}`} variant={variant} onNavigate={onNavigate} />)}</CardContent></Card>
  }
  if (definition.kind === 'context') {
    return <Empty className="min-h-32 border border-white/10 bg-black/10"><EmptyHeader><EmptyTitle className="text-zinc-400">Bounded context</EmptyTitle><EmptyDescription className="text-zinc-600">Its aggregates, value objects, and services are listed in the domain tree.</EmptyDescription></EmptyHeader></Empty>
  }
  const fields = definition.kind === 'aggregate'
    ? [{ name: 'root', type: { kind: 'reference' as const, name: index.selections.get(definition.rootKey)?.label ?? 'Unresolved aggregate root', key: definition.rootKey } }]
    : definition.fields
  return <Card className="gap-0 bg-black/20 py-0 ring-white/10"><CardContent className="p-0"><PresentationFields fields={fields} onNavigate={onNavigate} /></CardContent></Card>
}

function EnumVariant({ variant, onNavigate }: { variant: PresentationVariant; onNavigate: (selection: DomainKey) => void }) {
  const shapeLabel = ({ unit: 'Unit', tuple: 'Tuple', struct: 'Struct' } as const)[variant.shape]
  return (
    <article aria-label={`${variant.name} ${shapeLabel} variant`} className="bg-[#121516]">
      <div className="flex items-center justify-between gap-3 px-4 py-3">
        <h3 className="font-mono text-sm text-zinc-300">{variant.name}</h3>
        <Badge variant="outline" className="border-white/10 font-mono text-[10px] uppercase text-zinc-500">{shapeLabel}</Badge>
      </div>
      {variant.shape === 'unit'
        ? <p className="border-t border-white/10 px-4 py-4 text-sm text-zinc-600">No payload</p>
        : <PresentationFields fields={variant.fields} onNavigate={onNavigate} emptyMessage={`Empty ${variant.shape} payload`} className="border-t border-white/10" />}
    </article>
  )
}

function behaviorAnchorId(kind: 'action' | 'event' | 'error', key: string) {
  const encoded = Array.from(key, (character) => character.codePointAt(0)!.toString(16).padStart(6, '0')).join('')
  return `behavior-${kind}-${encoded}`
}

function BehaviorView({ behavior, lifecycle, onNavigate }: { behavior: PresentationBehavior; lifecycle?: PresentationLifecycle; onNavigate: (selection: DomainKey) => void }) {
  return <div className="space-y-10">
    <BehaviorSection title="Actions" count={behavior.actions.length} description="Commands that may change the object or produce domain outcomes.">{behavior.actions.length ? <div className="space-y-3">{behavior.actions.map((action) => <ActionCard key={action.id} action={action} onNavigate={onNavigate} />)}</div> : <BehaviorEmpty>No actions are modeled for this object.</BehaviorEmpty>}</BehaviorSection>
    {behavior.domainEvents.length > 0 && <BehaviorSection title="Domain Events" count={behavior.domainEvents.length} description="Facts produced when actions complete successfully."><div className="space-y-3">{behavior.domainEvents.map((event) => <DomainEventCard key={event.key} event={event} onNavigate={onNavigate} />)}</div></BehaviorSection>}
    {behavior.domainErrors.length > 0 && <BehaviorSection title="Domain Errors" count={behavior.domainErrors.length} description="Explicit failures returned by domain actions."><div className="space-y-3">{behavior.domainErrors.map((error) => <DomainErrorCard key={error.key} error={error} onNavigate={onNavigate} />)}</div></BehaviorSection>}
    <BehaviorSection title="Decisions" count={behavior.decisions.length} description="Domain choices implemented from typed input to output.">{behavior.decisions.length ? <div className="space-y-3">{behavior.decisions.map((decision) => <DecisionCard key={decision.id} decision={decision} onNavigate={onNavigate} />)}</div> : <BehaviorEmpty>No decisions are modeled for this object.</BehaviorEmpty>}</BehaviorSection>
    <BehaviorSection title="Queries" count={behavior.queries.length} description="Read-only projections derived from the object state.">{behavior.queries.length ? <div className="space-y-3">{behavior.queries.map((query) => <QueryCard key={query.id} query={query} onNavigate={onNavigate} />)}</div> : <BehaviorEmpty>No queries are modeled for this object.</BehaviorEmpty>}</BehaviorSection>
    <BehaviorSection title="Invariants" count={behavior.invariants.length} description="Rules that must hold for completed candidate state.">{behavior.invariants.length ? <div className="space-y-3">{behavior.invariants.map((invariant) => <Card key={invariant.id} size="sm" className="bg-black/20 ring-white/10"><CardHeader><CardTitle>{invariant.label}</CardTitle><p className="font-mono text-[11px] text-zinc-600">{invariant.id}</p></CardHeader></Card>)}</div> : <BehaviorEmpty>No invariants are modeled for this object.</BehaviorEmpty>}</BehaviorSection>
    {lifecycle && <LifecycleSection lifecycle={lifecycle} />}
  </div>
}

function BehaviorSection({ title, count, description, children }: { title: string; count: number; description: string; children: ReactNode }) {
  const headingId = `behavior-${title.toLowerCase().replaceAll(' ', '-')}`
  return <section aria-labelledby={headingId}><div className="mb-4 flex items-start justify-between gap-4"><div><h2 id={headingId} className="text-base font-medium text-zinc-100">{title}</h2><p className="mt-1 text-sm text-zinc-500">{description}</p></div><Badge variant="outline" className="border-white/10 text-zinc-500">{count}</Badge></div>{children}</section>
}

function ActionCard({ action, onNavigate }: { action: PresentationAction; onNavigate: (selection: DomainKey) => void }) {
  return <Card id={behaviorAnchorId('action', action.id)} tabIndex={-1} className="scroll-mt-6 gap-0 bg-black/20 py-0 outline-none ring-white/10 focus-visible:ring-2 focus-visible:ring-violet-300/50"><CardHeader className="border-b border-white/10 py-4"><div className="flex flex-wrap items-center justify-between gap-3"><div><CardTitle className="text-sm text-zinc-200">{action.label}</CardTitle><p className="mt-1 font-mono text-[11px] text-zinc-600">{action.id}</p></div><div className="flex items-center gap-2"><Badge variant="outline" className="border-violet-300/20 text-violet-300">Action</Badge><Badge variant="outline" className="border-white/10 text-zinc-500">{action.visibility}</Badge></div></div></CardHeader><CardContent className="grid gap-px bg-white/10 p-0 md:grid-cols-3"><ContractValue label="Input" value={action.input} onNavigate={onNavigate} /><ContractValue label="Output" value={action.output} onNavigate={onNavigate} /><ContractValue label="Error" value={action.error} onNavigate={onNavigate} /></CardContent>{action.outcomeLinks.length > 0 && <ActionOutcomes links={action.outcomeLinks} />}</Card>
}

function ActionOutcomes({ links }: { links: PresentationActionOutcomeLink[] }) {
  return <div className="border-t border-white/10 px-4 py-3"><p className="text-[10px] uppercase tracking-[0.14em] text-zinc-600">Outcomes</p><ul className="mt-2 flex flex-wrap gap-2">{links.map((link) => <li key={`${link.kind}-${link.key}`}><a href={`#${behaviorAnchorId(link.kind, link.key)}`} aria-label={`Jump to ${link.kind} ${link.label}`} className="inline-flex items-center gap-2 rounded-md border border-white/10 bg-white/3 px-2.5 py-1.5 text-xs text-zinc-300 outline-none transition-colors hover:bg-white/6 hover:text-zinc-100 focus-visible:ring-2 focus-visible:ring-cyan-300/50"><Badge variant="outline" className={link.kind === 'event' ? 'border-cyan-300/20 text-cyan-300' : 'border-red-300/20 text-red-300'}>{link.kind === 'event' ? 'Event' : 'Error'}</Badge><span>{link.label}</span></a></li>)}</ul></div>
}

function DomainEventCard({ event, onNavigate }: { event: PresentationDomainEvent; onNavigate: (selection: DomainKey) => void }) {
  return <Card id={behaviorAnchorId('event', event.key)} tabIndex={-1} className="scroll-mt-6 gap-0 bg-black/20 py-0 outline-none ring-white/10 focus-visible:ring-2 focus-visible:ring-cyan-300/50"><CardHeader className="border-b border-white/10 py-4"><div className="flex flex-wrap items-center justify-between gap-3"><div><CardTitle className="text-sm text-zinc-200">{event.label}</CardTitle><p className="mt-1 font-mono text-[11px] text-zinc-600">{event.stableId}</p></div><Badge variant="outline" className="border-cyan-300/20 text-cyan-300">Event</Badge></div></CardHeader><CardContent className="p-0"><PresentationFields fields={event.fields} onNavigate={onNavigate} className="border-b border-white/10" /><OutcomeActions label="Produced by" actions={event.producingActions} /></CardContent></Card>
}

function DomainErrorCard({ error, onNavigate }: { error: PresentationDomainError; onNavigate: (selection: DomainKey) => void }) {
  return <Card id={behaviorAnchorId('error', error.key)} tabIndex={-1} className="scroll-mt-6 gap-0 bg-black/20 py-0 outline-none ring-white/10 focus-visible:ring-2 focus-visible:ring-red-300/50"><CardHeader className="border-b border-white/10 py-4"><div className="flex flex-wrap items-center justify-between gap-3"><div><CardTitle className="text-sm text-zinc-200">{error.label}</CardTitle><p className="mt-1 font-mono text-[11px] text-zinc-600">{error.stableId}</p></div><Badge variant="outline" className="border-red-300/20 text-red-300">Error</Badge></div></CardHeader><CardContent className="p-0"><div className="grid gap-px bg-white/10 sm:grid-cols-2"><div className="bg-[#121516] px-4 py-3"><p className="text-[10px] uppercase tracking-[0.14em] text-zinc-600">Code</p><p className="mt-1 font-mono text-xs text-red-200">{error.code}</p></div><div className="bg-[#121516] px-4 py-3"><p className="text-[10px] uppercase tracking-[0.14em] text-zinc-600">Message</p><p className="mt-1 text-sm text-zinc-300">{error.message}</p></div></div><PresentationFields fields={error.fields} onNavigate={onNavigate} className="border-b border-white/10" /><OutcomeActions label="Returned by" actions={error.returningActions} /></CardContent></Card>
}


function OutcomeActions({ label, actions }: { label: 'Produced by' | 'Returned by'; actions: PresentationDomainEvent['producingActions'] }) {
  return <div className="px-4 py-3"><p className="text-[10px] uppercase tracking-[0.14em] text-zinc-600">{label}</p>{actions.length > 0 ? <ul className="mt-2 flex flex-wrap gap-2">{actions.map((action) => <li key={action.id}><a href={`#${behaviorAnchorId('action', action.id)}`} className="inline-flex rounded-md border border-violet-300/20 bg-violet-300/5 px-2.5 py-1.5 text-xs text-violet-300 outline-none transition-colors hover:bg-violet-300/10 hover:text-violet-200 focus-visible:ring-2 focus-visible:ring-violet-300/50">{action.label}</a></li>)}</ul> : <p className="mt-2 text-sm text-zinc-600">No actions</p>}</div>
}

function DecisionCard({ decision, onNavigate }: { decision: PresentationDecision; onNavigate: (selection: DomainKey) => void }) {
  return <Card className="gap-0 bg-black/20 py-0 ring-white/10"><CardHeader className="border-b border-white/10 py-4"><div className="flex flex-wrap items-center justify-between gap-3"><div><CardTitle className="text-sm text-zinc-200">{decision.label}</CardTitle><p className="mt-1 font-mono text-[11px] text-zinc-600">{decision.id}</p></div><div className="flex items-center gap-2"><Badge variant="outline" className="border-amber-300/20 text-amber-200">Decision</Badge><Badge variant="outline" className="border-white/10 text-zinc-400">{decision.implementation.kind === 'rust' ? 'Rust' : decision.implementation.kind}</Badge></div></div></CardHeader><CardContent className="grid gap-px bg-white/10 p-0 sm:grid-cols-2"><ContractValue label="Input" value={decision.input} onNavigate={onNavigate} /><ContractValue label="Output" value={decision.output} onNavigate={onNavigate} /></CardContent></Card>
}

function QueryCard({ query, onNavigate }: { query: PresentationQuery; onNavigate: (selection: DomainKey) => void }) {
  return <Card className="gap-0 bg-black/20 py-0 ring-white/10"><CardHeader className="border-b border-white/10 py-4"><div className="flex flex-wrap items-center justify-between gap-3"><div><CardTitle className="text-sm text-zinc-200">{query.label}</CardTitle><p className="mt-1 font-mono text-[11px] text-zinc-600">{query.id}</p></div><Badge variant="outline" className="border-cyan-300/20 text-cyan-300">Query</Badge></div></CardHeader><CardContent className="grid gap-px bg-white/10 p-0 sm:grid-cols-2"><ContractValue label="Input" value={query.input} onNavigate={onNavigate} /><ContractValue label="Output" value={query.output} onNavigate={onNavigate} /></CardContent></Card>
}

function LifecycleSection({ lifecycle }: { lifecycle: PresentationLifecycle }) {
  return <section aria-labelledby="behavior-lifecycle"><div className="mb-4"><h2 id="behavior-lifecycle" className="text-base font-medium text-zinc-100">Lifecycle</h2><p className="mt-1 text-sm text-zinc-500">States and action-driven transitions for this object.</p></div><Card className="gap-0 bg-black/20 py-0 ring-white/10"><CardHeader className="border-b border-white/10 py-4"><CardTitle className="text-sm text-zinc-200">{lifecycle.label}</CardTitle><p className="font-mono text-[11px] text-zinc-600">{lifecycle.id}</p></CardHeader><CardContent className="grid gap-0 p-0 md:grid-cols-2"><div className="border-b border-white/10 p-4 md:border-r md:border-b-0"><h3 className="text-[10px] uppercase tracking-[0.14em] text-zinc-600">States</h3><ul className="mt-3 space-y-2">{lifecycle.states.map((state) => <li key={state.id} className="flex items-center justify-between gap-3 rounded-md border border-white/10 bg-[#121516] px-3 py-2"><div className="min-w-0"><p className="truncate text-sm text-zinc-300">{state.label}</p><p className="truncate font-mono text-[10px] text-zinc-600">{state.id}</p></div>{state.id === lifecycle.initial.id && <Badge className="border-lime-300/20 bg-lime-300/10 text-lime-300">Initial</Badge>}</li>)}</ul></div><div className="p-4"><h3 className="text-[10px] uppercase tracking-[0.14em] text-zinc-600">Transitions</h3>{lifecycle.transitions.length ? <ol className="mt-3 space-y-2">{lifecycle.transitions.map((transition, index) => <li key={`${transition.source.id}-${transition.action.id}-${transition.target.id}-${index}`} className="flex flex-wrap items-center gap-2 rounded-md border border-white/10 bg-[#121516] px-3 py-2 text-sm"><span className="text-zinc-300">{transition.source.label}</span><ArrowRight aria-hidden="true" className="size-3.5 text-zinc-600" /><span className="text-violet-300">{transition.action.label}</span><ArrowRight aria-hidden="true" className="size-3.5 text-zinc-600" /><span className="text-zinc-300">{transition.target.label}</span></li>)}</ol> : <p className="mt-3 text-sm text-zinc-600">No lifecycle transitions are modeled.</p>}</div></CardContent></Card></section>
}

function ContractValue({ label, value, onNavigate }: { label: string; value: DisplayType | null; onNavigate: (selection: DomainKey) => void }) {
  return <div className="min-w-0 bg-[#121516] px-4 py-3"><p className="text-[10px] uppercase tracking-[0.14em] text-zinc-600">{label}</p><div className={`mt-1 font-mono text-xs ${value ? 'text-cyan-200' : 'text-zinc-700'}`}>{value ? <DisplayTypeView type={value} onNavigate={onNavigate} /> : 'None'}</div></div>
}

function BehaviorEmpty({ children }: { children: ReactNode }) {
  return <Empty className="min-h-28 border border-white/10 bg-black/10"><EmptyHeader><EmptyTitle className="text-zinc-400">Nothing modeled</EmptyTitle><EmptyDescription className="text-zinc-600">{children}</EmptyDescription></EmptyHeader></Empty>
}

function behaviorCount(behavior: PresentationBehavior) { return behavior.actions.length + behavior.decisions.length + behavior.queries.length + behavior.invariants.length }

function kindLabel(kind: SelectionKind, root = false): string {
  if (kind === 'entity' && root) return 'Root Entity'
  return ({ context: 'Bounded Context', aggregate: 'Aggregate', entity: 'Entity', identity: 'Identity', valueObject: 'Value Object', domainService: 'Domain Service' })[kind]
}

export default App
