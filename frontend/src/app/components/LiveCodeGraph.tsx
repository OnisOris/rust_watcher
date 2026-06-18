import { useRef, useEffect, useCallback, useState } from 'react'
import type { GraphNode, GraphEdge, GraphMode, GraphFilters, NodeType, EdgeType, ThemeMode } from '../types'

interface LiveCodeGraphProps {
  nodes: GraphNode[]
  edges: GraphEdge[]
  mode: GraphMode
  filters: GraphFilters
  selectedNodeId: string | null
  focusBubbleNodeId: string | null
  recenterKey: number
  theme: ThemeMode
  onSelectNode: (id: string | null) => void
  onDoubleClickNode: (id: string) => void
  onUpdateNodes: (nodes: GraphNode[]) => void
}

// ── Color palettes ─────────────────────────────────────────────────────────
const NODE_COLORS: Record<NodeType, string> = {
  File: '#3B82F6',
  Module: '#8B5CF6',
  Struct: '#06B6D4',
  Enum: '#F59E0B',
  Trait: '#10B981',
  Impl: '#6366F1',
  Function: '#EC4899',
  Method: '#F97316',
  Macro: '#EF4444',
  ExternalCrate: '#7D8795',
}

const EDGE_COLORS: Record<EdgeType, string> = {
  Contains: '#374151',
  Uses: '#4B5870',
  Calls: '#06B6D4',
  Implements: '#10B981',
  TypeReference: '#3B82F6',
  DataFlow: '#8B5CF6',
  ModDeclaration: '#6366F1',
  ExternalDependency: '#374151',
}

const NODE_SIZES: Record<NodeType, number> = {
  Module: 26,
  ExternalCrate: 24,
  File: 18,
  Struct: 18,
  Enum: 18,
  Trait: 18,
  Impl: 10,
  Function: 14,
  Method: 12,
  Macro: 12,
}

// ── Physics constants ───────────────────────────────────────────────────────
const REPULSION = 6000
const SPRING_STRENGTH = 0.04
const SPRING_LENGTH = 120
const DAMPING = 0.82
const CENTER_GRAVITY = 0.008
const MAX_SPEED = 8

interface CanvasTheme {
  bg: string
  bg2: string
  card: string
  surface: string
  border: string
  text: string
  textMuted: string
  gridDot: string
  focusMask: string
}

function canvasTheme(): CanvasTheme {
  const styles = getComputedStyle(document.documentElement)
  const read = (name: string, fallback: string) => styles.getPropertyValue(name).trim() || fallback
  return {
    bg: read('--cc-graph-bg', '#eef4fb'),
    bg2: read('--cc-graph-bg-2', '#f8fbff'),
    card: read('--cc-card', '#ffffff'),
    surface: read('--cc-minimap-bg', '#f8fbff'),
    border: read('--cc-border-strong', '#b7c6d8'),
    text: read('--cc-text', '#172033'),
    textMuted: read('--cc-text-muted', '#52647a'),
    gridDot: read('--cc-grid-dot', 'rgba(30,64,112,0.14)'),
    focusMask: read('--cc-focus-mask', 'rgba(255,255,255,0.68)'),
  }
}

function getNodeBounds(nodes: GraphNode[]) {
  if (nodes.length === 0) return null
  let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity
  for (const n of nodes) {
    const size = NODE_SIZES[n.type] ?? 16
    minX = Math.min(minX, n.x - size)
    maxX = Math.max(maxX, n.x + size)
    minY = Math.min(minY, n.y - size)
    maxY = Math.max(maxY, n.y + size)
  }
  return { minX, maxX, minY, maxY, width: Math.max(1, maxX - minX), height: Math.max(1, maxY - minY) }
}

function fitGraphToView(nodes: GraphNode[], canvasW: number, canvasH: number, pan: { x: number; y: number }, zoomRef: { current: number }) {
  const bounds = getNodeBounds(nodes)
  if (!bounds || canvasW <= 0 || canvasH <= 0) return
  const margin = 96
  const availableW = Math.max(1, canvasW - margin * 2)
  const availableH = Math.max(1, canvasH - margin * 2)
  const nextZoom = Math.max(0.35, Math.min(1.6, Math.min(availableW / bounds.width, availableH / bounds.height)))
  const centerX = (bounds.minX + bounds.maxX) / 2
  const centerY = (bounds.minY + bounds.maxY) / 2
  zoomRef.current = nextZoom
  pan.x = canvasW / 2 - centerX * nextZoom
  pan.y = canvasH / 2 - centerY * nextZoom
}

function runPhysicsTick(nodes: GraphNode[], edges: GraphEdge[], width: number, height: number): GraphNode[] {
  const updated = nodes.map(n => ({ ...n }))
  const index = new Map(updated.map(n => [n.id, n]))

  // repulsion between all pairs
  for (let i = 0; i < updated.length; i++) {
    for (let j = i + 1; j < updated.length; j++) {
      const a = updated[i], b = updated[j]
      const dx = b.x - a.x
      const dy = b.y - a.y
      const dist = Math.sqrt(dx * dx + dy * dy) || 1
      const force = REPULSION / (dist * dist)
      const fx = (dx / dist) * force
      const fy = (dy / dist) * force
      a.vx -= fx; a.vy -= fy
      b.vx += fx; b.vy += fy
    }
  }

  // spring attraction along edges
  for (const edge of edges) {
    const a = index.get(edge.source)
    const b = index.get(edge.target)
    if (!a || !b) continue
    const dx = b.x - a.x
    const dy = b.y - a.y
    const dist = Math.sqrt(dx * dx + dy * dy) || 1
    const stretch = dist - SPRING_LENGTH
    const force = SPRING_STRENGTH * stretch
    const fx = (dx / dist) * force
    const fy = (dy / dist) * force
    a.vx += fx; a.vy += fy
    b.vx -= fx; b.vy -= fy
  }

  // center gravity + integrate
  for (const n of updated) {
    if (n.pinned) { n.vx = 0; n.vy = 0; continue }
    n.vx += -n.x * CENTER_GRAVITY
    n.vy += -n.y * CENTER_GRAVITY
    n.vx *= DAMPING
    n.vy *= DAMPING
    const speed = Math.sqrt(n.vx * n.vx + n.vy * n.vy)
    if (speed > MAX_SPEED) { n.vx = (n.vx / speed) * MAX_SPEED; n.vy = (n.vy / speed) * MAX_SPEED }
    n.x += n.vx
    n.y += n.vy
  }
  return updated
}

// ── Canvas drawing helpers ─────────────────────────────────────────────────
function drawArrow(
  ctx: CanvasRenderingContext2D,
  x1: number,
  y1: number,
  x2: number,
  y2: number,
  color: string,
  width: number,
  dashed: boolean,
  animated: boolean,
  animT: number,
  sourceRadius: number,
  targetRadius: number,
) {
  const dx = x2 - x1, dy = y2 - y1
  const len = Math.sqrt(dx * dx + dy * dy) || 1
  const ux = dx / len, uy = dy / len
  const arrowSize = 9
  const sx = x1 + ux * (sourceRadius + 4), sy = y1 + uy * (sourceRadius + 4)
  const tipX = x2 - ux * (targetRadius + 5), tipY = y2 - uy * (targetRadius + 5)
  const ex = tipX - ux * arrowSize, ey = tipY - uy * arrowSize

  ctx.save()
  ctx.strokeStyle = color
  ctx.lineWidth = width
  if (dashed) ctx.setLineDash([5, 4])
  else ctx.setLineDash([])
  ctx.globalAlpha = 0.7
  ctx.beginPath()
  ctx.moveTo(sx, sy)
  ctx.lineTo(ex, ey)
  ctx.stroke()

  // animated pulse dot
  if (animated) {
    const progress = (animT % 1800) / 1800
    const px = sx + (tipX - sx) * progress, py = sy + (tipY - sy) * progress
    ctx.globalAlpha = 0.9
    ctx.fillStyle = color
    ctx.setLineDash([])
    ctx.beginPath()
    ctx.arc(px, py, 3, 0, Math.PI * 2)
    ctx.fill()
  }

  // arrowhead
  {
    ctx.globalAlpha = 0.85
    ctx.fillStyle = color
    ctx.setLineDash([])
    ctx.beginPath()
    const px1 = ex - uy * arrowSize * 0.5, py1 = ey + ux * arrowSize * 0.5
    const px2 = ex + uy * arrowSize * 0.5, py2 = ey - ux * arrowSize * 0.5
    ctx.moveTo(tipX, tipY)
    ctx.lineTo(px1, py1)
    ctx.lineTo(px2, py2)
    ctx.closePath()
    ctx.fill()
  }
  ctx.restore()
}

function drawNode(ctx: CanvasRenderingContext2D, n: GraphNode, isSelected: boolean, isHovered: boolean, isFocusContext: boolean, isFaded: boolean, theme: CanvasTheme) {
  const color = NODE_COLORS[n.type]
  const size = NODE_SIZES[n.type]
  const alpha = isFaded ? 0.18 : isFocusContext ? 1 : 1

  ctx.save()
  ctx.globalAlpha = alpha

  // glow
  if (isSelected || isHovered || n.bookmarked) {
    ctx.shadowBlur = isSelected ? 28 : isHovered ? 18 : 10
    ctx.shadowColor = color
  } else {
    ctx.shadowBlur = 6
    ctx.shadowColor = color
  }

  const fillColor = theme.card
  ctx.fillStyle = fillColor
  ctx.strokeStyle = isSelected ? theme.text : isHovered ? color : color
  ctx.lineWidth = isSelected ? 2.5 : isHovered ? 2 : 1.5

  if (n.type === 'ExternalCrate') {
    ctx.setLineDash([4, 3])
  } else {
    ctx.setLineDash([])
  }

  switch (n.type) {
    case 'File': {
      const w = size * 1.6, h = size * 2
      ctx.beginPath()
      ctx.roundRect(n.x - w / 2, n.y - h / 2, w, h, 4)
      ctx.fill()
      ctx.stroke()
      break
    }
    case 'Module': {
      ctx.beginPath()
      ctx.roundRect(n.x - size, n.y - size * 0.7, size * 2, size * 1.4, 8)
      ctx.fill()
      ctx.stroke()
      // inner accent bar
      ctx.fillStyle = color
      ctx.globalAlpha = alpha * 0.35
      ctx.beginPath()
      ctx.roundRect(n.x - size + 4, n.y - size * 0.7 + 3, (size * 2 - 8) * 0.4, 3, 2)
      ctx.fill()
      break
    }
    case 'Enum': {
      // rotated diamond
      ctx.beginPath()
      ctx.moveTo(n.x, n.y - size)
      ctx.lineTo(n.x + size * 0.75, n.y)
      ctx.lineTo(n.x, n.y + size)
      ctx.lineTo(n.x - size * 0.75, n.y)
      ctx.closePath()
      ctx.fill()
      ctx.stroke()
      break
    }
    case 'Trait': {
      ctx.setLineDash([5, 3])
      ctx.beginPath()
      ctx.roundRect(n.x - size, n.y - size * 0.65, size * 2, size * 1.3, 6)
      ctx.stroke()
      ctx.setLineDash([])
      ctx.globalAlpha = alpha * 0.15
      ctx.fill()
      break
    }
    case 'Impl': {
      ctx.beginPath()
      ctx.arc(n.x, n.y, size, 0, Math.PI * 2)
      ctx.fill()
      ctx.stroke()
      // inner dot
      ctx.fillStyle = color
      ctx.globalAlpha = alpha * 0.6
      ctx.beginPath()
      ctx.arc(n.x, n.y, size * 0.4, 0, Math.PI * 2)
      ctx.fill()
      break
    }
    case 'Method': {
      ctx.beginPath()
      ctx.arc(n.x, n.y, size, 0, Math.PI * 2)
      ctx.fill()
      ctx.stroke()
      // dot
      ctx.fillStyle = color
      ctx.globalAlpha = alpha * 0.8
      ctx.beginPath()
      ctx.arc(n.x + size * 0.55, n.y - size * 0.55, 3, 0, Math.PI * 2)
      ctx.fill()
      break
    }
    case 'Macro': {
      ctx.beginPath()
      ctx.arc(n.x, n.y, size, 0, Math.PI * 2)
      ctx.fill()
      ctx.stroke()
      // lightning symbol
      ctx.fillStyle = color
      ctx.globalAlpha = alpha * 0.9
      ctx.font = `bold ${size}px sans-serif`
      ctx.textAlign = 'center'
      ctx.textBaseline = 'middle'
      ctx.fillText('⚡', n.x, n.y + 1)
      break
    }
    default: {
      // Function, Method, Struct, ExternalCrate
      ctx.beginPath()
      if (n.type === 'Struct') {
        ctx.roundRect(n.x - size, n.y - size * 0.65, size * 2, size * 1.3, 5)
      } else {
        ctx.arc(n.x, n.y, size, 0, Math.PI * 2)
      }
      ctx.fill()
      ctx.stroke()
    }
  }

  ctx.restore()
}

function drawLabel(ctx: CanvasRenderingContext2D, n: GraphNode, isSelected: boolean, isHovered: boolean, theme: CanvasTheme) {
  const size = NODE_SIZES[n.type]
  const color = NODE_COLORS[n.type]
  ctx.save()
  ctx.font = `${isSelected ? '700' : '500'} ${isSelected || isHovered ? 12 : 10}px Inter, sans-serif`
  ctx.textAlign = 'center'
  ctx.textBaseline = 'top'
  const offsetY = n.type === 'Module' ? size * 0.7 + 5 : size + 5
  // shadow for readability
  ctx.shadowColor = theme.card
  ctx.shadowBlur = 5
  ctx.fillStyle = isSelected ? theme.text : isHovered ? color : theme.textMuted
  ctx.fillText(n.label, n.x, n.y + offsetY)
  ctx.restore()
}

// ── MiniMap ────────────────────────────────────────────────────────────────
function drawMiniMap(ctx: CanvasRenderingContext2D, nodes: GraphNode[], pan: { x: number; y: number }, zoom: number, canvasW: number, canvasH: number, theme: CanvasTheme) {
  const mmW = 160, mmH = 100, mmX = canvasW - mmW - 16, mmY = canvasH - mmH - 16
  const innerX = mmX + 8, innerY = mmY + 8, innerW = mmW - 16, innerH = mmH - 22
  ctx.save()
  ctx.globalAlpha = 0.88
  ctx.fillStyle = theme.surface
  ctx.strokeStyle = theme.border
  ctx.lineWidth = 1
  ctx.beginPath()
  ctx.roundRect(mmX, mmY, mmW, mmH, 6)
  ctx.fill()
  ctx.stroke()

  const graphBounds = getNodeBounds(nodes)
  if (!graphBounds) {
    ctx.restore()
    return
  }
  const pad = 90
  const viewMinX = -pan.x / zoom
  const viewMinY = -pan.y / zoom
  const viewMaxX = viewMinX + canvasW / zoom
  const viewMaxY = viewMinY + canvasH / zoom
  const graphCenterX = (graphBounds.minX + graphBounds.maxX) / 2
  const graphCenterY = (graphBounds.minY + graphBounds.maxY) / 2
  const worldW = Math.max(graphBounds.width + pad * 2, canvasW / 0.9, 420)
  const worldH = Math.max(graphBounds.height + pad * 2, canvasH / 0.9, 280)
  const minX = graphCenterX - worldW / 2
  const minY = graphCenterY - worldH / 2
  const scaleX = innerW / worldW
  const scaleY = innerH / worldH
  const s = Math.min(scaleX, scaleY)
  const mapW = worldW * s
  const mapH = worldH * s
  const mapX = innerX + (innerW - mapW) / 2
  const mapY = innerY + (innerH - mapH) / 2

  ctx.save()
  ctx.beginPath()
  ctx.rect(innerX, innerY, innerW, innerH)
  ctx.clip()

  for (const n of nodes) {
    const nx = mapX + (n.x - minX) * s
    const ny = mapY + (n.y - minY) * s
    ctx.fillStyle = NODE_COLORS[n.type]
    ctx.globalAlpha = 0.7
    ctx.beginPath()
    ctx.arc(nx, ny, 2.5, 0, Math.PI * 2)
    ctx.fill()
  }

  // viewport indicator
  {
    const vpX = mapX + (viewMinX - minX) * s
    const vpY = mapY + (viewMinY - minY) * s
    const vpW = Math.max(3, (viewMaxX - viewMinX) * s)
    const vpH = Math.max(3, (viewMaxY - viewMinY) * s)
    ctx.globalAlpha = 0.16
    ctx.fillStyle = '#06B6D4'
    ctx.fillRect(vpX, vpY, vpW, vpH)
    ctx.globalAlpha = 0.75
    ctx.strokeStyle = '#06B6D4'
    ctx.lineWidth = 1.5
    ctx.setLineDash([])
    ctx.strokeRect(vpX, vpY, vpW, vpH)
  }
  ctx.restore()

  ctx.restore()
}

// ── Component ──────────────────────────────────────────────────────────────
export function LiveCodeGraph({ nodes, edges, mode, filters, selectedNodeId, focusBubbleNodeId, recenterKey, theme, onSelectNode, onDoubleClickNode, onUpdateNodes }: LiveCodeGraphProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const nodesRef = useRef<GraphNode[]>(nodes)
  const animFrameRef = useRef<number>(0)
  const animTimeRef = useRef<number>(0)
  const panRef = useRef({ x: 0, y: 0 })
  const zoomRef = useRef(1)
  const isDraggingRef = useRef(false)
  const dragStartRef = useRef({ x: 0, y: 0, panX: 0, panY: 0 })
  const dragNodeRef = useRef<string | null>(null)
  const hoveredNodeRef = useRef<string | null>(null)
  const userNavigatedRef = useRef(false)
  const graphSignatureRef = useRef('')
  const [hoveredNode, setHoveredNode] = useState<string | null>(null)
  const physicsTicksRef = useRef(0)

  const fitCurrentGraph = useCallback((force = false) => {
    const canvas = canvasRef.current
    if (!canvas || nodesRef.current.length === 0) return
    if (!force && userNavigatedRef.current) return
    const rect = canvas.getBoundingClientRect()
    fitGraphToView(nodesRef.current, rect.width, rect.height, panRef.current, zoomRef)
  }, [])

  // keep nodesRef in sync
  useEffect(() => {
    const signature = nodes.map(n => n.id).join('|')
    nodesRef.current = nodes
    if (signature !== graphSignatureRef.current) {
      graphSignatureRef.current = signature
      physicsTicksRef.current = 0
      userNavigatedRef.current = false
      requestAnimationFrame(() => fitCurrentGraph(true))
    }
  }, [fitCurrentGraph, nodes])

  useEffect(() => {
    userNavigatedRef.current = false
    fitCurrentGraph(true)
  }, [fitCurrentGraph, recenterKey])

  const toWorld = useCallback((cx: number, cy: number) => {
    const canvas = canvasRef.current!
    const rect = canvas.getBoundingClientRect()
    const x = (cx - rect.left - panRef.current.x) / zoomRef.current
    const y = (cy - rect.top - panRef.current.y) / zoomRef.current
    return { x, y }
  }, [])

  const hitTest = useCallback((cx: number, cy: number): GraphNode | null => {
    const { x, y } = toWorld(cx, cy)
    for (let i = nodesRef.current.length - 1; i >= 0; i--) {
      const n = nodesRef.current[i]
      const size = NODE_SIZES[n.type]
      const dist = Math.sqrt((n.x - x) ** 2 + (n.y - y) ** 2)
      if (dist <= size + 6) return n
    }
    return null
  }, [toWorld])

  const getVisibleNodeIds = useCallback(() => {
    const visible = new Set<string>()
    const pickDepthCenter = () => {
      if (focusBubbleNodeId) return focusBubbleNodeId
      if (selectedNodeId) return selectedNodeId
      const nodeById = new Map(nodesRef.current.map(n => [n.id, n]))
      const mainNode = nodesRef.current.find(n => n.type === 'Function' && n.label === 'main')
      if (mainNode) return mainNode.id
      const degree = new Map<string, number>()
      edges.forEach(e => {
        degree.set(e.source, (degree.get(e.source) ?? 0) + 1)
        degree.set(e.target, (degree.get(e.target) ?? 0) + 1)
      })
      const semanticHub = [...degree.entries()]
        .map(([id, count]) => ({ node: nodeById.get(id), count }))
        .filter((entry): entry is { node: GraphNode; count: number } => !!entry.node && entry.node.type !== 'File' && entry.node.type !== 'Module')
        .sort((a, b) => b.count - a.count)[0]
      if (semanticHub) return semanticHub.node.id
      return [...degree.entries()].sort((a, b) => b[1] - a[1])[0]?.[0] ?? nodesRef.current[0]?.id ?? null
    }

    const centerId = filters.depth === 'full' ? null : pickDepthCenter()
    if (!centerId) {
      nodesRef.current.forEach(n => visible.add(n.id))
      return visible
    }
    // depth mode: show center + neighbors up to D1/D2/D3.
    visible.add(centerId)
    const depth = typeof filters.depth === 'number' ? filters.depth : 99
    const expand = (id: string, d: number) => {
      if (d <= 0) return
      edges.forEach(e => {
        if (e.source === id && !visible.has(e.target)) { visible.add(e.target); expand(e.target, d - 1) }
        if (e.target === id && !visible.has(e.source)) { visible.add(e.source); expand(e.source, d - 1) }
      })
    }
    expand(centerId, depth)
    return visible
  }, [focusBubbleNodeId, selectedNodeId, edges, filters.depth])

  // main render + physics loop
  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const ctx = canvas.getContext('2d')!
    const dpr = window.devicePixelRatio || 1

    const resize = () => {
      const rect = canvas.getBoundingClientRect()
      canvas.width = rect.width * dpr
      canvas.height = rect.height * dpr
      ctx.scale(dpr, dpr)
      fitCurrentGraph(false)
    }
    resize()
    const ro = new ResizeObserver(resize)
    ro.observe(canvas)

    const loop = (ts: number) => {
      animTimeRef.current = ts
      const W = canvas.width / dpr
      const H = canvas.height / dpr
      const canvasColors = canvasTheme()

      // run physics for first 300 ticks then slow down
      if (physicsTicksRef.current < 300) {
        nodesRef.current = runPhysicsTick(nodesRef.current, edges, W, H)
        physicsTicksRef.current++
        if (!userNavigatedRef.current && physicsTicksRef.current < 120) {
          fitGraphToView(nodesRef.current, W, H, panRef.current, zoomRef)
        }
        if (physicsTicksRef.current % 60 === 0) {
          onUpdateNodes([...nodesRef.current])
        }
      }

      ctx.clearRect(0, 0, W, H)

      // background
      const bg = ctx.createRadialGradient(W / 2, H / 2, 0, W / 2, H / 2, Math.max(W, H) * 0.6)
      bg.addColorStop(0, canvasColors.bg2)
      bg.addColorStop(1, canvasColors.bg)
      ctx.fillStyle = bg
      ctx.fillRect(0, 0, W, H)

      // subtle dot grid
      ctx.save()
      ctx.translate(panRef.current.x, panRef.current.y)
      ctx.scale(zoomRef.current, zoomRef.current)
      const gridSpacing = 40
      const startX = Math.floor(-panRef.current.x / zoomRef.current / gridSpacing - 1) * gridSpacing
      const startY = Math.floor(-panRef.current.y / zoomRef.current / gridSpacing - 1) * gridSpacing
      const endX = startX + (W / zoomRef.current) + gridSpacing * 2
      const endY = startY + (H / zoomRef.current) + gridSpacing * 2
      ctx.fillStyle = canvasColors.gridDot
      for (let gx = startX; gx < endX; gx += gridSpacing) {
        for (let gy = startY; gy < endY; gy += gridSpacing) {
          ctx.beginPath()
          ctx.arc(gx, gy, 1, 0, Math.PI * 2)
          ctx.fill()
        }
      }

      const visibleIds = getVisibleNodeIds()
      const nodeMap = new Map(nodesRef.current.map(n => [n.id, n]))
      const hoveredConnections = new Set<string>()
      const selectedConnections = new Set<string>()
      if (hoveredNodeRef.current) {
        edges.forEach(e => {
          if (e.source === hoveredNodeRef.current || e.target === hoveredNodeRef.current) {
            hoveredConnections.add(e.source)
            hoveredConnections.add(e.target)
          }
        })
      }
      if (selectedNodeId) {
        edges.forEach(e => {
          if (e.source === selectedNodeId || e.target === selectedNodeId) {
            selectedConnections.add(e.source)
            selectedConnections.add(e.target)
          }
        })
      }

      // draw edges
      for (const edge of edges) {
        if (!filters.edgeTypes.has(edge.type)) continue
        const src = nodeMap.get(edge.source), tgt = nodeMap.get(edge.target)
        if (!src || !tgt) continue
        if (!visibleIds.has(src.id) || !visibleIds.has(tgt.id)) continue

        const isActive = hoveredConnections.has(edge.source) && hoveredConnections.has(edge.target)
          || selectedConnections.has(edge.source) && selectedConnections.has(edge.target)
        const baseColor = EDGE_COLORS[edge.type]
        const color = isActive ? baseColor : baseColor + '99'
        const width = edge.type === 'DataFlow' ? 2.5 : edge.type === 'Calls' ? 1.8 : 1.2
        const dashed = edge.type === 'Implements' || edge.type === 'ExternalDependency'
        const animated = edge.type === 'DataFlow'

        drawArrow(ctx, src.x, src.y, tgt.x, tgt.y, color, width, dashed, animated, ts, NODE_SIZES[src.type], NODE_SIZES[tgt.type])
      }

      // draw focus bubble background
      if (focusBubbleNodeId) {
        ctx.fillStyle = canvasColors.focusMask
        ctx.fillRect(-panRef.current.x / zoomRef.current - 2000, -panRef.current.y / zoomRef.current - 2000, 4000 + W / zoomRef.current, 4000 + H / zoomRef.current)
      }

      // draw nodes
      for (const n of nodesRef.current) {
        if (!filters.nodeTypes.has(n.type)) continue
        if (!visibleIds.has(n.id)) continue
        const isSelected = n.id === selectedNodeId
        const isHovered = n.id === hoveredNodeRef.current
        const isFocusContext = visibleIds.has(n.id)
        const isFaded = (focusBubbleNodeId !== null && !isFocusContext)
          || (hoveredNodeRef.current !== null && !hoveredConnections.has(n.id) && !isHovered && n.id !== hoveredNodeRef.current)

        drawNode(ctx, n, isSelected, isHovered, isFocusContext, isFaded, canvasColors)
        drawLabel(ctx, n, isSelected, isHovered, canvasColors)

        // pin indicator
        if (n.pinned) {
          ctx.save()
          ctx.fillStyle = '#F59E0B'
          ctx.globalAlpha = 0.9
          ctx.font = '10px sans-serif'
          ctx.textAlign = 'center'
          ctx.fillText('📌', n.x + NODE_SIZES[n.type] + 2, n.y - NODE_SIZES[n.type] - 2)
          ctx.restore()
        }
      }

      ctx.restore()

      // minimap (screen-space)
      drawMiniMap(ctx, nodesRef.current.filter(n => visibleIds.has(n.id)), panRef.current, zoomRef.current, W, H, canvasColors)

      // "You are here" breadcrumb for selected
      if (selectedNodeId) {
        const sel = nodeMap.get(selectedNodeId)
        if (sel) {
          const sx = sel.x * zoomRef.current + panRef.current.x
          const sy = sel.y * zoomRef.current + panRef.current.y
          if (sx > 0 && sx < W && sy > 0 && sy < H) {
            ctx.save()
            ctx.strokeStyle = '#06B6D4'
            ctx.lineWidth = 2
            ctx.globalAlpha = 0.6
            ctx.setLineDash([4, 4])
            ctx.beginPath()
            ctx.arc(sx, sy, (NODE_SIZES[sel.type] + 8) * zoomRef.current, 0, Math.PI * 2)
            ctx.stroke()
            ctx.restore()
          }
        }
      }

      animFrameRef.current = requestAnimationFrame(loop)
    }

    animFrameRef.current = requestAnimationFrame(loop)
    return () => {
      cancelAnimationFrame(animFrameRef.current)
      ro.disconnect()
    }
  }, [edges, selectedNodeId, focusBubbleNodeId, filters, getVisibleNodeIds, onUpdateNodes, theme])

  // mouse events
  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    const hit = hitTest(e.clientX, e.clientY)
    if (hit) {
      dragNodeRef.current = hit.id
      isDraggingRef.current = false
    } else {
      dragNodeRef.current = null
      isDraggingRef.current = true
      userNavigatedRef.current = true
      dragStartRef.current = { x: e.clientX, y: e.clientY, panX: panRef.current.x, panY: panRef.current.y }
    }
  }, [hitTest])

  const handleMouseMove = useCallback((e: React.MouseEvent) => {
    const hit = hitTest(e.clientX, e.clientY)
    const newHovered = hit?.id ?? null
    if (newHovered !== hoveredNodeRef.current) {
      hoveredNodeRef.current = newHovered
      setHoveredNode(newHovered)
    }

    if (dragNodeRef.current) {
      const { x, y } = toWorld(e.clientX, e.clientY)
      nodesRef.current = nodesRef.current.map(n =>
        n.id === dragNodeRef.current ? { ...n, x, y, vx: 0, vy: 0 } : n
      )
    } else if (isDraggingRef.current) {
      panRef.current.x = dragStartRef.current.panX + (e.clientX - dragStartRef.current.x)
      panRef.current.y = dragStartRef.current.panY + (e.clientY - dragStartRef.current.y)
    }
  }, [hitTest, toWorld])

  const handleMouseUp = useCallback((e: React.MouseEvent) => {
    if (dragNodeRef.current && !isDraggingRef.current) {
      // it was a click, not a drag
    }
    dragNodeRef.current = null
    isDraggingRef.current = false
  }, [])

  const handleClick = useCallback((e: React.MouseEvent) => {
    const hit = hitTest(e.clientX, e.clientY)
    onSelectNode(hit?.id ?? null)
  }, [hitTest, onSelectNode])

  const handleDoubleClick = useCallback((e: React.MouseEvent) => {
    const hit = hitTest(e.clientX, e.clientY)
    if (hit) onDoubleClickNode(hit.id)
  }, [hitTest, onDoubleClickNode])

  const handleWheel = useCallback((e: React.WheelEvent) => {
    e.preventDefault()
    userNavigatedRef.current = true
    const canvas = canvasRef.current!
    const rect = canvas.getBoundingClientRect()
    const mx = e.clientX - rect.left, my = e.clientY - rect.top
    const delta = -e.deltaY * 0.001
    const newZoom = Math.max(0.15, Math.min(4, zoomRef.current * (1 + delta)))
    panRef.current.x = mx - (mx - panRef.current.x) * (newZoom / zoomRef.current)
    panRef.current.y = my - (my - panRef.current.y) * (newZoom / zoomRef.current)
    zoomRef.current = newZoom
  }, [])

  return (
    <div className="relative w-full h-full overflow-hidden">
      <canvas
        ref={canvasRef}
        className="w-full h-full"
        style={{ cursor: hoveredNode ? 'pointer' : 'grab', background: 'var(--cc-bg)' }}
        onMouseDown={handleMouseDown}
        onMouseMove={handleMouseMove}
        onMouseUp={handleMouseUp}
        onClick={handleClick}
        onDoubleClick={handleDoubleClick}
        onWheel={handleWheel}
      />
    </div>
  )
}
