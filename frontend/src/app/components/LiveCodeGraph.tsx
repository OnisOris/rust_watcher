import { useEffect, useMemo, useRef, useState } from 'react'
import type { PointerEvent as ReactPointerEvent } from 'react'
import type { GraphEdge, GraphFilters, GraphLabelMode, GraphLayoutSettings, GraphMode, GraphNode, ThemeMode, DiagnosticRecord } from '../types'
import { inferNodeLanguage, languageColor, languageIcon } from '../api/language'

interface LiveCodeGraphProps {
  nodes: GraphNode[]
  edges: GraphEdge[]
  filters: GraphFilters
  selectedNodeId: string | null
  recenterKey: number
  theme: ThemeMode
  layoutSettings: GraphLayoutSettings
  graphMode: GraphMode
  labelMode: GraphLabelMode
  diagnosticsByNode?: Map<string, DiagnosticRecord[]>
  highlightedTraceNodeIds?: Set<string>
  highlightedTraceEdgeIds?: Set<string>
  onSelectNode: (id: string | null) => void
  onUpdateNodes: (nodes: GraphNode[]) => void
}

const NODE_SIZE = 18
const EDGE_COLOR = 'rgba(71, 85, 105, 0.55)'

function stablePoint(id: string, index: number, count: number) {
  let hash = 0
  for (let i = 0; i < id.length; i++) hash = (hash * 31 + id.charCodeAt(i)) | 0
  const angle = index * Math.PI * (3 - Math.sqrt(5)) + (hash % 100) / 100
  const radius = Math.max(140, Math.sqrt(Math.max(1, count)) * 58) * Math.sqrt((index + 1) / Math.max(1, count))
  return { x: Math.cos(angle) * radius, y: Math.sin(angle) * radius }
}

function useForceLayout(nodes: GraphNode[], edges: GraphEdge[], settings: GraphLayoutSettings, recenterKey: number) {
  const [layout, setLayout] = useState<GraphNode[]>(nodes)
  const positionsRef = useRef(new Map<string, GraphNode>())

  useEffect(() => {
    const previous = positionsRef.current
    const seeded = nodes.map((node, index) => {
      const existing = previous.get(node.id)
      if (existing) return { ...node, x: existing.x, y: existing.y, vx: existing.vx ?? 0, vy: existing.vy ?? 0 }
      const point = stablePoint(node.id, index, nodes.length)
      return { ...node, x: point.x * settings.spacing, y: point.y * settings.spacing, vx: 0, vy: 0 }
    })
    setLayout(seeded)
  }, [nodes, settings.spacing, recenterKey])

  useEffect(() => {
    let frame = 0
    let current = layout
    const byEdge = () => edges.filter(edge => current.some(node => node.id === edge.source) && current.some(node => node.id === edge.target))
    const tick = () => {
      const next = current.map(node => ({ ...node }))
      const byId = new Map(next.map(node => [node.id, node]))
      const forces = new Map(next.map(node => [node.id, { x: 0, y: 0 }]))
      for (let i = 0; i < next.length; i++) {
        for (let j = i + 1; j < next.length; j++) {
          const a = next[i]
          const b = next[j]
          const dx = b.x - a.x
          const dy = b.y - a.y
          const dist2 = Math.max(50, dx * dx + dy * dy)
          const dist = Math.sqrt(dist2)
          const f = Math.min(20, 6500 * settings.repulsion / dist2)
          const fx = dx / dist * f
          const fy = dy / dist * f
          forces.get(a.id)!.x -= fx
          forces.get(a.id)!.y -= fy
          forces.get(b.id)!.x += fx
          forces.get(b.id)!.y += fy
        }
      }
      for (const edge of byEdge()) {
        const a = byId.get(edge.source)
        const b = byId.get(edge.target)
        if (!a || !b) continue
        const dx = b.x - a.x
        const dy = b.y - a.y
        const dist = Math.sqrt(dx * dx + dy * dy) || 1
        const target = 130 * settings.linkLength * settings.spacing
        const f = (dist - target) * 0.025
        forces.get(a.id)!.x += dx / dist * f
        forces.get(a.id)!.y += dy / dist * f
        forces.get(b.id)!.x -= dx / dist * f
        forces.get(b.id)!.y -= dy / dist * f
      }
      for (const node of next) {
        if (node.pinned) continue
        const f = forces.get(node.id)!
        f.x -= node.x * 0.003
        f.y -= node.y * 0.003
        node.vx = (node.vx + f.x) * (1 - 0.08 * settings.damping)
        node.vy = (node.vy + f.y) * (1 - 0.08 * settings.damping)
        node.x += Math.max(-12, Math.min(12, node.vx))
        node.y += Math.max(-12, Math.min(12, node.vy))
      }
      current = next
      positionsRef.current = new Map(next.map(node => [node.id, node]))
      setLayout(next)
      frame = requestAnimationFrame(tick)
    }
    frame = requestAnimationFrame(tick)
    const stop = window.setTimeout(() => cancelAnimationFrame(frame), 1600)
    return () => { cancelAnimationFrame(frame); window.clearTimeout(stop) }
  }, [edges, layout, settings.damping, settings.linkLength, settings.repulsion, settings.spacing])

  return [layout, setLayout] as const
}

function bounds(nodes: GraphNode[]) {
  if (!nodes.length) return { minX: -1, minY: -1, maxX: 1, maxY: 1, width: 2, height: 2 }
  let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity
  for (const node of nodes) {
    minX = Math.min(minX, node.x - 90)
    maxX = Math.max(maxX, node.x + 90)
    minY = Math.min(minY, node.y - 70)
    maxY = Math.max(maxY, node.y + 90)
  }
  return { minX, minY, maxX, maxY, width: Math.max(1, maxX - minX), height: Math.max(1, maxY - minY) }
}

function visibleNodeIds(nodes: GraphNode[], edges: GraphEdge[], depth: GraphFilters['depth']) {
  if (depth === 'full') return new Set(nodes.map(node => node.id))
  const center = nodes.find(node => node.label === 'main')?.id ?? nodes[0]?.id
  const visible = new Set<string>()
  if (!center) return visible
  visible.add(center)
  for (let i = 0; i < Number(depth); i++) {
    for (const edge of edges) {
      if (visible.has(edge.source)) visible.add(edge.target)
      if (visible.has(edge.target)) visible.add(edge.source)
    }
  }
  return visible
}

export function LiveCodeGraph({ nodes, edges, filters, selectedNodeId, recenterKey, layoutSettings, labelMode, diagnosticsByNode, highlightedTraceNodeIds, highlightedTraceEdgeIds, onSelectNode, onUpdateNodes }: LiveCodeGraphProps) {
  const [draggedId, setDraggedId] = useState<string | null>(null)
  const [layout, setLayout] = useForceLayout(nodes, edges, layoutSettings, recenterKey)
  const visibleIds = useMemo(() => visibleNodeIds(layout, edges, filters.depth), [layout, edges, filters.depth])
  const visibleNodes = useMemo(() => layout.filter(node => visibleIds.has(node.id) && filters.nodeTypes.has(node.type)), [layout, visibleIds, filters.nodeTypes])
  const nodeById = useMemo(() => new Map(layout.map(node => [node.id, node])), [layout])
  const visibleEdges = edges.filter(edge => filters.edgeTypes.has(edge.type) && visibleIds.has(edge.source) && visibleIds.has(edge.target))
  const graphBounds = bounds(visibleNodes)
  const viewBox = `${graphBounds.minX} ${graphBounds.minY} ${graphBounds.width} ${graphBounds.height}`

  const moveDragged = (event: ReactPointerEvent<SVGSVGElement>) => {
    if (!draggedId) return
    const svg = event.currentTarget
    const point = svg.createSVGPoint()
    point.x = event.clientX
    point.y = event.clientY
    const matrix = svg.getScreenCTM()
    if (!matrix) return
    const world = point.matrixTransform(matrix.inverse())
    const next = layout.map(node => node.id === draggedId ? { ...node, x: world.x, y: world.y, vx: 0, vy: 0, pinned: true } : node)
    setLayout(next)
  }

  const finishDrag = () => {
    if (draggedId) onUpdateNodes(layout)
    setDraggedId(null)
  }

  return (
    <div className="relative w-full h-full overflow-hidden" style={{ background: 'var(--cc-bg)' }}>
      <svg
        className="w-full h-full"
        viewBox={viewBox}
        onPointerMove={moveDragged}
        onPointerUp={finishDrag}
        onPointerLeave={finishDrag}
        onClick={() => onSelectNode(null)}
      >
        <defs>
          <marker id="arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto" markerUnits="strokeWidth">
            <path d="M 0 0 L 8 4 L 0 8 z" fill="rgba(71, 85, 105, 0.65)" />
          </marker>
        </defs>
        <rect x={graphBounds.minX} y={graphBounds.minY} width={graphBounds.width} height={graphBounds.height} fill="transparent" />
        {visibleEdges.map(edge => {
          const source = nodeById.get(edge.source)
          const target = nodeById.get(edge.target)
          if (!source || !target) return null
          const active = highlightedTraceEdgeIds?.has(edge.id) || edge.bundledEdgeIds?.some(id => highlightedTraceEdgeIds?.has(id))
          const stroke = active ? 'var(--cc-accent)' : edge.type === 'ApiCall' ? '#E11D48' : edge.type === 'EndpointHandler' ? '#F97316' : edge.type === 'DataFlow' ? '#8B5CF6' : EDGE_COLOR
          return <line key={edge.id} x1={source.x} y1={source.y} x2={target.x} y2={target.y} stroke={stroke} strokeWidth={active ? 3 : edge.type === 'Contains' ? 1 : 1.8} markerEnd="url(#arrow)" opacity={active ? 0.95 : 0.62} />
        })}
        {visibleNodes.map(node => {
          const language = inferNodeLanguage(node)
          const color = languageColor(language)
          const selected = node.id === selectedNodeId || highlightedTraceNodeIds?.has(node.id)
          const diagnostics = diagnosticsByNode?.get(node.id) ?? []
          const showLabel = labelMode === 'all' || selected || node.type === 'File' || node.type === 'Module' || node.type === 'Endpoint'
          return (
            <g
              key={node.id}
              transform={`translate(${node.x} ${node.y})`}
              style={{ cursor: 'pointer' }}
              onPointerDown={(event) => { event.stopPropagation(); setDraggedId(node.id) }}
              onClick={(event) => { event.stopPropagation(); onSelectNode(node.id) }}
            >
              <circle r={NODE_SIZE} fill="var(--cc-card)" stroke={selected ? 'var(--cc-text)' : color} strokeWidth={selected ? 3 : 2} />
              <rect x={8} y={-26} width={Math.max(20, languageIcon(language).length * 7 + 8)} height={16} rx={8} fill="var(--cc-card)" stroke={color} strokeWidth={1.5} />
              <text x={8 + Math.max(20, languageIcon(language).length * 7 + 8) / 2} y={-14} textAnchor="middle" fontSize={8} fontWeight={800} fill={color}>{languageIcon(language)}</text>
              {diagnostics.length > 0 && <circle cx={15} cy={15} r={5} fill={diagnostics.some(item => item.severity === 'Error') ? '#F87171' : '#F59E0B'} />}
              {node.pinned && <text x={-4} y={5} fontSize={12} fill="#F59E0B">P</text>}
              {showLabel && <text x={0} y={34} textAnchor="middle" fontSize={11} fontWeight={selected ? 800 : 600} fill="var(--cc-text-muted)">{node.label}</text>}
            </g>
          )
        })}
      </svg>
      <div className="absolute left-4 bottom-4 rounded-xl px-3 py-2" style={{ background: 'var(--cc-overlay)', border: '1px solid var(--cc-border)', backdropFilter: 'blur(8px)' }}>
        <div style={{ fontSize: 10, color: 'var(--cc-text)', fontWeight: 700, marginBottom: 6 }}>Language badges</div>
        <div className="flex gap-2" style={{ fontSize: 10, color: 'var(--cc-text-muted)' }}>
          {['rust', 'typescript', 'python', 'qml', 'endpoints'].map(language => (
            <span key={language} className="flex items-center gap-1"><span style={{ width: 18, height: 10, borderRadius: 999, background: languageColor(language), display: 'inline-block' }} />{languageIcon(language)}</span>
          ))}
        </div>
      </div>
    </div>
  )
}
