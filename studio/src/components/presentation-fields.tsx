import { ArrowUpRight } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import type { DisplayType, DomainKey, PresentationField } from '@/domain/index'

export function PresentationFields({ fields, onNavigate, emptyMessage = 'No fields', className = '' }: {
  fields: PresentationField[]
  onNavigate: (selection: DomainKey) => void
  emptyMessage?: string
  className?: string
}) {
  if (fields.length === 0) {
    return <div className={`px-4 py-4 text-sm text-zinc-600 ${className}`}>{emptyMessage}</div>
  }
  return (
    <div className={className}>
      <Table>
        <TableHeader>
          <TableRow className="border-white/10 hover:bg-transparent">
            <TableHead className="h-11 px-4 text-[10px] uppercase tracking-[0.14em] text-zinc-600">Field</TableHead>
            <TableHead className="h-11 px-4 text-[10px] uppercase tracking-[0.14em] text-zinc-600">Type</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {fields.map((field) => (
            <TableRow key={field.name} className="border-white/10 hover:bg-white/3">
              <TableCell className="px-4 py-3 font-mono text-xs text-zinc-300">{field.name}</TableCell>
              <TableCell className="px-4 py-3 font-mono text-xs">
                <DisplayTypeView type={field.type} onNavigate={onNavigate} />
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}

export function DisplayTypeView({ type, onNavigate }: {
  type: DisplayType
  onNavigate: (selection: DomainKey) => void
}) {
  const label = formatDisplayType(type)
  if (type.kind === 'scalar') return <span aria-label={label} title="Scalar" className="text-amber-200">{type.name}</span>
  if (type.kind === 'semanticScalar') return <span aria-label={label} title={`Semantic scalar ${type.name} (${type.id}), represented as ${type.representation}`} className="text-amber-200">{type.name}</span>
  if (type.kind === 'unit') return <span aria-label={label} className="text-zinc-500">()</span>
  if (type.kind === 'reference') {
    return type.key
      ? <Button variant="link" size="xs" aria-label={`Open ${type.name}`} onClick={() => onNavigate(type.key!)} className="h-auto gap-1 p-0 font-mono text-xs text-cyan-200 no-underline hover:text-cyan-100 hover:no-underline">{type.name}<ArrowUpRight /></Button>
      : <span aria-label={label} className="text-cyan-200">{type.name}</span>
  }
  if (type.kind === 'optional') return <span aria-label={label} className="inline-flex items-center text-zinc-500">Option&lt;<DisplayTypeView type={type.value} onNavigate={onNavigate} />&gt;</span>
  return <span aria-label={label} className="inline-flex items-center text-zinc-500">Vec&lt;<DisplayTypeView type={type.element} onNavigate={onNavigate} />&gt;</span>
}

function formatDisplayType(type: DisplayType): string {
  if (type.kind === 'optional') return `Option<${formatDisplayType(type.value)}>`
  if (type.kind === 'list') return `Vec<${formatDisplayType(type.element)}>`
  if (type.kind === 'semanticScalar') return `${type.name} (represented as ${type.representation})`
  return type.name
}
