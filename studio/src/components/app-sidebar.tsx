import { useState } from 'react'
import {
  Box,
  Boxes,
  Braces,
  ChevronRight,
  Fingerprint,
  Gem,
  Network,
  Wrench,
  type LucideIcon,
} from 'lucide-react'

import { SidebarResizeHandle } from '@/components/sidebar-resize-handle'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarMenuSub,
} from '@/components/ui/sidebar'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import type { DomainKey, SelectionKind, SidebarTreeNode } from '@/domain/index'

export type SidebarCounts = { contexts: number; aggregates: number; objects: number }

const icons: Record<Exclude<SelectionKind, 'identity'>, LucideIcon> = {
  context: Network,
  aggregate: Boxes,
  entity: Box,
  valueObject: Gem,
  domainService: Wrench,
}

export function AppSidebar({
  tree,
  selected,
  onSelect,
  workspaceName,
  counts,
}: {
  tree: SidebarTreeNode[]
  selected: DomainKey | null
  onSelect: (selection: DomainKey) => void
  workspaceName: string
  counts: SidebarCounts
}) {
  return (
    <Sidebar collapsible="offcanvas" className="border-white/10 bg-[#101314]">
      <SidebarHeader className="border-b border-sidebar-border p-3">
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton size="lg" className="hover:bg-transparent active:bg-transparent">
              <div className="grid size-8 shrink-0 place-items-center rounded-md bg-lime-300 text-zinc-950">
                <Braces className="size-4" strokeWidth={2.5} />
              </div>
              <span className="grid min-w-0 flex-1 text-left leading-tight">
                <span className="truncate text-sm font-semibold">Rostfrei Studio</span>
                <span className="truncate text-[10px] uppercase tracking-[0.16em] text-sidebar-foreground/45">{workspaceName}</span>
              </span>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarHeader>
      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupContent>
            <SidebarMenu role="tree" aria-label="Domain model">
              {tree.map((node) => <DomainTree key={node.key} node={node} selected={selected} onSelect={onSelect} />)}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>
      <SidebarFooter className="border-t border-sidebar-border">
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton className="text-sidebar-foreground/60">
              <Fingerprint />
              <span>{counts.contexts} {counts.contexts === 1 ? 'context' : 'contexts'} · {counts.aggregates} {counts.aggregates === 1 ? 'aggregate' : 'aggregates'} · {counts.objects} objects</span>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarFooter>
      <SidebarResizeHandle />
    </Sidebar>
  )
}

function DomainTree({ node, selected, onSelect }: {
  node: SidebarTreeNode
  selected: DomainKey | null
  onSelect: (selection: DomainKey) => void
}) {
  const [open, setOpen] = useState(true)
  const Icon = icons[node.kind]
  const active = node.key === selected
  const content = <><DomainIcon icon={Icon} kind={kindLabel(node.kind)} root={node.root} /><span className="min-w-0 flex-1 break-words leading-4">{node.label}</span></>

  if (!node.children.length) {
    return (
      <SidebarMenuItem>
        <SidebarMenuButton role="treeitem" isActive={active} tooltip={node.label} onClick={() => onSelect(node.key)} className="h-auto min-h-8 items-start py-1.5 [&>span:last-child]:overflow-visible [&>span:last-child]:text-clip [&>span:last-child]:whitespace-normal">
          {content}
        </SidebarMenuButton>
      </SidebarMenuItem>
    )
  }

  return (
    <SidebarMenuItem>
      <Collapsible open={open} onOpenChange={setOpen}>
        <CollapsibleTrigger render={<SidebarMenuButton role="treeitem" aria-expanded={open} isActive={active} tooltip={node.label} onClick={() => onSelect(node.key)} className="h-auto min-h-8 items-start py-1.5 [&>span:last-child]:overflow-visible [&>span:last-child]:text-clip [&>span:last-child]:whitespace-normal" />}>
          <ChevronRight className={`mt-0.5 transition-transform duration-150 ${open ? 'rotate-90' : ''}`} />
          {content}
        </CollapsibleTrigger>
        <CollapsibleContent>
          <SidebarMenuSub role="group" className="mx-2 px-2">
            {node.children.map((child) => <DomainTree key={child.key} node={child} selected={selected} onSelect={onSelect} />)}
          </SidebarMenuSub>
        </CollapsibleContent>
      </Collapsible>
    </SidebarMenuItem>
  )
}

function kindLabel(kind: SelectionKind): string {
  return ({ context: 'Bounded Context', aggregate: 'Aggregate', entity: 'Entity', identity: 'Identity', valueObject: 'Value Object', domainService: 'Domain Service' })[kind]
}

function DomainIcon({ icon: Icon, kind, root }: { icon: LucideIcon; kind: string; root: boolean }) {
  return (
    <Tooltip>
      <TooltipTrigger render={<span className="relative mt-0.5 inline-flex shrink-0 text-sidebar-foreground/70" />}>
        <Icon />
        {root && <span aria-hidden="true" className="absolute -right-0.5 -bottom-0.5 size-1.5 rounded-full bg-lime-300 ring-2 ring-sidebar" />}
      </TooltipTrigger>
      <TooltipContent side="right">{kind}{root ? ' · Aggregate root' : ''}</TooltipContent>
    </Tooltip>
  )
}
