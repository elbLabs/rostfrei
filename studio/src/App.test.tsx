import { fireEvent, render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { DomainModel } from './domain/schema'
import App from './App'
import {
  DEFAULT_GRAPH_SIDEBAR_WIDTH,
  DEFAULT_SIDEBAR_WIDTH,
  useSidebarLayoutStore,
} from './stores/sidebar-layout'

const native = vi.hoisted(() => ({
  open: vi.fn(),
  invoke: vi.fn(),
}))

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: native.open }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: native.invoke }))

const orders = { context: 'commerce', local: 'orders' }
const order = { aggregate: orders, local: 'order' }
const line = { aggregate: orders, local: 'line' }
const orderIdentity = { owner: order }
const lineIdentity = { owner: line }
const status = { owner: { kind: 'aggregate' as const, id: orders }, local: 'status' }
const note = { owner: { kind: 'entity' as const, id: line }, local: 'note' }
const archiveLine = { owner: { kind: 'entity' as const, id: line }, local: 'archive' }
const transition = { owner: { kind: 'aggregate' as const, id: orders }, local: 'transition' }
const uuidScalar = { kind: 'semantic' as const, id: 'uuid', label: 'UUID', representation: 'string' as const }

const domainModel: DomainModel = {
  boundedContexts: [{ id: 'commerce', label: 'Commerce' }],
  aggregates: [{ id: orders, label: 'Order Management', root: order }],
  entities: [
    {
      id: order,
      label: 'Customer Order',
      identity: { field: 'id', id: orderIdentity },
      fields: [
        { name: 'id', value: { kind: 'identity', id: orderIdentity } },
        { name: 'lines', value: { kind: 'list', element: { kind: 'entity', id: line } } },
        { name: 'status', value: { kind: 'optional', value: { kind: 'valueObject', id: status } } },
        { name: 'externalId', value: { kind: 'optional', value: { kind: 'scalar', scalar: uuidScalar } } },
      ],
    },
    {
      id: line,
      label: 'Order Line',
      identity: { field: 'id', id: lineIdentity },
      fields: [{ name: 'note', value: { kind: 'valueObject', id: note } }],
      lifecycle: {
        id: 'orderLineLifecycle',
        label: 'Order line lifecycle',
        states: [
          { id: 'active', label: 'Active' },
          { id: 'archived', label: 'Archived' },
        ],
        initial: 'active',
        transitions: [{ source: 'active', action: archiveLine, target: 'archived' }],
      },
    },
  ],
  domainIdentities: [
    { id: orderIdentity, scalar: uuidScalar },
    { id: lineIdentity, scalar: 'u64' },
  ],
  valueObjects: [
    { id: status, label: 'Order Status', variants: ['Draft', 'Submitted'] },
    { id: note, label: 'Line Note', fields: [{ name: 'text', value: { kind: 'scalar', scalar: 'string' } }] },
  ],
  domainServices: [{ id: { context: 'commerce', local: 'pricing' }, label: 'Pricing Service' }],
  domainCommands: [{ id: { owner: { kind: 'aggregate', id: orders }, local: 'submit' }, label: 'Submit Order', fields: [] }],
  domainEvents: [{
    id: { aggregate: orders, local: 'submitted' },
    label: 'Order Submitted',
    fields: [
      { name: 'orderId', value: { kind: 'identity', id: orderIdentity } },
      { name: 'statusHistory', value: { kind: 'list', element: {
        kind: 'optional', value: { kind: 'valueObject', id: status },
      } } },
    ],
  }],
  domainErrors: [{
    id: { owner: { kind: 'aggregate', id: orders }, local: 'invalid' },
    label: 'Invalid Order',
    code: 'INVALID',
    message: 'The order cannot be submitted.',
    fields: [
      { name: 'reason', value: { kind: 'scalar', scalar: 'string' } },
      { name: 'lastStatus', value: { kind: 'optional', value: { kind: 'valueObject', id: status } } },
    ],
  }],
  actions: [{
    id: { owner: { kind: 'aggregate', id: orders }, local: 'submit' },
    label: 'Submit customer order',
    input: { kind: 'domainCommand', id: { owner: { kind: 'aggregate', id: orders }, local: 'submit' } },
    output: { kind: 'domainEvent', id: { aggregate: orders, local: 'submitted' } },
    error: { owner: { kind: 'aggregate', id: orders }, local: 'invalid' },
  }, {
    id: archiveLine,
    label: 'Archive order line',
    input: null,
    output: null,
    error: null,
  }],
  decisions: [{
    id: { owner: { kind: 'aggregate', id: orders }, local: 'routeStatus' },
    label: 'Route order status',
    input: { kind: 'valueObject', id: status },
    output: { kind: 'valueObject', id: note },
    implementation: { kind: 'rust' },
  }],
  queries: [{
    id: { aggregate: orders, local: 'status' },
    label: 'Current status',
    input: { kind: 'domainIdentity', id: orderIdentity },
    output: { kind: 'optional', value: { kind: 'valueObject', id: status } },
  }],
  invariants: [{ id: { owner: { kind: 'aggregate', id: orders }, local: 'hasLines' }, label: 'Has at least one line' }],
}

function taggedDomainModel(): DomainModel {
  const result = structuredClone(domainModel)
  result.valueObjects.push({
    id: transition,
    label: 'Order Transition',
    variants: ['None', 'EmptyTuple', 'EmptyStruct', 'Values', 'Moved'],
    variantShapes: [
      { name: 'None', kind: 'unit' },
      { name: 'EmptyTuple', kind: 'tuple', fields: [] },
      { name: 'EmptyStruct', kind: 'struct', fields: [] },
      { name: 'Values', kind: 'tuple', fields: [
        { name: '0', value: { kind: 'scalar', scalar: 'string' } },
        { name: '1', value: { kind: 'scalar', scalar: uuidScalar } },
        { name: '2', value: { kind: 'optional', value: { kind: 'valueObject', id: status } } },
      ] },
      { name: 'Moved', kind: 'struct', fields: [
        { name: 'line', value: { kind: 'entity', id: line } },
      ] },
    ],
  })
  return result
}

function successfulNativeCalls(model = domainModel) {
  native.open.mockResolvedValue('/work/acme-shop')
  native.invoke.mockImplementation((command: string) => {
    if (command === 'load_domain_model') return Promise.resolve(model)
    if (command === 'check_workspace') return Promise.resolve({ success: true, diagnostics: [] })
    return Promise.reject(new Error(`Unexpected command: ${command}`))
  })
}

async function openWorkspace(user = userEvent.setup()) {
  await user.click(screen.getByRole('button', { name: 'Open workspace' }))
  await screen.findByRole('tree', { name: 'Domain model' })
  return user
}

describe('Rostfrei Studio', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    successfulNativeCalls()
    useSidebarLayoutStore.getState().resetWidth()
    useSidebarLayoutStore.getState().resetGraphWidth()
    useSidebarLayoutStore.getState().setGraphOpen(true)
  })

  it('starts without opening a hardcoded workspace', () => {
    render(<App />)
    expect(screen.getByRole('heading', { name: 'Open a domain workspace' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Open workspace' })).toBeInTheDocument()
    expect(native.open).not.toHaveBeenCalled()
    expect(native.invoke).not.toHaveBeenCalled()
  })

  it('shows loading and then the compiled model with dynamic workspace data', async () => {
    let resolveModel!: (model: DomainModel) => void
    native.invoke.mockImplementation(() => new Promise((resolve) => { resolveModel = resolve }))
    const user = userEvent.setup()
    render(<App />)

    await user.click(screen.getByRole('button', { name: 'Open workspace' }))
    expect(await screen.findByRole('heading', { name: 'Loading acme-shop' })).toBeInTheDocument()
    expect(native.open).toHaveBeenCalledWith({ directory: true, multiple: false })
    expect(native.invoke).toHaveBeenCalledWith('load_domain_model', { workspacePath: '/work/acme-shop', package: 'bike-rental' })

    resolveModel(domainModel)
    const tree = await screen.findByRole('tree', { name: 'Domain model' })
    expect(within(tree).getByText('Commerce')).toBeInTheDocument()
    expect(within(tree).getByText('Order Management')).toBeInTheDocument()
    expect(screen.getAllByText('acme-shop')).toHaveLength(2)
    expect(screen.getByText('1 context · 1 aggregate · 5 objects')).toBeInTheDocument()
  })

  it('renders real definitions, linked types, and breadcrumbs', async () => {
    const user = userEvent.setup()
    render(<App />)
    await openWorkspace(user)
    const tree = screen.getByRole('tree', { name: 'Domain model' })

    await user.click(within(tree).getByRole('treeitem', { name: 'Customer Order' }))
    expect(screen.getByRole('heading', { name: 'Customer Order' })).toBeInTheDocument()
    expect(screen.getByText('Unavailable in compiled model')).toBeInTheDocument()
    const definition = screen.getAllByRole('tabpanel').find((panel) => !panel.hasAttribute('inert'))!
    expect(within(definition).getByText('lines')).toBeInTheDocument()
    expect(within(definition).getByLabelText('Vec<Order Line>')).toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: 'Open Order Line' }))
    await user.click(screen.getByRole('button', { name: 'Open Line Note' }))
    expect(screen.getByRole('heading', { name: 'Line Note' })).toBeInTheDocument()
    const header = screen.getByRole('banner', { name: 'Workspace header' })
    expect(within(header).getByRole('button', { name: 'Order Line' })).toBeInTheDocument()
    await user.click(within(header).getByRole('button', { name: 'Order Management' }))
    expect(screen.getByRole('heading', { name: 'Order Management' })).toBeInTheDocument()
  })

  it('renders tagged variant shapes, payload fields, types, and navigable references', async () => {
    successfulNativeCalls(taggedDomainModel())
    const user = userEvent.setup()
    render(<App />)
    await openWorkspace(user)
    const tree = screen.getByRole('tree', { name: 'Domain model' })

    await user.click(within(tree).getByRole('treeitem', { name: 'Order Transition' }))

    const unit = screen.getByRole('article', { name: 'None Unit variant' })
    const emptyTuple = screen.getByRole('article', { name: 'EmptyTuple Tuple variant' })
    const emptyStruct = screen.getByRole('article', { name: 'EmptyStruct Struct variant' })
    const values = screen.getByRole('article', { name: 'Values Tuple variant' })
    const moved = screen.getByRole('article', { name: 'Moved Struct variant' })

    expect(within(unit).getByText('Unit')).toBeInTheDocument()
    expect(within(unit).getByText('No payload')).toBeInTheDocument()
    expect(within(emptyTuple).getByText('Tuple')).toBeInTheDocument()
    expect(within(emptyTuple).getByText('Empty tuple payload')).toBeInTheDocument()
    expect(within(emptyStruct).getByText('Struct')).toBeInTheDocument()
    expect(within(emptyStruct).getByText('Empty struct payload')).toBeInTheDocument()
    expect(within(values).getByText('0')).toBeInTheDocument()
    expect(within(values).getByLabelText('string')).toBeInTheDocument()
    expect(within(values).getByText('1')).toBeInTheDocument()
    expect(within(values).getByLabelText('UUID (represented as string)')).toBeInTheDocument()
    expect(within(values).getByText('2')).toBeInTheDocument()
    expect(within(values).getByLabelText('Option<Order Status>')).toBeInTheDocument()
    expect(within(moved).getByText('line')).toBeInTheDocument()

    await user.click(within(moved).getByRole('button', { name: 'Open Order Line' }))
    expect(screen.getByRole('heading', { name: 'Order Line' })).toBeInTheDocument()
  })

  it('renders legacy enum variants as unit variants', async () => {
    const user = userEvent.setup()
    render(<App />)
    await openWorkspace(user)
    const tree = screen.getByRole('tree', { name: 'Domain model' })

    await user.click(within(tree).getByRole('treeitem', { name: 'Order Status' }))

    expect(screen.getByRole('article', { name: 'Draft Unit variant' })).toHaveTextContent('No payload')
    expect(screen.getByRole('article', { name: 'Submitted Unit variant' })).toHaveTextContent('No payload')
  })

  it('renders semantic scalar labels and representation metadata without changing canonical scalars', async () => {
    const user = userEvent.setup()
    render(<App />)
    await openWorkspace(user)
    const tree = screen.getByRole('tree', { name: 'Domain model' })

    await user.click(within(tree).getByRole('treeitem', { name: 'Customer Order' }))
    const definition = screen.getAllByRole('tabpanel').find((panel) => !panel.hasAttribute('inert'))!
    const semantic = within(definition).getByText('UUID')
    expect(within(definition).getByLabelText('Option<UUID (represented as string)>')).toBeInTheDocument()
    expect(semantic).toHaveAttribute('aria-label', 'UUID (represented as string)')
    expect(semantic).toHaveAttribute('title', 'Semantic scalar UUID (uuid), represented as string')

    await user.click(within(tree).getByRole('treeitem', { name: 'Line Note' }))
    const canonical = screen.getByLabelText('string')
    expect(canonical).toHaveTextContent('string')
    expect(canonical).toHaveAttribute('title', 'Scalar')
  })

  it('renders semantic scalar identity definitions', async () => {
    const user = userEvent.setup()
    render(<App />)
    await openWorkspace(user)
    const tree = screen.getByRole('tree', { name: 'Domain model' })

    await user.click(within(tree).getByRole('treeitem', { name: 'Customer Order' }))
    await user.click(screen.getByRole('button', { name: 'Open Identity of Customer Order' }))

    expect(screen.getByRole('heading', { name: 'Identity of Customer Order' })).toBeInTheDocument()
    const semantic = screen.getByLabelText('UUID (represented as string)')
    expect(semantic).toHaveTextContent('UUID')
    expect(semantic).toHaveAttribute('title', 'Semantic scalar UUID (uuid), represented as string')
  })

  it('navigates backward and forward through object history', async () => {
    const user = userEvent.setup()
    render(<App />)
    await openWorkspace(user)
    const tree = screen.getByRole('tree', { name: 'Domain model' })
    const back = screen.getByRole('button', { name: 'Go back' })
    const forward = screen.getByRole('button', { name: 'Go forward' })

    expect(back).toBeDisabled()
    expect(forward).toBeDisabled()

    await user.click(within(tree).getByRole('treeitem', { name: 'Customer Order' }))
    await user.click(screen.getByRole('button', { name: 'Open Order Line' }))
    expect(screen.getByRole('heading', { name: 'Order Line' })).toBeInTheDocument()

    await user.click(back)
    expect(screen.getByRole('heading', { name: 'Customer Order' })).toBeInTheDocument()
    expect(forward).toBeEnabled()

    await user.click(back)
    expect(screen.getByRole('heading', { name: 'Order Management' })).toBeInTheDocument()

    await user.click(forward)
    expect(screen.getByRole('heading', { name: 'Customer Order' })).toBeInTheDocument()
  })

  it('shows behavior generated for the selected owner', async () => {
    const user = userEvent.setup()
    render(<App />)
    await openWorkspace(user)
    await user.click(screen.getByRole('tab', { name: /Behavior/ }))
    const behavior = screen.getAllByRole('tabpanel').find((panel) => !panel.hasAttribute('inert'))!

    expect(within(behavior).getByText('Submit customer order', { selector: '[data-slot="card-title"]' })).toBeInTheDocument()
    expect(within(behavior).getByText('Submit Order')).toBeInTheDocument()
    expect(within(behavior).getByText('Current status')).toBeInTheDocument()
    const queries = within(behavior).getByRole('heading', { name: 'Queries' }).closest('section') as HTMLElement
    expect(within(queries).getByLabelText('Option<Order Status>')).toBeInTheDocument()
    expect(within(behavior).getByText('Has at least one line')).toBeInTheDocument()
  })

  it('renders aggregate outcomes with bidirectional action links without changing behavior or sidebar counts', async () => {
    const user = userEvent.setup()
    render(<App />)
    await openWorkspace(user)
    const tree = screen.getByRole('tree', { name: 'Domain model' })

    expect(within(tree).getAllByRole('treeitem')).toHaveLength(7)
    expect(within(tree).queryByText('Order Submitted')).not.toBeInTheDocument()
    expect(within(tree).queryByText('Invalid Order')).not.toBeInTheDocument()

    const behaviorTab = screen.getByRole('tab', { name: 'Behavior 4' })
    await user.click(behaviorTab)
    const behavior = screen.getAllByRole('tabpanel').find((panel) => !panel.hasAttribute('inert'))!
    const actionCard = within(behavior)
      .getByText('Submit customer order', { selector: '[data-slot="card-title"]' })
      .closest('[id]') as HTMLElement
    const eventSection = within(behavior).getByRole('heading', { name: 'Domain Events' }).closest('section') as HTMLElement
    const errorSection = within(behavior).getByRole('heading', { name: 'Domain Errors' }).closest('section') as HTMLElement
    const eventCard = within(eventSection)
      .getByText('Order Submitted', { selector: '[data-slot="card-title"]' })
      .closest('[id]') as HTMLElement
    const errorCard = within(errorSection)
      .getByText('Invalid Order', { selector: '[data-slot="card-title"]' })
      .closest('[id]') as HTMLElement

    expect(within(eventCard).getByText('submitted')).toBeInTheDocument()
    expect(within(eventCard).getByText('orderId')).toBeInTheDocument()
    expect(within(eventCard).getByRole('button', { name: 'Open Identity of Customer Order' })).toBeInTheDocument()
    expect(within(eventCard).getByText('statusHistory')).toBeInTheDocument()
    expect(within(eventCard).getByLabelText('Vec<Option<Order Status>>')).toBeInTheDocument()
    expect(within(eventCard).getByText('Produced by')).toBeInTheDocument()

    expect(within(errorCard).getByText('invalid')).toBeInTheDocument()
    expect(within(errorCard).getByText('INVALID')).toBeInTheDocument()
    expect(within(errorCard).getByText('The order cannot be submitted.')).toBeInTheDocument()
    expect(within(errorCard).getByText('reason')).toBeInTheDocument()
    expect(within(errorCard).getByLabelText('string')).toBeInTheDocument()
    expect(within(errorCard).getByText('lastStatus')).toBeInTheDocument()
    expect(within(errorCard).getByLabelText('Option<Order Status>')).toBeInTheDocument()
    expect(within(errorCard).getByText('Returned by')).toBeInTheDocument()

    expect(within(actionCard).getByRole('link', { name: 'Jump to event Order Submitted' }))
      .toHaveAttribute('href', `#${eventCard.id}`)
    expect(within(actionCard).getByRole('link', { name: 'Jump to error Invalid Order' }))
      .toHaveAttribute('href', `#${errorCard.id}`)
    expect(within(eventCard).getByRole('link', { name: 'Submit customer order' }))
      .toHaveAttribute('href', `#${actionCard.id}`)
    expect(within(errorCard).getByRole('link', { name: 'Submit customer order' }))
      .toHaveAttribute('href', `#${actionCard.id}`)

    await user.click(within(tree).getByRole('treeitem', { name: 'Customer Order' }))
    await user.click(screen.getByRole('tab', { name: 'Behavior 0' }))
    expect(screen.queryByRole('heading', { name: 'Domain Events' })).not.toBeInTheDocument()
    expect(screen.queryByRole('heading', { name: 'Domain Errors' })).not.toBeInTheDocument()
    expect(within(tree).getAllByRole('treeitem')).toHaveLength(7)
  })

  it('renders Rust decisions and includes them in the behavior count', async () => {
    const user = userEvent.setup()
    render(<App />)
    await openWorkspace(user)
    const behaviorTab = screen.getByRole('tab', { name: 'Behavior 4' })
    await user.click(behaviorTab)
    const behavior = screen.getAllByRole('tabpanel').find((panel) => !panel.hasAttribute('inert'))!
    const decisions = within(behavior).getByRole('heading', { name: 'Decisions' }).closest('section')!

    expect(within(decisions).getByText('Route order status')).toBeInTheDocument()
    expect(within(decisions).getByText('Decision')).toBeInTheDocument()
    expect(within(decisions).getByText('Rust')).toBeInTheDocument()
    expect(within(decisions).getByText('Input')).toBeInTheDocument()
    expect(within(decisions).getByText('Order Status')).toBeInTheDocument()
    expect(within(decisions).getByText('Output')).toBeInTheDocument()
    expect(within(decisions).getByText('Line Note')).toBeInTheDocument()
  })

  it('renders entity lifecycle states and transitions only for lifecycle selections', async () => {
    const user = userEvent.setup()
    render(<App />)
    await openWorkspace(user)
    const tree = screen.getByRole('tree', { name: 'Domain model' })
    await user.click(within(tree).getByRole('treeitem', { name: 'Order Line' }))
    await user.click(screen.getByRole('tab', { name: /Behavior/ }))
    const behavior = screen.getAllByRole('tabpanel').find((panel) => !panel.hasAttribute('inert'))!
    const lifecycle = within(behavior).getByRole('heading', { name: 'Lifecycle' }).closest('section')!

    expect(within(lifecycle).getByText('Order line lifecycle')).toBeInTheDocument()
    expect(within(lifecycle).getAllByText('Active')).toHaveLength(2)
    expect(within(lifecycle).getAllByText('Archived')).toHaveLength(2)
    expect(within(lifecycle).getByText('Initial')).toBeInTheDocument()
    expect(within(lifecycle).getByText('Archive order line')).toBeInTheDocument()

    await user.click(within(tree).getByRole('treeitem', { name: 'Customer Order' }))
    expect(screen.getByRole('heading', { name: 'Customer Order' })).toBeInTheDocument()
    expect(screen.queryByRole('heading', { name: 'Lifecycle' })).not.toBeInTheDocument()
  })

  it('checks successfully, reloads the model, and reports Valid', async () => {
    const user = userEvent.setup()
    render(<App />)
    await openWorkspace(user)
    await user.click(screen.getByRole('button', { name: 'Compile' }))

    expect(await screen.findByText('Valid')).toBeInTheDocument()
    expect(native.invoke).toHaveBeenCalledWith('check_workspace', { workspacePath: '/work/acme-shop' })
    expect(native.invoke.mock.calls.filter(([command]) => command === 'load_domain_model')).toHaveLength(2)
  })

  it('retains the tree and shows diagnostics after an invalid check', async () => {
    native.invoke.mockImplementation((command: string) => {
      if (command === 'load_domain_model') return Promise.resolve(domainModel)
      return Promise.resolve({ success: false, diagnostics: [{ level: 'error', message: 'missing field `total`', file: 'src/order.rs', line: 19 }] })
    })
    const user = userEvent.setup()
    render(<App />)
    await openWorkspace(user)
    await user.click(screen.getByRole('button', { name: 'Compile' }))

    expect(await screen.findByText('Invalid')).toBeInTheDocument()
    expect(screen.getByText('Workspace diagnostics')).toBeInTheDocument()
    expect(screen.getByText(/missing field `total`/)).toBeInTheDocument()
    expect(screen.getByRole('tree', { name: 'Domain model' })).toBeInTheDocument()
  })

  it('collapses and expands the generated hierarchy', async () => {
    const user = userEvent.setup()
    render(<App />)
    await openWorkspace(user)
    const tree = screen.getByRole('tree', { name: 'Domain model' })
    const aggregate = within(tree).getByRole('treeitem', { name: 'Order Management' })
    expect(aggregate).toHaveAttribute('aria-expanded', 'true')

    await user.click(aggregate)
    expect(aggregate).toHaveAttribute('aria-expanded', 'false')
    expect(within(tree).queryByText('Customer Order')).not.toBeInTheDocument()
  })

  it('closes, reopens, and navigates from the domain graph sidebar', async () => {
    const user = userEvent.setup()
    render(<App />)
    await openWorkspace(user)

    expect(screen.getByLabelText('Domain graph sidebar')).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: 'Close domain graph' }))
    expect(screen.queryByLabelText('Domain graph sidebar')).not.toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: 'Show domain graph' }))
    const graph = screen.getByLabelText('Domain graph sidebar')
    fireEvent.click(await within(graph).findByRole('button', { name: /Order Line, entity/ }))

    expect(screen.getByRole('heading', { name: 'Order Line' })).toBeInTheDocument()
  })

  it('resizes the loaded sidebar with the keyboard', async () => {
    const user = userEvent.setup()
    render(<App />)
    await openWorkspace(user)
    screen.getByRole('separator', { name: 'Resize sidebar' }).focus()
    await user.keyboard('{ArrowRight}')
    expect(useSidebarLayoutStore.getState().width).toBe(DEFAULT_SIDEBAR_WIDTH + 12)

    screen.getByRole('separator', { name: 'Resize domain graph' }).focus()
    await user.keyboard('{ArrowLeft}')
    expect(useSidebarLayoutStore.getState().graphWidth).toBe(DEFAULT_GRAPH_SIDEBAR_WIDTH + 12)
  })
})
