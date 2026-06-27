import { useCallback, useEffect, useRef, useState } from 'react'
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

const NODE_COLORS: Record<NodeType, string> = {
  File: '#3B82F6',
  Module: '#8B5CF6',
  Struct: '#06B6D4',
  Class: '#0EA5E9',
  Object: '#38BDF8',
  Enum: '#F59E0B',
  Trait: '#10B981',
  Impl: '#6366F1',
  Function: '#EC4899',
  Method: '#F97316',
  Component: '#14B8A6',
  Hook: '#A855F7',
  Interface: '#22C55E',
  TypeAlias: '#84CC16',
  Property: '#FACC15',
  Signal: '#FB7185',
  Handler: '#F472B6',
  Endpoint: '#E11D48',
  Macro: '#EF4444',
  ExternalCrate: '#7D8795',
}

const EDGE_COLORS: Record<GraphEdge['type'], string> = {
  Contains: '#475569',
  Imports: '#64748B',
  Uses: '#4B5870',
  Calls: '#06B6D4',
  Renders: '#14B8A6',
  ApiCall: '#E11D48',
  EndpointHandler: '#F97316',
  Implements: '#10B981',
  TypeReference: '#3B82F6',
  DataFlow: '#8B5CF6',
  ModDeclaration: '#6366F1',
  ExternalDependency: '#64748B',
}

const NODE_SIZES: Record<NodeType, number> = {
  Module: 26,
  ExternalCrate: 18,
  File: 18,
  Struct: 18,
  Class: 18,
  Object: 17,
  Enum: 18,
  Trait: 18,
  Impl: 10,
  Function: 14,
  Method: 12,
  Component: 18,
  Hook: 13,
  Interface: 16,
  TypeAlias: 15,
  Property: 10,
  Signal: 11,
  Handler: 12,
  Endpoint: 16,
  Macro: 12,
}

const GOLDEN_ANGLE = Math.PI * (3 - Math.sqrt(5))
const BASE_REPULSION = 9400
const SPRING_STRENGTH = 0.027
const BASE_SPRING_LENGTH = 132
const CENTER_GRAVITY = 0.0033
const COLLISION_PADDING = 18
const COLLISION_STRENGTH = 0.16
const MAX_FORCE = 46
const MAX_NODE_FORCE = 24
const MAX_SPEED = 13
const VISIBLE_SETTLE_MS = 1500
const SETTLE_FADE_MS = 900
const FIT_SETTLE_MS = 430
const SPATIAL_GRID_THRESHOLD = 260
const SPATIAL_CELL_SIZE = 250
const SPATIAL_SEARCH_RADIUS = 2
const MINIMAP_W = 160
const MINIMAP_H = 100

type CanvasTheme = {
  bg: string
  bg2: string
  card: string
  surface: string
  border: string
  text: string
  textMuted: string
  gridDot: string
}

type DragState =
  | { kind: 'node'; id: string; startX: number; startY: number; moved: boolean }
  | { kind: 'pan'; startX: number; startY: number; panX: number; panY: number; moved: boolean }
  | null

type LabelCandidate = {
  node: GraphNode
  degree: number
  isSelected: boolean
  isHovered: boolean
  priority: number
}

type PhysicsOptions = {
  dampingScale?: number
  dragScale?: number
  maxSpeedScale?: number
  maxForceScale?: number
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
  }
}

function stablePoint(id: string, index: number, count: number) {
  let hash = 0
  for (let i = 0; i < id.length; i++) hash = (hash * 31 + id.charCodeAt(i)) | 0
  const angle = index * GOLDEN_ANGLE + (Math.abs(hash) % 97) / 97
  const radius = Math.max(120, Math.sqrt(Math.max(1, count)) * 52) * Math.sqrt((index + 1) / Math.max(1, count))
  return { x: Math.cos(angle) * radius, y: Math.sin(angle) * radius }
}

function languageSeedOffset(node: GraphNode) {
  switch (inferNodeLanguage(node)) {
    case 'typescript': return { x: -420, y: 90 }
    case 'rust': return { x: 260, y: -10 }
    case 'python': return { x: 140, y: 360 }
    case 'qml': return { x: -330, y: 360 }
    case 'endpoints': return { x: -40, y: -300 }
    case 'external': return { x: 610, y: 30 }
    default: return { x: 0, y: 0 }
  }
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
    const offset = languageSeedOffset(node)
    return {
      ...node,
      x: (point.x + offset.x) * spacing,
      y: (point.y + offset.y) * spacing,
      vx: 0,
      vy: 0,
    }
  })
}

function getNodeBounds(nodes: GraphNode[]) {
  if (nodes.length === 0) return null
  let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity
  for (const node of nodes) {
    const size = NODE_SIZES[node.type] ?? 16
    const labelWidthPad = Math.min(120, Math.max(36, node.label.length * 4.3))
    const labelHeightPad = node.file ? 42 : 28
    minX = Math.min(minX, node.x - size - labelWidthPad)
    maxX = Math.max(maxX, node.x + size + labelWidthPad)
    minY = Math.min(minY, node.y - size - 28)
    maxY = Math.max(maxY, node.y + size + labelHeightPad)
  }
  return { minX, maxX, minY, maxY, width: Math.max(1, maxX - minX), height: Math.max(1, maxY - minY) }
}

function fitGraphToView(nodes: GraphNode[], canvasW: number, canvasH: number, pan: { x: number; y: number }, zoomRef: { current: number }) {
  const bounds = getNodeBounds(nodes)
  if (!bounds || canvasW <= 0 || canvasH <= 0) return
  const margin = 92
  const availableW = Math.max(1, canvasW - margin * 2)
  const availableH = Math.max(1, canvasH - margin * 2)
  const fitZoom = Math.min(availableW / bounds.width, availableH / bounds.height)
  const densityZoom = nodes.length > 55 ? Math.min(1.75, 1 + Math.log(nodes.length / 55) * 0.28) : 1
  const nextZoom = Math.max(0.22, Math.min(1.65, fitZoom * densityZoom))
  const centerX = (bounds.minX + bounds.maxX) / 2
  const centerY = (bounds.minY + bounds.maxY) / 2
  zoomRef.current = nextZoom
  pan.x = canvasW / 2 - centerX * nextZoom
  pan.y = canvasH / 2 - centerY * nextZoom
}

function graphSizeScale(nodeCount: number) {
  return Math.min(3.8, Math.max(1, Math.pow(Math.max(1, nodeCount) / 38, 0.38)))
}

function nodeRadius(node: GraphNode) {
  return NODE_SIZES[node.type] ?? 14
}

function clampForce(value: number, limit: number) {
  return Math.max(-limit, Math.min(limit, value))
}

function activeGraph(nodes: GraphNode[], edges: GraphEdge[], filters: GraphFilters) {
  const activeNodes = nodes.filter(node => filters.nodeTypes.has(node.type))
  const activeIds = new Set(activeNodes.map(node => node.id))
  const activeEdges = edges.filter(edge => filters.edgeTypes.has(edge.type) && activeIds.has(edge.source) && activeIds.has(edge.target))
  const visibleIds = activeIds
  return { visibleIds, activeNodes, activeEdges, activeIds }
}

function applyPairForce(a: GraphNode, b: GraphNode, forces: Map<string, { x: number; y: number }>, repulsion: number, collisionPadding: number) {
  const dx = b.x - a.x
  const dy = b.y - a.y
  const dist2 = Math.max(64, dx * dx + dy * dy)
  const dist = Math.sqrt(dist2)
  const minDist = nodeRadius(a) + nodeRadius(b) + collisionPadding
  let force = repulsion / dist2
  if (dist < minDist) {
    force += (minDist - dist) * COLLISION_STRENGTH
  }
  force = Math.min(MAX_FORCE, force)
  const fx = dx / dist * force
  const fy = dy / dist * force
  forces.get(a.id)!.x -= fx
  forces.get(a.id)!.y -= fy
  forces.get(b.id)!.x += fx
  forces.get(b.id)!.y += fy
}

function addRepulsion(activeNodes: GraphNode[], forces: Map<string, { x: number; y: number }>, repulsion: number, collisionPadding: number) {
  if (activeNodes.length < SPATIAL_GRID_THRESHOLD) {
    for (let i = 0; i < activeNodes.length; i++) {
      for (let j = i + 1; j < activeNodes.length; j++) {
        applyPairForce(activeNodes[i], activeNodes[j], forces, repulsion, collisionPadding)
      }
    }
    return
  }

  const cellSize = SPATIAL_CELL_SIZE
  const grid = new Map<string, GraphNode[]>()
  const keyFor = (x: number, y: number) => `${Math.floor(x / cellSize)}:${Math.floor(y / cellSize)}`
  for (const node of activeNodes) {
    const key = keyFor(node.x, node.y)
    const bucket = grid.get(key)
    if (bucket) bucket.push(node)
    else grid.set(key, [node])
  }

  const seen = new Set<string>()
  for (const node of activeNodes) {
    const cx = Math.floor(node.x / cellSize)
    const cy = Math.floor(node.y / cellSize)
    for (let gx = cx - SPATIAL_SEARCH_RADIUS; gx <= cx + SPATIAL_SEARCH_RADIUS; gx++) {
      for (let gy = cy - SPATIAL_SEARCH_RADIUS; gy <= cy + SPATIAL_SEARCH_RADIUS; gy++) {
        const bucket = grid.get(`${gx}:${gy}`)
        if (!bucket) continue
        for (const other of bucket) {
          if (other.id === node.id) continue
          const pair = node.id < other.id ? `${node.id}::${other.id}` : `${other.id}::${node.id}`
          if (seen.has(pair)) continue
          seen.add(pair)
          applyPairForce(node, other, forces, repulsion, collisionPadding)
        }
      }
    }
  }
}

function buildDegreeMap(nodes: GraphNode[], edges: GraphEdge[]) {
  const degree = new Map(nodes.map(node => [node.id, 0]))
  for (const edge of edges) {
    if (degree.has(edge.source)) degree.set(edge.source, (degree.get(edge.source) ?? 0) + (edge.bundledCount ?? 1))
    if (degree.has(edge.target)) degree.set(edge.target, (degree.get(edge.target) ?? 0) + (edge.bundledCount ?? 1))
  }
  return degree
}

function runPhysicsTick(
  nodes: GraphNode[],
  edges: GraphEdge[],
  filters: GraphFilters,
  layoutSettings: GraphLayoutSettings,
  options: PhysicsOptions = {},
) {
  const updated = nodes.map(node => ({ ...node }))
  const byId = new Map(updated.map(node => [node.id, node]))
  const { activeNodes, activeEdges, activeIds } = activeGraph(updated, edges, filters)
  const forces = new Map(activeNodes.map(node => [node.id, { x: 0, y: 0 }]))
  if (activeNodes.length === 0) return { nodes: updated, averageSpeed: 0 }

  const graphScale = graphSizeScale(activeNodes.length)
  const repulsion = BASE_REPULSION * layoutSettings.repulsion * graphScale * graphScale
  const collisionPadding = COLLISION_PADDING * Math.sqrt(graphScale) * layoutSettings.spacing
  addRepulsion(activeNodes, forces, repulsion, collisionPadding)

  const degree = buildDegreeMap(activeNodes, activeEdges)
  const baseLength = BASE_SPRING_LENGTH * layoutSettings.linkLength * layoutSettings.spacing * Math.sqrt(graphScale)
  for (const edge of activeEdges) {
    const source = byId.get(edge.source)
    const target = byId.get(edge.target)
    if (!source || !target) continue
    const dx = target.x - source.x
    const dy = target.y - source.y
    const dist = Math.sqrt(dx * dx + dy * dy) || 1
    const sourceDeg = degree.get(source.id) ?? 1
    const targetDeg = degree.get(target.id) ?? 1
    const hubScale = (sourceDeg > 8 || targetDeg > 8) ? 0.72 : 1
    const targetLength = baseLength * hubScale * (edge.type === 'Contains' ? 0.7 : edge.type === 'ExternalDependency' ? 1.35 : 1)
    const force = (dist - targetLength) * SPRING_STRENGTH
    const fx = dx / dist * force
    const fy = dy / dist * force
    forces.get(source.id)!.x += fx
    forces.get(source.id)!.y += fy
    forces.get(target.id)!.x -= fx
    forces.get(target.id)!.y -= fy
  }

  let speedSum = 0
  const damping = Math.max(0.62, Math.min(0.94, 1 - 0.11 * layoutSettings.damping / (options.dampingScale ?? 1)))
  const maxSpeed = MAX_SPEED * (options.maxSpeedScale ?? 1)
  const maxForce = MAX_NODE_FORCE * (options.maxForceScale ?? 1)
  const centerGravity = CENTER_GRAVITY * Math.sqrt(graphScale)

  for (const node of updated) {
    if (!activeIds.has(node.id)) continue
    if (node.pinned) {
      node.vx = 0
      node.vy = 0
      continue
    }
    const force = forces.get(node.id) ?? { x: 0, y: 0 }
    force.x -= node.x * centerGravity
    force.y -= node.y * centerGravity
    const deg = degree.get(node.id) ?? 0
    const hubDrag = deg > 8 ? 0.72 : 1
    node.vx = ((node.vx ?? 0) + clampForce(force.x, maxForce) * hubDrag) * damping
    node.vy = ((node.vy ?? 0) + clampForce(force.y, maxForce) * hubDrag) * damping
    node.vx = clampForce(node.vx, maxSpeed)
    node.vy = clampForce(node.vy, maxSpeed)
    node.x += node.vx
    node.y += node.vy
    speedSum += Math.sqrt(node.vx * node.vx + node.vy * node.vy)
  }

  return { nodes: updated, averageSpeed: speedSum / Math.max(1, activeNodes.length) }
}

function edgeColor(edge: GraphEdge) {
  return EDGE_COLORS[edge.type] ?? '#64748B'
}

function edgeWidth(edge: GraphEdge) {
  if (edge.type === 'DataFlow' || edge.type === 'ApiCall' || edge.type === 'EndpointHandler') return 2.4
  if (edge.type === 'Calls' || edge.type === 'Renders') return 1.7
  return 1.1
}

function drawArrow(ctx: CanvasRenderingContext2D, src: GraphNode, tgt: GraphNode, color: string, width: number, alpha: number, animated: boolean, ts: number) {
  const dx = tgt.x - src.x
  const dy = tgt.y - src.y
  const dist = Math.sqrt(dx * dx + dy * dy) || 1
  const ux = dx / dist
  const uy = dy / dist
  const startR = nodeRadius(src) + 3
  const endR = nodeRadius(tgt) + 6
  const x1 = src.x + ux * startR
  const y1 = src.y + uy * startR
  const x2 = tgt.x - ux * endR
  const y2 = tgt.y - uy * endR

  ctx.save()
  ctx.globalAlpha = alpha
  ctx.strokeStyle = color
  ctx.lineWidth = width
  ctx.setLineDash([])
  ctx.beginPath()
  ctx.moveTo(x1, y1)
  ctx.lineTo(x2, y2)
  ctx.stroke()

  if (animated) {
    const p = (ts % 1800) / 1800
    ctx.globalAlpha = Math.min(0.92, alpha + 0.2)
    ctx.fillStyle = color
    ctx.beginPath()
    ctx.arc(x1 + (x2 - x1) * p, y1 + (y2 - y1) * p, 2.8, 0, Math.PI * 2)
    ctx.fill()
  }

  if (dist > 28) {
    const arrowSize = 8
    const ex = x2 - ux * arrowSize
    const ey = y2 - uy * arrowSize
    ctx.globalAlpha = Math.min(0.85, alpha + 0.1)
    ctx.fillStyle = color
    ctx.beginPath()
    ctx.moveTo(x2, y2)
    ctx.lineTo(ex - uy * arrowSize * 0.48, ey + ux * arrowSize * 0.48)
    ctx.lineTo(ex + uy * arrowSize * 0.48, ey - ux * arrowSize * 0.48)
    ctx.closePath()
    ctx.fill()
  }
  ctx.restore()
}

function drawNode(ctx: CanvasRenderingContext2D, node: GraphNode, isSelected: boolean, isHovered: boolean, isFaded: boolean, theme: CanvasTheme) {
  const typeColor = NODE_COLORS[node.type] ?? '#7D8795'
  const language = inferNodeLanguage(node)
  const langColor = languageColor(language)
  const color = language === 'external' ? typeColor : langColor
  const size = nodeRadius(node)
  const alpha = isFaded ? 0.18 : node.reachability === 'Detached' ? 0.55 : 1

  ctx.save()
  ctx.globalAlpha = alpha
  ctx.shadowBlur = isSelected ? 26 : isHovered ? 16 : 5
  ctx.shadowColor = color
  ctx.fillStyle = theme.card
  ctx.strokeStyle = isSelected ? theme.text : color
  ctx.lineWidth = isSelected ? 3 : isHovered ? 2.4 : 1.7
  if (node.reachability === 'Detached') ctx.setLineDash([5, 4])

  if (node.type === 'File') {
    const w = size * 1.55
    const h = size * 1.95
    ctx.beginPath()
    ctx.roundRect(node.x - w / 2, node.y - h / 2, w, h, 4)
    ctx.fill()
    ctx.stroke()
  } else if (node.type === 'Module' || node.type === 'Struct' || node.type === 'Class' || node.type === 'Object' || node.type === 'Interface' || node.type === 'TypeAlias') {
    ctx.beginPath()
    ctx.roundRect(node.x - size, node.y - size * 0.68, size * 2, size * 1.36, node.type === 'Module' ? 8 : 5)
    ctx.fill()
    ctx.stroke()
  } else if (node.type === 'Endpoint') {
    ctx.beginPath()
    ctx.roundRect(node.x - size * 1.35, node.y - size * 0.65, size * 2.7, size * 1.3, 9)
    ctx.fill()
    ctx.stroke()
  } else if (node.type === 'Enum') {
    ctx.beginPath()
    ctx.moveTo(node.x, node.y - size)
    ctx.lineTo(node.x + size * 0.76, node.y)
    ctx.lineTo(node.x, node.y + size)
    ctx.lineTo(node.x - size * 0.76, node.y)
    ctx.closePath()
    ctx.fill()
    ctx.stroke()
  } else {
    ctx.beginPath()
    ctx.arc(node.x, node.y, size, 0, Math.PI * 2)
    ctx.fill()
    ctx.stroke()
  }

  ctx.setLineDash([])
  ctx.shadowBlur = 0
  ctx.fillStyle = color
  ctx.globalAlpha = alpha * 0.82
  ctx.beginPath()
  ctx.arc(node.x, node.y, Math.max(3, size * 0.23), 0, Math.PI * 2)
  ctx.fill()

  const icon = languageIcon(language)
  const badgeW = Math.max(18, icon.length * 6.5 + 9)
  const bx = node.x + size * 0.35
  const by = node.y - size - 14
  ctx.globalAlpha = alpha
  ctx.fillStyle = theme.card
  ctx.strokeStyle = color
  ctx.lineWidth = 1.2
  ctx.beginPath()
  ctx.roundRect(bx, by, badgeW, 16, 8)
  ctx.fill()
  ctx.stroke()
  ctx.fillStyle = color
  ctx.font = '800 8px Inter, sans-serif'
  ctx.textAlign = 'center'
  ctx.textBaseline = 'middle'
  ctx.fillText(icon, bx + badgeW / 2, by + 8.2)
  ctx.restore()
}

function drawDiagnosticBadge(ctx: CanvasRenderingContext2D, node: GraphNode, severity: 'Error' | 'Warning') {
  const size = nodeRadius(node)
  ctx.save()
  ctx.fillStyle = severity === 'Error' ? '#F87171' : '#F59E0B'
  ctx.strokeStyle = '#ffffff'
  ctx.lineWidth = 1.5
  ctx.beginPath()
  ctx.arc(node.x + size * 0.75, node.y - size * 0.75, 4.4, 0, Math.PI * 2)
  ctx.fill()
  ctx.stroke()
  ctx.restore()
}

function fitLabel(text: string, maxChars: number) {
  return text.length <= maxChars ? text : `${text.slice(0, Math.max(1, maxChars - 1))}…`
}

function shortPath(path: string) {
  const parts = path.split('/').filter(Boolean)
  if (parts.length <= 2) return path
  return `${parts[parts.length - 2]}/${parts[parts.length - 1]}`
}

function labelPriority(node: GraphNode, degree: number, selected: boolean, hovered: boolean) {
  if (selected) return 10000
  if (hovered) return 9000
  const typeBoost: Partial<Record<NodeType, number>> = {
    Module: 120,
    File: 100,
    Endpoint: 90,
    Struct: 74,
    Class: 74,
    Object: 64,
    Enum: 70,
    Trait: 70,
    Component: 48,
  }
  return degree * 34 + (typeBoost[node.type] ?? 0) + (node.pinned ? 600 : 0)
}

function labelBudget(visibleCount: number, zoom: number) {
  if (visibleCount <= 45) return visibleCount
  const zoomScale = zoom >= 1 ? 1.35 : zoom >= 0.75 ? 1 : zoom >= 0.5 ? 0.72 : 0.45
  return Math.round(Math.min(visibleCount, Math.max(24, Math.sqrt(visibleCount) * 7.5 * zoomScale)))
}

function shouldDrawLabel(candidate: LabelCandidate, labelMode: GraphLabelMode, drawn: number, budget: number, visibleCount: number, zoom: number) {
  const force = candidate.isSelected || candidate.isHovered || candidate.node.pinned
  if (force) return true
  if (labelMode === 'all') return true
  const keyNode = candidate.degree >= 5 || candidate.node.type === 'Module' || candidate.node.type === 'File' || candidate.node.type === 'Endpoint' || candidate.node.type === 'Struct' || candidate.node.type === 'Trait'
  if (labelMode === 'key' && !keyNode) return false
  if (drawn >= budget) return false
  if (visibleCount > 70 && zoom < 0.62 && candidate.degree < 3) return false
  return keyNode || labelMode === 'auto'
}

function labelBox(ctx: CanvasRenderingContext2D, node: GraphNode, selected: boolean, hovered: boolean, lines: string[]) {
  const size = nodeRadius(node)
  const fontSize = selected || hovered ? 12 : 10
  ctx.font = `${selected ? 700 : 520} ${fontSize}px Inter, sans-serif`
  const width = Math.max(...lines.map(line => ctx.measureText(line).width), 1) + 12
  const lineH = selected || hovered ? 14 : 12
  const height = lines.length * lineH + 2
  const y = node.y + size + 5
  return { x1: node.x - width / 2, y1: y - 1, x2: node.x + width / 2, y2: y + height, lineH }
}

function overlap(a: { x1: number; y1: number; x2: number; y2: number }, b: { x1: number; y1: number; x2: number; y2: number }) {
  return a.x1 < b.x2 && a.x2 > b.x1 && a.y1 < b.y2 && a.y2 > b.y1
}

function drawLabel(ctx: CanvasRenderingContext2D, node: GraphNode, selected: boolean, hovered: boolean, theme: CanvasTheme, lines: string[], lineH: number) {
  const size = nodeRadius(node)
  ctx.save()
  ctx.font = `${selected ? 700 : 520} ${selected || hovered ? 12 : 10}px Inter, sans-serif`
  ctx.textAlign = 'center'
  ctx.textBaseline = 'top'
  ctx.shadowColor = theme.card
  ctx.shadowBlur = 5
  ctx.fillStyle = selected ? theme.text : hovered ? languageColor(inferNodeLanguage(node)) : theme.textMuted
  lines.forEach((line, index) => ctx.fillText(line, node.x, node.y + size + 5 + index * lineH))
  ctx.restore()
}

function drawLabels(ctx: CanvasRenderingContext2D, candidates: LabelCandidate[], visibleCount: number, zoom: number, labelMode: GraphLabelMode, theme: CanvasTheme) {
  const occupied: Array<{ x1: number; y1: number; x2: number; y2: number }> = []
  const sorted = [...candidates].sort((a, b) => b.priority - a.priority)
  const budget = labelMode === 'key' ? Math.round(Math.max(12, Math.sqrt(visibleCount) * 3.8)) : labelBudget(visibleCount, zoom)
  let drawn = 0
  for (const candidate of sorted) {
    if (!shouldDrawLabel(candidate, labelMode, drawn, budget, visibleCount, zoom)) continue
    const selected = candidate.isSelected
    const hovered = candidate.isHovered
    const maxChars = selected || hovered ? 30 : 18
    const lines = [fitLabel(candidate.node.label, maxChars)]
    if ((selected || hovered || candidate.node.type === 'Endpoint' || candidate.node.type === 'File') && candidate.node.file && candidate.node.file !== candidate.node.label) {
      lines.push(fitLabel(shortPath(candidate.node.file), maxChars))
    }
    if ((selected || hovered) && candidate.node.reachability === 'External') lines.push('External')
    const box = labelBox(ctx, candidate.node, selected, hovered, lines)
    const force = selected || hovered || candidate.node.pinned
    if (labelMode !== 'all' && !force && occupied.some(item => overlap(item, box))) continue
    drawLabel(ctx, candidate.node, selected, hovered, theme, lines, box.lineH)
    occupied.push(box)
    drawn++
  }
}

function drawMiniMap(ctx: CanvasRenderingContext2D, nodes: GraphNode[], pan: { x: number; y: number }, zoom: number, canvasW: number, canvasH: number, theme: CanvasTheme) {
  if (nodes.length < 8) return
  const bounds = getNodeBounds(nodes)
  if (!bounds) return
  const mmX = canvasW - MINIMAP_W - 16
  const mmY = canvasH - MINIMAP_H - 16
  const innerX = mmX + 8
  const innerY = mmY + 8
  const innerW = MINIMAP_W - 16
  const innerH = MINIMAP_H - 22
  const pad = 90
  const worldW = Math.max(bounds.width + pad * 2, canvasW / 0.9, 420)
  const worldH = Math.max(bounds.height + pad * 2, canvasH / 0.9, 280)
  const centerX = (bounds.minX + bounds.maxX) / 2
  const centerY = (bounds.minY + bounds.maxY) / 2
  const minX = centerX - worldW / 2
  const minY = centerY - worldH / 2
  const scale = Math.min(innerW / worldW, innerH / worldH)
  const mapW = worldW * scale
  const mapH = worldH * scale
  const mapX = innerX + (innerW - mapW) / 2
  const mapY = innerY + (innerH - mapH) / 2

  ctx.save()
  ctx.globalAlpha = 0.88
  ctx.fillStyle = theme.surface
  ctx.strokeStyle = theme.border
  ctx.lineWidth = 1
  ctx.beginPath()
  ctx.roundRect(mmX, mmY, MINIMAP_W, MINIMAP_H, 6)
  ctx.fill()
  ctx.stroke()

  for (const node of nodes) {
    const nx = mapX + (node.x - minX) * scale
    const ny = mapY + (node.y - minY) * scale
    ctx.fillStyle = languageColor(inferNodeLanguage(node))
    ctx.globalAlpha = 0.72
    ctx.beginPath()
    ctx.arc(nx, ny, 2.3, 0, Math.PI * 2)
    ctx.fill()
  }

  const viewMinX = -pan.x / zoom
  const viewMinY = -pan.y / zoom
  const viewMaxX = viewMinX + canvasW / zoom
  const viewMaxY = viewMinY + canvasH / zoom
  const vpX = mapX + (viewMinX - minX) * scale
  const vpY = mapY + (viewMinY - minY) * scale
  const vpW = Math.max(3, (viewMaxX - viewMinX) * scale)
  const vpH = Math.max(3, (viewMaxY - viewMinY) * scale)
  ctx.globalAlpha = 0.16
  ctx.fillStyle = '#06B6D4'
  ctx.fillRect(vpX, vpY, vpW, vpH)
  ctx.globalAlpha = 0.75
  ctx.strokeStyle = '#06B6D4'
  ctx.lineWidth = 1.5
  ctx.strokeRect(vpX, vpY, vpW, vpH)
  ctx.restore()
}

function drawLanguageLegend(ctx: CanvasRenderingContext2D, canvasH: number, paused: boolean, theme: CanvasTheme) {
  const x = 18
  const y = Math.max(92, canvasH - 178)
  const w = 304
  const h = paused ? 96 : 78
  const languages = ['rust', 'typescript', 'python', 'qml', 'endpoints', 'external'] as const
  ctx.save()
  ctx.globalAlpha = 0.9
  ctx.fillStyle = theme.surface
  ctx.strokeStyle = theme.border
  ctx.lineWidth = 1
  ctx.beginPath()
  ctx.roundRect(x, y, w, h, 9)
  ctx.fill()
  ctx.stroke()
  ctx.fillStyle = theme.text
  ctx.font = '700 10px Inter, sans-serif'
  ctx.textAlign = 'left'
  ctx.textBaseline = 'top'
  ctx.fillText('Language badges', x + 12, y + 10)
  ctx.font = '9px Inter, sans-serif'
  languages.forEach((language, index) => {
    const px = x + 12 + (index % 3) * 92
    const py = y + 29 + Math.floor(index / 3) * 18
    const color = languageColor(language)
    ctx.fillStyle = color
    ctx.globalAlpha = 0.86
    ctx.beginPath()
    ctx.roundRect(px, py + 2, 22, 11, 6)
    ctx.fill()
    ctx.fillStyle = '#fff'
    ctx.globalAlpha = 0.96
    ctx.font = '800 7.5px Inter, sans-serif'
    ctx.textAlign = 'center'
    ctx.fillText(languageIcon(language), px + 11, py + 3)
    ctx.globalAlpha = 0.82
    ctx.fillStyle = theme.textMuted
    ctx.font = '9px Inter, sans-serif'
    ctx.textAlign = 'left'
    ctx.fillText(language === 'endpoints' ? 'API' : language, px + 28, py)
  })
  if (paused) {
    ctx.globalAlpha = 0.9
    ctx.fillStyle = '#F59E0B'
    ctx.font = '700 10px Inter, sans-serif'
    ctx.fillText('Paused - press Space to resume', x + 12, y + 77)
  }
  ctx.restore()
}

function averageSpeed(nodes: GraphNode[]) {
  if (!nodes.length) return 0
  return nodes.reduce((sum, node) => sum + Math.sqrt((node.vx ?? 0) ** 2 + (node.vy ?? 0) ** 2), 0) / nodes.length
}

function keepContinuousMotion(nodes: GraphNode[], ts: number) {
  return nodes.map((node, index) => {
    if (node.pinned) return node
    const angle = ts * 0.0013 + index * GOLDEN_ANGLE
    return {
      ...node,
      vx: (node.vx ?? 0) + Math.cos(angle) * 0.045,
      vy: (node.vy ?? 0) + Math.sin(angle) * 0.045,
    }
  })
}

function isEditableTarget(target: EventTarget | null) {
  if (!(target instanceof HTMLElement)) return false
  const tag = target.tagName.toLowerCase()
  return target.isContentEditable || tag === 'input' || tag === 'textarea' || tag === 'select'
}

export function LiveCodeGraph({ nodes, edges, filters, selectedNodeId, recenterKey, theme, layoutSettings, graphMode, labelMode, diagnosticsByNode, highlightedTraceNodeIds, highlightedTraceEdgeIds, onSelectNode, onUpdateNodes }: LiveCodeGraphProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const nodesRef = useRef<GraphNode[]>([])
  const edgesRef = useRef<GraphEdge[]>(edges)
  const filtersRef = useRef(filters)
  const layoutSettingsRef = useRef(layoutSettings)
  const diagnosticsByNodeRef = useRef(diagnosticsByNode)
  const highlightedTraceNodeIdsRef = useRef(highlightedTraceNodeIds)
  const highlightedTraceEdgeIdsRef = useRef(highlightedTraceEdgeIds)
  const selectedNodeIdRef = useRef<string | null>(selectedNodeId)
  const hoveredNodeRef = useRef<string | null>(null)
  const panRef = useRef({ x: 0, y: 0 })
  const zoomRef = useRef(1)
  const dragRef = useRef<DragState>(null)
  const userNavigatedRef = useRef(false)
  const rafRef = useRef<number>(0)
  const renderRafRef = useRef<number>(0)
  const graphSignatureRef = useRef('')
  const visibleSignatureRef = useRef('')
  const settleStartedAtRef = useRef<number | null>(null)
  const settledRef = useRef(false)
  const pausedRef = useRef(false)
  const continuousSimulationRef = useRef(false)
  const onUpdateNodesRef = useRef(onUpdateNodes)
  const onSelectNodeRef = useRef(onSelectNode)
  const [hoveredNode, setHoveredNode] = useState<string | null>(null)
  const [paused, setPaused] = useState(false)

  edgesRef.current = edges
  filtersRef.current = filters
  layoutSettingsRef.current = layoutSettings
  diagnosticsByNodeRef.current = diagnosticsByNode
  highlightedTraceNodeIdsRef.current = highlightedTraceNodeIds
  highlightedTraceEdgeIdsRef.current = highlightedTraceEdgeIds
  selectedNodeIdRef.current = selectedNodeId
  pausedRef.current = paused
  onUpdateNodesRef.current = onUpdateNodes
  onSelectNodeRef.current = onSelectNode

  const fitCurrentGraph = useCallback((force = false) => {
    const canvas = canvasRef.current
    if (!canvas || nodesRef.current.length === 0) return
    if (!force && userNavigatedRef.current) return
    const rect = canvas.getBoundingClientRect()
    const { activeNodes } = activeGraph(nodesRef.current, edgesRef.current, filtersRef.current)
    fitGraphToView(activeNodes.length > 0 ? activeNodes : nodesRef.current, rect.width, rect.height, panRef.current, zoomRef)
  }, [])

  const draw = useCallback(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const ctx = canvas.getContext('2d')
    if (!ctx) return
    const rect = canvas.getBoundingClientRect()
    const dpr = window.devicePixelRatio || 1
    const W = rect.width
    const H = rect.height
    if (canvas.width !== Math.round(W * dpr) || canvas.height !== Math.round(H * dpr)) {
      canvas.width = Math.round(W * dpr)
      canvas.height = Math.round(H * dpr)
    }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
    const colors = canvasTheme()
    const filtersNow = filtersRef.current
    const { visibleIds, activeNodes, activeEdges } = activeGraph(nodesRef.current, edgesRef.current, filtersNow)
    const nodeMap = new Map(nodesRef.current.map(node => [node.id, node]))
    const degree = buildDegreeMap(activeNodes, activeEdges)
    const hovered = hoveredNodeRef.current
    const selected = selectedNodeIdRef.current
    const traceNodeIds = highlightedTraceNodeIdsRef.current
    const traceEdgeIds = highlightedTraceEdgeIdsRef.current
    const traceActive = !!traceNodeIds?.size || !!traceEdgeIds?.size
    const hoveredConnections = new Set<string>()
    const selectedConnections = new Set<string>()

    if (hovered) {
      for (const edge of activeEdges) {
        if (edge.source === hovered || edge.target === hovered) {
          hoveredConnections.add(edge.source)
          hoveredConnections.add(edge.target)
        }
      }
    }
    if (selected) {
      for (const edge of activeEdges) {
        if (edge.source === selected || edge.target === selected) {
          selectedConnections.add(edge.source)
          selectedConnections.add(edge.target)
        }
      }
    }

    ctx.clearRect(0, 0, W, H)
    const bg = ctx.createRadialGradient(W / 2, H / 2, 0, W / 2, H / 2, Math.max(W, H) * 0.62)
    bg.addColorStop(0, colors.bg2)
    bg.addColorStop(1, colors.bg)
    ctx.fillStyle = bg
    ctx.fillRect(0, 0, W, H)

    ctx.save()
    ctx.translate(panRef.current.x, panRef.current.y)
    ctx.scale(zoomRef.current, zoomRef.current)

    const gridSpacing = 40
    const startX = Math.floor(-panRef.current.x / zoomRef.current / gridSpacing - 1) * gridSpacing
    const startY = Math.floor(-panRef.current.y / zoomRef.current / gridSpacing - 1) * gridSpacing
    const endX = startX + W / zoomRef.current + gridSpacing * 2
    const endY = startY + H / zoomRef.current + gridSpacing * 2
    ctx.fillStyle = colors.gridDot
    for (let gx = startX; gx < endX; gx += gridSpacing) {
      for (let gy = startY; gy < endY; gy += gridSpacing) {
        ctx.beginPath()
        ctx.arc(gx, gy, 1, 0, Math.PI * 2)
        ctx.fill()
      }
    }

    for (const edge of activeEdges) {
      const src = nodeMap.get(edge.source)
      const tgt = nodeMap.get(edge.target)
      if (!src || !tgt || !visibleIds.has(src.id) || !visibleIds.has(tgt.id)) continue
      const edgeInTrace = traceEdgeIds?.has(edge.id) || edge.bundledEdgeIds?.some(id => traceEdgeIds?.has(id))
      const isActive = !!edgeInTrace || (hoveredConnections.has(edge.source) && hoveredConnections.has(edge.target)) || (selectedConnections.has(edge.source) && selectedConnections.has(edge.target))
      const base = edgeColor(edge)
      const baseAlpha = edgeInTrace ? 0.95 : isActive ? 0.84 : traceActive ? 0.18 : activeEdges.length > 1400 ? 0.34 : 0.52
      const alpha = graphMode === 'DataFlow' && edge.type !== 'DataFlow'
        ? Math.min(baseAlpha, edge.type === 'ApiCall' || edge.type === 'EndpointHandler' ? 0.38 : 0.22)
        : baseAlpha
      drawArrow(ctx, src, tgt, base, isActive ? edgeWidth(edge) + 1.2 : edgeWidth(edge), alpha, edge.type === 'DataFlow' || edge.type === 'ApiCall' || edge.type === 'EndpointHandler', performance.now())
      if ((edge.bundledCount ?? 1) > 1 && (isActive || zoomRef.current > 0.7)) {
        ctx.save()
        ctx.fillStyle = colors.card
        ctx.strokeStyle = colors.border
        ctx.lineWidth = 1
        const x = (src.x + tgt.x) / 2
        const y = (src.y + tgt.y) / 2
        ctx.beginPath()
        ctx.roundRect(x - 10, y - 8, 20, 16, 8)
        ctx.fill()
        ctx.stroke()
        ctx.fillStyle = colors.textMuted
        ctx.font = '10px Inter, sans-serif'
        ctx.textAlign = 'center'
        ctx.textBaseline = 'middle'
        ctx.fillText(String(edge.bundledCount), x, y)
        ctx.restore()
      }
    }

    const labelCandidates: LabelCandidate[] = []
    for (const node of activeNodes) {
      if (!visibleIds.has(node.id) || !filtersNow.nodeTypes.has(node.type)) continue
      const isSelected = node.id === selected || (traceNodeIds?.has(node.id) ?? false)
      const isHovered = node.id === hovered
      const isFaded = (traceActive && !traceNodeIds?.has(node.id)) || (hovered !== null && !hoveredConnections.has(node.id) && !isHovered)
      drawNode(ctx, node, isSelected, isHovered, isFaded, colors)
      const diagnostics = diagnosticsByNodeRef.current?.get(node.id) ?? []
      if (diagnostics.length > 0) drawDiagnosticBadge(ctx, node, diagnostics.some(item => item.severity === 'Error') ? 'Error' : 'Warning')
      if (node.pinned) {
        ctx.save()
        ctx.fillStyle = '#F59E0B'
        ctx.globalAlpha = 0.9
        ctx.font = '10px sans-serif'
        ctx.textAlign = 'center'
        ctx.fillText('P', node.x, node.y + 4)
        ctx.restore()
      }
      labelCandidates.push({
        node,
        degree: degree.get(node.id) ?? 0,
        isSelected,
        isHovered,
        priority: labelPriority(node, degree.get(node.id) ?? 0, isSelected, isHovered),
      })
    }
    drawLabels(ctx, labelCandidates, activeNodes.length, zoomRef.current, labelMode, colors)
    ctx.restore()

    drawMiniMap(ctx, activeNodes, panRef.current, zoomRef.current, W, H, colors)
    drawLanguageLegend(ctx, H, pausedRef.current, colors)
  }, [graphMode, labelMode])

  const scheduleDraw = useCallback(() => {
    cancelAnimationFrame(renderRafRef.current)
    renderRafRef.current = requestAnimationFrame(draw)
  }, [draw])

  const runLoop = useCallback((ts: number) => {
    if (pausedRef.current && !continuousSimulationRef.current) {
      draw()
      return
    }

    const filtersNow = filtersRef.current
    const settings = layoutSettingsRef.current

    if (continuousSimulationRef.current) {
      const result = runPhysicsTick(nodesRef.current, edgesRef.current, filtersNow, settings)
      nodesRef.current = result.averageSpeed < 0.08
        ? keepContinuousMotion(result.nodes, ts)
        : result.nodes
      draw()
      if (!pausedRef.current && continuousSimulationRef.current) {
        rafRef.current = requestAnimationFrame(runLoop)
      }
      return
    }

    if (settleStartedAtRef.current === null) settleStartedAtRef.current = ts
    const elapsed = ts - settleStartedAtRef.current

    if (elapsed <= VISIBLE_SETTLE_MS) {
      const result = runPhysicsTick(nodesRef.current, edgesRef.current, filtersNow, settings)
      nodesRef.current = result.nodes
      if (!userNavigatedRef.current && elapsed < FIT_SETTLE_MS) fitCurrentGraph(false)
    } else if (!settledRef.current) {
      const fadeProgress = Math.min(1, (elapsed - VISIBLE_SETTLE_MS) / SETTLE_FADE_MS)
      const result = runPhysicsTick(
        nodesRef.current,
        edgesRef.current,
        filtersNow,
        settings,
        {
          dampingScale: 2.4 + fadeProgress * 4.6,
          maxSpeedScale: 0.55,
          maxForceScale: 0.45,
        },
      )
      nodesRef.current = result.nodes.map(node => ({
        ...node,
        vx: node.vx * Math.max(0.68, 0.92 - fadeProgress * 0.18),
        vy: node.vy * Math.max(0.68, 0.92 - fadeProgress * 0.18),
      }))
      if (averageSpeed(nodesRef.current) < 0.012 || fadeProgress >= 1) {
        nodesRef.current = nodesRef.current.map(node => ({ ...node, vx: 0, vy: 0 }))
        settledRef.current = true
      }
    }

    draw()
    if (!settledRef.current && !pausedRef.current) rafRef.current = requestAnimationFrame(runLoop)
  }, [draw, fitCurrentGraph])

  const restartSimulation = useCallback(() => {
    cancelAnimationFrame(rafRef.current)
    continuousSimulationRef.current = false
    pausedRef.current = false
    setPaused(false)
    settleStartedAtRef.current = null
    settledRef.current = false
    rafRef.current = requestAnimationFrame(runLoop)
  }, [runLoop])

  useEffect(() => {
    const signature = `${nodes.map(node => node.id).join('|')}::${edges.map(edge => edge.id).join('|')}`
    if (signature !== graphSignatureRef.current) {
      graphSignatureRef.current = signature
      const previous = new Map(nodesRef.current.map(node => [node.id, node]))
      nodesRef.current = seedLayout(nodes, previous, layoutSettingsRef.current.spacing)
      if (!userNavigatedRef.current) requestAnimationFrame(() => fitCurrentGraph(true))
      restartSimulation()
    } else {
      const current = new Map(nodesRef.current.map(node => [node.id, node]))
      nodesRef.current = nodes.map(node => {
        const existing = current.get(node.id)
        return existing ? { ...node, x: existing.x, y: existing.y, vx: existing.vx, vy: existing.vy, pinned: existing.pinned || node.pinned } : { ...node, vx: 0, vy: 0 }
      })
      scheduleDraw()
    }
  }, [nodes, edges, fitCurrentGraph, restartSimulation, scheduleDraw])

  useEffect(() => {
    const visibleSignature = `${graphMode}::${selectedNodeId ?? ''}::${filters.depth}::${[...filters.nodeTypes].sort().join('|')}::${[...filters.edgeTypes].sort().join('|')}::${filters.edgeVisibility}`
    if (visibleSignature === visibleSignatureRef.current) return
    visibleSignatureRef.current = visibleSignature
    if (!userNavigatedRef.current) requestAnimationFrame(() => fitCurrentGraph(true))
    restartSimulation()
  }, [filters, graphMode, selectedNodeId, fitCurrentGraph, restartSimulation])

  useEffect(() => {
    restartSimulation()
  }, [layoutSettings, restartSimulation])

  useEffect(() => {
    userNavigatedRef.current = false
    fitCurrentGraph(true)
    scheduleDraw()
  }, [recenterKey, fitCurrentGraph, scheduleDraw])

  useEffect(() => {
    scheduleDraw()
  }, [theme, selectedNodeId, highlightedTraceNodeIds, highlightedTraceEdgeIds, diagnosticsByNode, labelMode, paused, scheduleDraw])

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const resize = () => {
      if (!userNavigatedRef.current) fitCurrentGraph(false)
      scheduleDraw()
    }
    const ro = new ResizeObserver(resize)
    ro.observe(canvas)
    resize()
    return () => ro.disconnect()
  }, [fitCurrentGraph, scheduleDraw])

  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if (event.code !== 'Space' || isEditableTarget(event.target)) return
      event.preventDefault()

      if (continuousSimulationRef.current) {
        cancelAnimationFrame(rafRef.current)
        continuousSimulationRef.current = false
        nodesRef.current = nodesRef.current.map(node => ({ ...node, vx: 0, vy: 0 }))
        settledRef.current = true
        settleStartedAtRef.current = null
        pausedRef.current = true
        setPaused(true)
        scheduleDraw()
        return
      }

      cancelAnimationFrame(rafRef.current)
      continuousSimulationRef.current = true
      settledRef.current = false
      settleStartedAtRef.current = null
      pausedRef.current = false
      setPaused(false)
      rafRef.current = requestAnimationFrame(runLoop)
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [runLoop, scheduleDraw])

  useEffect(() => () => {
    cancelAnimationFrame(rafRef.current)
    cancelAnimationFrame(renderRafRef.current)
  }, [])

  const toWorld = useCallback((clientX: number, clientY: number) => {
    const canvas = canvasRef.current!
    const rect = canvas.getBoundingClientRect()
    return {
      x: (clientX - rect.left - panRef.current.x) / zoomRef.current,
      y: (clientY - rect.top - panRef.current.y) / zoomRef.current,
    }
  }, [])

  const hitTest = useCallback((clientX: number, clientY: number) => {
    const { x, y } = toWorld(clientX, clientY)
    const { visibleIds, activeNodes } = activeGraph(nodesRef.current, edgesRef.current, filtersRef.current)
    for (let i = activeNodes.length - 1; i >= 0; i--) {
      const node = activeNodes[i]
      if (!visibleIds.has(node.id)) continue
      const size = nodeRadius(node)
      const dx = node.x - x
      const dy = node.y - y
      const hitRadius = size + 12
      if (dx * dx + dy * dy <= hitRadius * hitRadius) return node
      const icon = languageIcon(inferNodeLanguage(node))
      const badgeW = Math.max(18, icon.length * 6.5 + 9)
      const bx = node.x + size * 0.35
      const by = node.y - size - 14
      if (x >= bx - 4 && x <= bx + badgeW + 4 && y >= by - 4 && y <= by + 20) return node
    }
    return null
  }, [toWorld])

  const handleMouseDown = useCallback((event: React.MouseEvent<HTMLCanvasElement>) => {
    if (event.button !== 0) return
    const hit = hitTest(event.clientX, event.clientY)
    if (hit) {
      dragRef.current = { kind: 'node', id: hit.id, startX: event.clientX, startY: event.clientY, moved: false }
      onSelectNodeRef.current(hit.id)
      scheduleDraw()
      return
    }
    dragRef.current = { kind: 'pan', startX: event.clientX, startY: event.clientY, panX: panRef.current.x, panY: panRef.current.y, moved: false }
  }, [hitTest, scheduleDraw])

  const handleMouseMove = useCallback((event: React.MouseEvent<HTMLCanvasElement>) => {
    const drag = dragRef.current
    if (drag?.kind === 'node') {
      const dx = event.clientX - drag.startX
      const dy = event.clientY - drag.startY
      if (dx * dx + dy * dy > 9) drag.moved = true
      const point = toWorld(event.clientX, event.clientY)
      nodesRef.current = nodesRef.current.map(node => node.id === drag.id ? { ...node, x: point.x, y: point.y, vx: 0, vy: 0, pinned: true } : node)
      settledRef.current = true
      scheduleDraw()
      return
    }
    if (drag?.kind === 'pan') {
      const dx = event.clientX - drag.startX
      const dy = event.clientY - drag.startY
      if (Math.abs(dx) + Math.abs(dy) > 2) drag.moved = true
      userNavigatedRef.current = true
      panRef.current = { x: drag.panX + dx, y: drag.panY + dy }
      scheduleDraw()
      return
    }

    const hit = hitTest(event.clientX, event.clientY)
    const nextHovered = hit?.id ?? null
    if (nextHovered !== hoveredNodeRef.current) {
      hoveredNodeRef.current = nextHovered
      setHoveredNode(nextHovered)
      scheduleDraw()
    }
  }, [hitTest, scheduleDraw, toWorld])

  const finishDrag = useCallback((event?: React.MouseEvent<HTMLCanvasElement>) => {
    const drag = dragRef.current
    if (drag?.kind === 'node') {
      onUpdateNodesRef.current([...nodesRef.current])
    } else if (drag?.kind === 'pan' && !drag.moved && event) {
      const hit = hitTest(event.clientX, event.clientY)
      onSelectNodeRef.current(hit?.id ?? null)
    }
    dragRef.current = null
    scheduleDraw()
  }, [hitTest, scheduleDraw])

  const handleWheel = useCallback((event: React.WheelEvent<HTMLCanvasElement>) => {
    event.preventDefault()
    userNavigatedRef.current = true
    const canvas = canvasRef.current
    if (!canvas) return
    const rect = canvas.getBoundingClientRect()
    const mx = event.clientX - rect.left
    const my = event.clientY - rect.top
    const oldZoom = zoomRef.current
    const factor = event.deltaY > 0 ? 0.9 : 1.1
    const newZoom = Math.max(0.12, Math.min(4.5, oldZoom * factor))
    panRef.current.x = mx - (mx - panRef.current.x) * (newZoom / oldZoom)
    panRef.current.y = my - (my - panRef.current.y) * (newZoom / oldZoom)
    zoomRef.current = newZoom
    scheduleDraw()
  }, [scheduleDraw])

  return (
    <div className="relative w-full h-full overflow-hidden">
      <canvas
        ref={canvasRef}
        className="w-full h-full"
        style={{ cursor: hoveredNode ? 'pointer' : dragRef.current?.kind === 'pan' ? 'grabbing' : 'grab', background: 'var(--cc-bg)' }}
        onMouseDown={handleMouseDown}
        onMouseMove={handleMouseMove}
        onMouseUp={finishDrag}
        onMouseLeave={finishDrag}
        onWheel={handleWheel}
      />
    </div>
  )
}
