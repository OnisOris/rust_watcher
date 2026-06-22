import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { PointerEvent as ReactPointerEvent, WheelEvent as ReactWheelEvent } from 'react'
import type { DiagnosticRecord, GraphEdge, GraphFilters, GraphLabelMode, GraphLayoutSettings, GraphMode, GraphNode, NodeType, ThemeMode } from '../types'
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

type Bounds = { minX: number; minY: number; maxX: number; maxY: number; width: number; height: number }
type ViewBox = { x: number; y: number; width: number; height: number }
type DragState =
  | { kind: 'node'; nodeId: string; moved: boolean }
  | { kind: 'pan'; startX: number; startY: number; view: ViewBox; moved: boolean }
  | null

const NODE_SIZES: Record<NodeType, number> = {
  Module: 24,
  ExternalCrate: 16,
  File: 17,
  Struct: 17,
  Class: 17,
  Object: 16,
  Enum: 17,
  Trait: 17,
  Impl: 11,
  Function: 13,
  Method: 12,
  Component: 17,
  Hook: 13,
  Interface: 16,
  TypeAlias: 14,
  Property: 10,
  Signal: 11,
  Handler: 12,
  Endpoint: 16,
  Macro: 12,
}

const EDGE_COLORS = {
  Contains: '#94A3B8',
  Imports: '#64748B',
  Uses: '#64748B',
  Calls: '#06B6D4',
  Renders: '#14B8A6',
  ApiCall: '#E11D48',
  EndpointHandler: '#F97316',
  Implements: '#10B981',
  TypeReference: '#3B82F6',
  DataFlow: '#8B5CF6',
  ModDeclaration: '#6366F1',
  ExternalDependency: '#64748B',
} satisfies Record<GraphEdge['type'], string>

const GOLDEN_ANGLE = Math.PI * (3 - Math.sqrt(5))
const BASE_REPULSION = 8200
const BASE_LINK_LENGTH = 132
const SPRING_STRENGTH = 0.026
const CENTER_GRAVITY = 0.003
const MAX_SPEED = 10
const SETTLE_MS = 2400
const MINIMAP_WIDTH = 158
const MINIMAP_HEIGHT = 96

function stablePoint(id: string, index: number, count: number) {
  let hash = 0
  for (let i = 0; i < id.length; i++) hash = (hash * 31 + id.charCodeAt(i)) | 0
  const angle = index * GOLDEN_ANGLE + (Math.abs(hash) % 100) / 100
  const radius = Math.max(120, Math.sqrt(Math.max(1, count)) * 52) * Math.sqrt((index + 1) / Math.max(1, count))
  return { x: Math.cos(angle) * radius, y: Math.sin(angle) * radius }
}

function seedLayout(nodes: GraphNode[], previous: Map<string, GraphNode>, spacing: number) {
  return nodes.map((node, index) => {
    const existing = previous.get(node.id)
    if (existing) {
      return {
        ...node,
        x: Number.isFinite(existing.x) ? existing.x : 0,
        y: Number.isFinite(existing.y) ? existing.y : 0,
        vx: existing.vx ?? 0,
        vy: existing.vy ?? 0,
        pinned: node.pinned || existing.pinned,
      }
    }
    const point = stablePoint(node.id, index, nodes.length)
    return { ...node, x: point.x * spacing, y: point.y * spacing, vx: 0, vy: 0 }
  })
}

function nodeBounds(nodes: GraphNode[]): Bounds {
  if (!nodes.length) return { minX: -200, minY: -140, maxX: 200, maxY: 140, width: 400, height: 280 }
  let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity
  for (const node of nodes) {
    const size = NODE_SIZES[node.type] ?? 14
    const labelPad = Math.min(120, Math.max(42, node.label.length * 4.5))
    minX = Math.min(minX, node.x - size - labelPad)
    maxX = Math.max(maxX, node.x + size + labelPad)
    minY = Math.min(minY, node.y - size - 32)
    maxY = Math.max(maxY, node.y + size + 44)
  }
  return { minX, minY, maxX, maxY, width: Math.max(1, maxX - minX), height: Math.max(1, maxY - minY) }
}

function fitView(nodes: GraphNode[]): ViewBox {
  const bounds = nodeBounds(nodes)
  const padX = Math.max(140, bounds.width * 0.12)
  const padY = Math.max(110, bounds.height * 0.16)
  return {
    x: bounds.minX - padX,
    y: bounds.minY - padY,
    width: bounds.width + padX * 2,
    height: bounds.height + padY * 2,
  }
}

function visibleNodeIds(nodes: GraphNode[], edges: GraphEdge[], depth: GraphFilters['depth']) {
  if (depth === 'full') return new Set(nodes.map(node => node.id))
  const nodeById = new Map(nodes.map(node => [node.id, node]))
  const degree = new Map<string, number>()
  for (const edge of edges) {
    degree.set(edge.source, (degree.get(edge.source) ?? 0) + 1)
    degree.set(edge.target, (degree.get(edge.target) ?? 0) + 1)
  }
  const center = nodes.find(node => node.type === 'Function' && node.label === 'main')?.id
    ?? [...degree.entries()].sort((a, b) => b[1] - a[1])[0]?.[0]
    ?? nodes[0]?.id
  const visible = new Set<string>()
  if (!center) return visible
  visible.add(center)
  for (let step = 0; step < depth; step++) {
    const frontier = [...visible]
    for (const edge of edges) {
      if (frontier.includes(edge.source) && nodeById.has(edge.target)) visible.add(edge.target)
      if (frontier.includes(edge.target) && nodeById.has(edge.source)) visible.add(edge.source)
    }
  }
  return visible
}

function runPhysicsTick(nodes: GraphNode[], edges: GraphEdge[], settings: GraphLayoutSettings) {
  const next = nodes.map(node => ({ ...node }))
  const byId = new Map(next.map(node => [node.id, node]))
  const forces = new Map(next.map(node => [node.id, { x: 0, y: 0 }]))
  const repulsion = BASE_REPULSION * settings.repulsion
  const targetLength = BASE_LINK_LENGTH * settings.linkLength * settings.spacing

  for (let i = 0; i < next.length; i++) {
    for (let j = i + 1; j < next.length; j++) {
      const a = next[i]
      const b = next[j]
      const dx = b.x - a.x
      const dy = b.y - a.y
      const dist2 = Math.max(80, dx * dx + dy * dy)
      const dist = Math.sqrt(dist2)
      const force = Math.min(22, repulsion / dist2)
      const fx = dx / dist * force
      const fy = dy / dist * force
      forces.get(a.id)!.x -= fx
      forces.get(a.id)!.y -= fy
      forces.get(b.id)!.x += fx
      forces.get(b.id)!.y += fy
    }
  }

  for (const edge of edges) {
    const source = byId.get(edge.source)
    const target = byId.get(edge.target)
    if (!source || !target) continue
    const dx = target.x - source.x
    const dy = target.y - source.y
    const dist = Math.sqrt(dx * dx + dy * dy) || 1
    const force = (dist - targetLength) * SPRING_STRENGTH
    const fx = dx / dist * force
    const fy = dy / dist * force
    forces.get(source.id)!.x += fx
    forces.get(source.id)!.y += fy
    forces.get(target.id)!.x -= fx
    forces.get(target.id)!.y -= fy
  }

  let speedSum = 0
  for (const node of next) {
    if (node.pinned) {
      node.vx = 0
      node.vy = 0
      continue
    }
    const force = forces.get(node.id)!
    force.x -= node.x * CENTER_GRAVITY
    force.y -= node.y * CENTER_GRAVITY
    const damping = Math.max(0.62, Math.min(0.94, 1 - 0.12 * settings.damping))
    node.vx = ((node.vx ?? 0) + force.x) * damping
    node.vy = ((node.vy ?? 0) + force.y) * damping
    node.vx = Math.max(-MAX_SPEED, Math.min(MAX_SPEED, node.vx))
    node.vy = Math.max(-MAX_SPEED, Math.min(MAX_SPEED, node.vy))
    node.x += node.vx
    node.y += node.vy
    speedSum += Math.sqrt(node.vx * node.vx + node.vy * node.vy)
  }

  return { nodes: next, averageSpeed: speedSum / Math.max(1, next.length) }
}

function edgeStroke(edge: GraphEdge, active: boolean) {
  if (active) return 'var(--cc-accent)'
  return EDGE_COLORS[edge.type] ?? '#64748B'
}

function edgeWidth(edge: GraphEdge, active: boolean) {
  if (active) return 3
  if (edge.type === 'ApiCall' || edge.type === 'EndpointHandler' || edge.type === 'DataFlow') return 2.2
  if (edge.type === 'Contains') return 1
  return 1.5
}

function badgeWidth(text: string) {
  return Math.max(20, text.length * 7 + 8)
}

function shouldShowLabel(node: GraphNode, selected: boolean, labelMode: GraphLabelMode, degree: number) {
  if (selected || labelMode === 'all') return true
  if (labelMode === 'key') return degree >= 4 || node.type === 'Module' || node.type === 'File' || node.type === 'Endpoint'
  return node.type === 'File' || node.type === 'Module' || node.type === 'Endpoint' || degree >= 7
}

function screenToWorld(svg: SVGSVGElement, view: ViewBox, clientX: number, clientY: number) {
  const rect = svg.getBoundingClientRect()
  const rx = (clientX - rect.left) / Math.max(1, rect.width)
  const ry = (clientY - rect.top) / Math.max(1, rect.height)
  return { x: view.x + rx * view.width, y: view.y + ry * view.height }
}

function MiniMap({ nodes, view }: { nodes: GraphNode[]; view: ViewBox }) {
  if (nodes.length < 8) return null
  const bounds = nodeBounds(nodes)
  const pad = 80
  const world = {
    x: bounds.minX - pad,
    y: bounds.minY - pad,
    width: bounds.width + pad * 2,
    height: bounds.height + pad * 2,
  }
  const scale = Math.min(MINIMAP_WIDTH / world.width, MINIMAP_HEIGHT / world.height)
  const mapWidth = world.width * scale
  const mapHeight = world.height * scale
  const offsetX = (MINIMAP_WIDTH - mapWidth) / 2
  const offsetY = (MINIMAP_HEIGHT - mapHeight) / 2
  const toMap = (x: number, y: number) => ({
    x: offsetX + (x - world.x) * scale,
    y: offsetY + (y - world.y) * scale,
  })
  const viewTopLeft = toMap(view.x, view.y)
  const viewBottomRight = toMap(view.x + view.width, view.y + view.height)

  return (
    <div className="absolute right-4 bottom-4 rounded-lg overflow-hidden" style={{ width: MINIMAP_WIDTH + 16, height: MINIMAP_HEIGHT + 16, background: 'var(--cc-overlay)', border: '1px solid var(--cc-border)', boxShadow: 'var(--cc-shadow)', backdropFilter: 'blur(8px)' }}>
      <svg width={MINIMAP_WIDTH + 16} height={MINIMAP_HEIGHT + 16}>
        <rect x={8} y={8} width={MINIMAP_WIDTH} height={MINIMAP_HEIGHT} rx={6} fill="var(--cc-minimap-bg)" stroke="var(--cc-border)" />
        {nodes.map(node => {
          const point = toMap(node.x, node.y)
          const language = inferNodeLanguage(node)
          return <circle key={node.id} cx={point.x + 8} cy={point.y + 8} r={2.2} fill={languageColor(language)} opacity={0.78} />
        })}
        <rect
          x={Math.max(8, Math.min(MINIMAP_WIDTH + 8, viewTopLeft.x + 8))}
          y={Math.max(8, Math.min(MINIMAP_HEIGHT + 8, viewTopLeft.y + 8))}
          width={Math.max(4, Math.min(MINIMAP_WIDTH, viewBottomRight.x - viewTopLeft.x))}
          height={Math.max(4, Math.min(MINIMAP_HEIGHT, viewBottomRight.y - viewTopLeft.y))}
          fill="rgba(14, 165, 233, 0.16)"
          stroke="var(--cc-accent)"
          strokeWidth={1.4}
        />
      </svg>
    </div>
  )
}

export function LiveCodeGraph({ nodes, edges, filters, selectedNodeId, recenterKey, layoutSettings, labelMode, diagnosticsByNode, highlightedTraceNodeIds, highlightedTraceEdgeIds, onSelectNode, onUpdateNodes }: LiveCodeGraphProps) {
  const svgRef = useRef<SVGSVGElement>(null)
  const animationRef = useRef<number>(0)
  const dragRef = useRef<DragState>(null)
  const layoutRef = useRef<GraphNode[]>([])
  const initializedRef = useRef(false)
  const [layout, setLayout] = useState<GraphNode[]>([])
  const [view, setView] = useState<ViewBox>(() => fitView(nodes))

  useEffect(() => {
    const previous = new Map(layoutRef.current.map(node => [node.id, node]))
    const seeded = seedLayout(nodes, previous, layoutSettings.spacing)
    layoutRef.current = seeded
    setLayout(seeded)
    if (!initializedRef.current) {
      initializedRef.current = true
      setView(fitView(seeded))
    }
  }, [nodes, layoutSettings.spacing])

  useEffect(() => {
    if (layoutRef.current.length > 0) setView(fitView(layoutRef.current))
  }, [recenterKey])

  useEffect(() => {
    cancelAnimationFrame(animationRef.current)
    const start = performance.now()
    const tick = (time: number) => {
      const result = runPhysicsTick(layoutRef.current, edges, layoutSettings)
      layoutRef.current = result.nodes
      setLayout(result.nodes)
      const elapsed = time - start
      if (elapsed < SETTLE_MS && result.averageSpeed > 0.015) {
        animationRef.current = requestAnimationFrame(tick)
      }
    }
    animationRef.current = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(animationRef.current)
  }, [edges, layoutSettings])

  const visibleIds = useMemo(() => visibleNodeIds(layout, edges, filters.depth), [layout, edges, filters.depth])
  const degree = useMemo(() => {
    const result = new Map<string, number>()
    for (const edge of edges) {
      result.set(edge.source, (result.get(edge.source) ?? 0) + 1)
      result.set(edge.target, (result.get(edge.target) ?? 0) + 1)
    }
    return result
  }, [edges])
  const visibleNodes = useMemo(
    () => layout.filter(node => visibleIds.has(node.id) && filters.nodeTypes.has(node.type)),
    [layout, visibleIds, filters.nodeTypes],
  )
  const visibleNodeIdSet = useMemo(() => new Set(visibleNodes.map(node => node.id)), [visibleNodes])
  const nodeById = useMemo(() => new Map(layout.map(node => [node.id, node])), [layout])
  const visibleEdges = useMemo(
    () => edges.filter(edge => filters.edgeTypes.has(edge.type) && visibleNodeIdSet.has(edge.source) && visibleNodeIdSet.has(edge.target)),
    [edges, filters.edgeTypes, visibleNodeIdSet],
  )

  const updateNodePosition = useCallback((nodeId: string, x: number, y: number) => {
    const next = layoutRef.current.map(node => node.id === nodeId ? { ...node, x, y, vx: 0, vy: 0, pinned: true } : node)
    layoutRef.current = next
    setLayout(next)
  }, [])

  const handlePointerDown = useCallback((event: ReactPointerEvent<SVGSVGElement>) => {
    if (event.button !== 0) return
    dragRef.current = { kind: 'pan', startX: event.clientX, startY: event.clientY, view, moved: false }
    event.currentTarget.setPointerCapture(event.pointerId)
  }, [view])

  const handleNodePointerDown = useCallback((event: ReactPointerEvent<SVGGElement>, nodeId: string) => {
    if (event.button !== 0) return
    event.stopPropagation()
    dragRef.current = { kind: 'node', nodeId, moved: false }
    svgRef.current?.setPointerCapture(event.pointerId)
  }, [])

  const handlePointerMove = useCallback((event: ReactPointerEvent<SVGSVGElement>) => {
    const drag = dragRef.current
    const svg = svgRef.current
    if (!drag || !svg) return
    if (drag.kind === 'node') {
      const point = screenToWorld(svg, view, event.clientX, event.clientY)
      drag.moved = true
      updateNodePosition(drag.nodeId, point.x, point.y)
      return
    }
    const rect = svg.getBoundingClientRect()
    const dx = (event.clientX - drag.startX) / Math.max(1, rect.width) * drag.view.width
    const dy = (event.clientY - drag.startY) / Math.max(1, rect.height) * drag.view.height
    if (Math.abs(dx) + Math.abs(dy) > 2) drag.moved = true
    setView({ ...drag.view, x: drag.view.x - dx, y: drag.view.y - dy })
  }, [updateNodePosition, view])

  const finishDrag = useCallback(() => {
    const drag = dragRef.current
    if (drag?.kind === 'node') onUpdateNodes(layoutRef.current)
    dragRef.current = null
  }, [onUpdateNodes])

  const handleWheel = useCallback((event: ReactWheelEvent<SVGSVGElement>) => {
    event.preventDefault()
    const svg = svgRef.current
    if (!svg) return
    const rect = svg.getBoundingClientRect()
    const mx = (event.clientX - rect.left) / Math.max(1, rect.width)
    const my = (event.clientY - rect.top) / Math.max(1, rect.height)
    const worldX = view.x + mx * view.width
    const worldY = view.y + my * view.height
    const factor = event.deltaY > 0 ? 1.12 : 0.88
    const nextWidth = Math.max(120, Math.min(9000, view.width * factor))
    const nextHeight = Math.max(90, Math.min(7000, view.height * factor))
    setView({
      x: worldX - mx * nextWidth,
      y: worldY - my * nextHeight,
      width: nextWidth,
      height: nextHeight,
    })
  }, [view])

  const handleClick = useCallback(() => {
    const drag = dragRef.current
    if (drag?.moved) return
    onSelectNode(null)
  }, [onSelectNode])

  return (
    <div className="relative w-full h-full overflow-hidden" style={{ background: 'var(--cc-bg)' }}>
      <svg
        ref={svgRef}
        className="w-full h-full touch-none select-none"
        viewBox={`${view.x} ${view.y} ${view.width} ${view.height}`}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={finishDrag}
        onPointerCancel={finishDrag}
        onPointerLeave={finishDrag}
        onWheel={handleWheel}
        onClick={handleClick}
        style={{ cursor: dragRef.current?.kind === 'pan' ? 'grabbing' : 'grab' }}
      >
        <defs>
          <marker id="arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto" markerUnits="strokeWidth">
            <path d="M 0 0 L 8 4 L 0 8 z" fill="rgba(71, 85, 105, 0.65)" />
          </marker>
        </defs>
        <rect x={view.x} y={view.y} width={view.width} height={view.height} fill="transparent" />
        {visibleEdges.map(edge => {
          const source = nodeById.get(edge.source)
          const target = nodeById.get(edge.target)
          if (!source || !target) return null
          const active = !!highlightedTraceEdgeIds?.has(edge.id) || !!edge.bundledEdgeIds?.some(id => highlightedTraceEdgeIds?.has(id))
          return (
            <g key={edge.id}>
              <line
                x1={source.x}
                y1={source.y}
                x2={target.x}
                y2={target.y}
                stroke={edgeStroke(edge, active)}
                strokeWidth={edgeWidth(edge, active)}
                strokeDasharray={edge.type === 'ExternalDependency' || edge.type === 'Renders' ? '5 4' : undefined}
                markerEnd="url(#arrow)"
                opacity={active ? 0.95 : 0.58}
              />
              {(edge.bundledCount ?? 1) > 1 && (
                <text x={(source.x + target.x) / 2} y={(source.y + target.y) / 2 - 4} textAnchor="middle" fontSize={9} fontWeight={800} fill="var(--cc-text-muted)">
                  {edge.bundledCount}
                </text>
              )}
            </g>
          )
        })}
        {visibleNodes.map(node => {
          const language = inferNodeLanguage(node)
          const color = languageColor(language)
          const icon = languageIcon(language)
          const selected = node.id === selectedNodeId || highlightedTraceNodeIds?.has(node.id)
          const diagnostics = diagnosticsByNode?.get(node.id) ?? []
          const size = NODE_SIZES[node.type] ?? 14
          const labelVisible = shouldShowLabel(node, !!selected, labelMode, degree.get(node.id) ?? 0)
          const badgeW = badgeWidth(icon)
          return (
            <g
              key={node.id}
              transform={`translate(${node.x} ${node.y})`}
              style={{ cursor: 'pointer' }}
              onPointerDown={(event) => handleNodePointerDown(event, node.id)}
              onClick={(event) => { event.stopPropagation(); onSelectNode(node.id) }}
            >
              <circle r={size} fill="var(--cc-card)" stroke={selected ? 'var(--cc-text)' : color} strokeWidth={selected ? 3 : 2} opacity={node.reachability === 'Detached' ? 0.56 : 1} />
              <circle r={Math.max(3, size * 0.22)} fill={color} opacity={0.82} />
              <rect x={size * 0.35} y={-size - 14} width={badgeW} height={16} rx={8} fill="var(--cc-card)" stroke={color} strokeWidth={1.4} />
              <text x={size * 0.35 + badgeW / 2} y={-size - 2.5} textAnchor="middle" fontSize={8} fontWeight={850} fill={color}>{icon}</text>
              {diagnostics.length > 0 && <circle cx={size * 0.75} cy={size * 0.75} r={4.8} fill={diagnostics.some(item => item.severity === 'Error') ? '#F87171' : '#F59E0B'} />}
              {node.pinned && <text x={-3} y={4} fontSize={10} fill="#F59E0B" textAnchor="middle">P</text>}
              {labelVisible && (
                <text x={0} y={size + 14} textAnchor="middle" fontSize={selected ? 12 : 10} fontWeight={selected ? 800 : 650} fill="var(--cc-text-muted)">
                  {node.label}
                </text>
              )}
            </g>
          )
        })}
      </svg>

      <div className="absolute left-4 bottom-4 rounded-xl px-3 py-2" style={{ background: 'var(--cc-overlay)', border: '1px solid var(--cc-border)', backdropFilter: 'blur(8px)' }}>
        <div style={{ fontSize: 10, color: 'var(--cc-text)', fontWeight: 700, marginBottom: 6 }}>Language badges</div>
        <div className="flex gap-2" style={{ fontSize: 10, color: 'var(--cc-text-muted)' }}>
          {(['rust', 'typescript', 'python', 'qml', 'endpoints'] as const).map(language => (
            <span key={language} className="flex items-center gap-1"><span style={{ width: 18, height: 10, borderRadius: 999, background: languageColor(language), display: 'inline-block' }} />{languageIcon(language)}</span>
          ))}
        </div>
      </div>

      <MiniMap nodes={visibleNodes} view={view} />
    </div>
  )
}
