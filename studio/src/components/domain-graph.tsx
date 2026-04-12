import { useEffect, useId, useRef } from 'react'
import * as d3 from 'd3'

import { buildFocusedGraph, type DomainGraphEdgeKind } from '@/domain/graph'
import type { DomainIndex, DomainKey, SelectionKind } from '@/domain/index'

export interface DomainGraphProps {
  index: DomainIndex
  selectedKey: DomainKey
  onNavigate: (key: DomainKey) => void
}

interface SimulationNode extends d3.SimulationNodeDatum {
  key: DomainKey
  label: string
  kind: SelectionKind
  root: boolean
  selected: boolean
}

interface SimulationLink extends d3.SimulationLinkDatum<SimulationNode> {
  id: string
  source: DomainKey | SimulationNode
  target: DomainKey | SimulationNode
  kind: DomainGraphEdgeKind
  label?: string
}

const nodeColors: Record<SelectionKind, string> = {
  context: '#a3e635',
  aggregate: '#22d3ee',
  entity: '#60a5fa',
  identity: '#a78bfa',
  valueObject: '#fbbf24',
  domainService: '#fb7185',
}

const edgeColors: Record<DomainGraphEdgeKind, string> = {
  ownership: '#64748b', root: '#a3e635', field: '#38bdf8', input: '#c084fc', output: '#34d399', error: '#fb7185',
}

export function DomainGraph({ index, selectedKey, onNavigate }: DomainGraphProps) {
  const containerRef = useRef<HTMLDivElement>(null)
  const svgRef = useRef<SVGSVGElement>(null)
  const markerPrefix = useId().replaceAll(':', '')

  useEffect(() => {
    const container = containerRef.current
    const svgElement = svgRef.current
    if (!container || !svgElement) return

    const graph = buildFocusedGraph(index, selectedKey)
    const nodes: SimulationNode[] = graph.nodes.map((node, position) => ({
      key: node.key, label: node.label, kind: node.kind, root: node.root, selected: node.selected,
      x: Math.cos(position * 2.4) * 80, y: Math.sin(position * 2.4) * 80,
    }))
    const links: SimulationLink[] = graph.edges.map((edge) => ({ ...edge }))
    const svg = d3.select(svgElement)
    svg.selectAll('*').remove()
    const viewport = svg.append('g')
    const marker = svg.append('defs').selectAll<SVGMarkerElement, [string, string]>('marker')
      .data(Object.entries(edgeColors))
      .join('marker')
      .attr('id', ([kind]) => `${markerPrefix}-domain-graph-arrow-${kind}`)
      .attr('viewBox', '0 -5 10 10')
      .attr('refX', 23)
      .attr('markerWidth', 5)
      .attr('markerHeight', 5)
      .attr('orient', 'auto')
    marker.append('path').attr('d', 'M0,-5L10,0L0,5').attr('fill', ([, color]) => color)

    const link = viewport.append('g').selectAll<SVGLineElement, SimulationLink>('line')
      .data(links, (item) => item.id)
      .join('line')
      .attr('stroke', (item) => edgeColors[item.kind])
      .attr('stroke-width', (item) => item.kind === 'root' ? 2.5 : 1.4)
      .attr('stroke-dasharray', (item) => item.kind === 'ownership' ? '4 4' : null)
      .attr('stroke-opacity', 0.72)
      .attr('marker-end', (item) => `url(#${markerPrefix}-domain-graph-arrow-${item.kind})`)
    const edgeLabel = viewport.append('g').selectAll<SVGTextElement, SimulationLink>('text')
      .data(links.filter((item) => item.label !== undefined), (item) => item.id)
      .join('text')
      .text((item) => item.label ?? '')
      .attr('fill', '#94a3b8')
      .attr('font-size', 9)
      .attr('text-anchor', 'middle')
      .attr('paint-order', 'stroke')
      .attr('stroke', '#101314')
      .attr('stroke-width', 3)

    const node = viewport.append('g').selectAll<SVGGElement, SimulationNode>('g')
      .data(nodes, (item) => item.key)
      .join('g')
      .attr('role', 'button')
      .attr('tabindex', 0)
      .attr('aria-label', (item) => `${item.label}, ${kindLabel(item.kind)}${item.root ? ', aggregate root' : ''}`)
      .style('cursor', 'pointer')
      .on('click', (_event, item) => onNavigate(item.key))
      .on('keydown', (event: KeyboardEvent, item) => {
        if (event.key === 'Enter' || event.key === ' ') {
          event.preventDefault()
          onNavigate(item.key)
        }
      })
    node.append('circle')
      .attr('r', (item) => item.selected ? 17 : 12)
      .attr('fill', '#101314')
      .attr('stroke', (item) => nodeColors[item.kind])
      .attr('stroke-width', (item) => item.selected ? 4 : 2)
    node.filter((item) => item.root).append('circle')
      .attr('cx', 10).attr('cy', -10).attr('r', 4).attr('fill', '#a3e635').attr('stroke', '#101314').attr('stroke-width', 2)
    node.append('text')
      .text((item) => item.label)
      .attr('y', (item) => item.selected ? 31 : 26)
      .attr('text-anchor', 'middle')
      .attr('fill', (item) => item.selected ? '#f8fafc' : '#cbd5e1')
      .attr('font-size', (item) => item.selected ? 12 : 11)
      .attr('font-weight', (item) => item.selected ? 650 : 500)
      .attr('paint-order', 'stroke')
      .attr('stroke', '#101314')
      .attr('stroke-width', 4)

    const simulation = d3.forceSimulation(nodes)
      .force('link', d3.forceLink<SimulationNode, SimulationLink>(links).id((item) => item.key).distance(105).strength(0.75))
      .force('charge', d3.forceManyBody().strength(-320))
      .force('center', d3.forceCenter())
      .force('collide', d3.forceCollide<SimulationNode>().radius((item) => item.selected ? 46 : 40))

    const render = () => {
      link
        .attr('x1', (item) => (item.source as SimulationNode).x ?? 0)
        .attr('y1', (item) => (item.source as SimulationNode).y ?? 0)
        .attr('x2', (item) => (item.target as SimulationNode).x ?? 0)
        .attr('y2', (item) => (item.target as SimulationNode).y ?? 0)
      edgeLabel
        .attr('x', (item) => (((item.source as SimulationNode).x ?? 0) + ((item.target as SimulationNode).x ?? 0)) / 2)
        .attr('y', (item) => (((item.source as SimulationNode).y ?? 0) + ((item.target as SimulationNode).y ?? 0)) / 2 - 4)
      node.attr('transform', (item) => `translate(${item.x ?? 0},${item.y ?? 0})`)
    }
    simulation.on('tick', render)

    node.call(d3.drag<SVGGElement, SimulationNode>()
      .clickDistance(4)
      .on('start', (event, item) => {
        if (!event.active) simulation.alphaTarget(0.2).restart()
        item.fx = item.x
        item.fy = item.y
      })
      .on('drag', (event, item) => {
        item.fx = event.x
        item.fy = event.y
      })
      .on('end', (event, item) => {
        if (!event.active) simulation.alphaTarget(0)
        item.fx = null
        item.fy = null
      }))

    const zoom = d3.zoom<SVGSVGElement, unknown>().scaleExtent([0.35, 3]).on('zoom', (event) => viewport.attr('transform', event.transform))
    svg.call(zoom)
    const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches
    const resize = ([entry]: ResizeObserverEntry[]) => {
      if (!entry) return
      const { width, height } = entry.contentRect
      svg.attr('viewBox', `${-width / 2} ${-height / 2} ${width} ${height}`)
      simulation.force('center', d3.forceCenter(0, 0)).alpha(0.25)
      if (reducedMotion) {
        simulation.stop().tick(40)
        render()
      } else {
        simulation.restart()
      }
    }
    const observer = new ResizeObserver(resize)
    observer.observe(container)

    if (reducedMotion) {
      simulation.stop().tick(120)
      render()
    }

    return () => {
      observer.disconnect()
      simulation.stop()
      svg.on('.zoom', null)
      node.on('.drag', null)
    }
  }, [index, markerPrefix, selectedKey, onNavigate])

  return (
    <div ref={containerRef} className="relative min-h-72 size-full overflow-hidden rounded-lg border border-white/10 bg-[#101314]">
      <svg ref={svgRef} role="img" aria-label="Focused domain relationship graph" className="block size-full touch-none" />
    </div>
  )
}

function kindLabel(kind: SelectionKind): string {
  return ({ context: 'bounded context', aggregate: 'aggregate', entity: 'entity', identity: 'identity', valueObject: 'value object', domainService: 'domain service' })[kind]
}
