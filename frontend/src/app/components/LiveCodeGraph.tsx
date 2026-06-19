import { useRef, useEffect, useCallback, useState } from 'react'
import { DEFAULT_GRAPH_LAYOUT_SETTINGS } from '../types'
import type { DiagnosticRecord, GraphNode, GraphEdge, GraphFilters, NodeType, EdgeType, ThemeMode, GraphLayoutSettings, GraphLabelMode } from '../types'

interface LiveCodeGraphProps {
  nodes: GraphNode[]
  edges: GraphEdge[]
  filters: GraphFilters
  selectedNodeId: string | null
  recenterKey: number
  theme: ThemeMode
  layoutSettings: GraphLayoutSettings
  labelMode: GraphLabelMode
  diagnosticsByNode?: Map<string, DiagnosticRecord[]>
  onSelectNode: (id: string | null) => void
  onUpdateNodes: (nodes: GraphNode[]) => void
}

// ── Color palettes ─────────────────────────────────────────────────────────
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

const EDGE_COLORS: Record<EdgeType, string> = {
  Contains: '#374151',
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
  ExternalDependency: '#374151',
}

const NODE_SIZES: Record<NodeType, number> = {
  Module: 26,
  ExternalCrate: 24,
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

// ── Physics constants ───────────────────────────────────────────────────────
const BASE_REPULSION = 9000
const SPRING_STRENGTH = 0.028
const SPRING_DAMPING = 0.09
const BASE_SPRING_LENGTH = 132
const MEDIUM_DRAG_B = 0.14
const HUB_DRAG_B = 0.035
const HUB_MASS = 0.72
const CENTER_GRAVITY = 0.0035
const COLLISION_PADDING = 18
const COLLISION_STRENGTH = 0.18
const MAX_FORCE = 48
const MAX_NODE_FORCE = 24
const MAX_SPEED = 14
const GOLDEN_ANGLE = Math.PI * (3 - Math.sqrt(5))
const VISIBLE_SETTLE_MS = 1500
const SETTLE_FADE_MS = 900
const FIT_SETTLE_MS = 450
const SPATIAL_GRID_THRESHOLD = 220
const SPATIAL_CELL_SIZE = 260
const SPATIAL_SEARCH_RADIUS = 2

interface PhysicsOptions {
  dampingScale?: number
  dragScale?: number
  maxSpeedScale?: number
  maxForceScale?: number
  layoutSettings?: GraphLayoutSettings
}

interface CanvasTheme {
  bg: string
  bg2: string
  card: string
  surface: string
  border: string
  text: string
  textMuted: string
  gridDot: string
}

interface LabelCandidate {
  node: GraphNode
  isSelected: boolean
  isHovered: boolean
  degree: number
  priority: number
}

type LayoutWorkerResponse = {
  type: 'layout'
  signature: string
  nodes: GraphNode[]
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
  const fitZoom = Math.min(availableW / bounds.width, availableH / bounds.height)
  const densityZoom = nodes.length > 55 ? Math.min(1.75, 1 + Math.log(nodes.length / 55) * 0.28) : 1
  const nextZoom = Math.max(0.28, Math.min(1.6, fitZoom * densityZoom))
  const centerX = (bounds.minX + bounds.maxX) / 2
  const centerY = (bounds.minY + bounds.maxY) / 2
  zoomRef.current = nextZoom
  pan.x = canvasW / 2 - centerX * nextZoom
  pan.y = canvasH / 2 - centerY * nextZoom
}

function graphSizeScale(nodeCount: number) {
  return Math.min(3.8, Math.max(1, Math.pow(Math.max(1, nodeCount) / 38, 0.38)))
}

function visibleNodeIdsFor(
  nodes: GraphNode[],
  edges: GraphEdge[],
  depth: GraphFilters['depth'],
) {
  const visible = new Set<string>()
  const pickDepthCenter = () => {
    const nodeById = new Map(nodes.map(n => [n.id, n]))
    const mainNode = nodes.find(n => n.type === 'Function' && n.label === 'main')
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
    return [...degree.entries()].sort((a, b) => b[1] - a[1])[0]?.[0] ?? nodes[0]?.id ?? null
  }

  const centerId = depth === 'full' ? null : pickDepthCenter()
  if (!centerId) {
    nodes.forEach(n => visible.add(n.id))
    return visible
  }

  visible.add(centerId)
  const maxDepth = typeof depth === 'number' ? depth : 99
  const expand = (id: string, d: number) => {
    if (d <= 0) return
    edges.forEach(e => {
      if (e.source === id && !visible.has(e.target)) { visible.add(e.target); expand(e.target, d - 1) }
      if (e.target === id && !visible.has(e.source)) { visible.add(e.source); expand(e.source, d - 1) }
    })
  }
  expand(centerId, maxDepth)
  return visible
}

function visibleGraphSignature(
  nodes: GraphNode[],
  edges: GraphEdge[],
  filters: GraphFilters,
) {
  const visibleIds = visibleNodeIdsFor(nodes, edges, filters.depth)
  return [
    filters.depth,
    [...visibleIds].sort().join('|'),
    [...filters.nodeTypes].sort().join('|'),
    [...filters.edgeTypes].sort().join('|'),
    filters.edgeVisibility,
  ].join('::')
}

function layoutSettingsSignature(layoutSettings: GraphLayoutSettings) {
  return [
    layoutSettings.spacing.toFixed(2),
    layoutSettings.repulsion.toFixed(2),
    layoutSettings.linkLength.toFixed(2),
    layoutSettings.damping.toFixed(2),
  ].join(':')
}

function runVisiblePhysicsTick(
  nodes: GraphNode[],
  edges: GraphEdge[],
  visibleIds: Set<string>,
  filters: GraphFilters,
  width: number,
  height: number,
  options: PhysicsOptions = {},
) {
  const activeIds = new Set(nodes.filter(node => visibleIds.has(node.id) && filters.nodeTypes.has(node.type)).map(node => node.id))
  if (activeIds.size === 0) {
    return nodes
  }

  const activeEdges = edges.filter(edge =>
    filters.edgeTypes.has(edge.type)
    && activeIds.has(edge.source)
    && activeIds.has(edge.target)
  )
  if (activeIds.size === nodes.length && activeEdges.length === edges.length) {
    return runPhysicsTick(nodes, edges, width, height, options)
  }

  const activeNodes = nodes.filter(node => activeIds.has(node.id))
  const updatedActive = runPhysicsTick(activeNodes, activeEdges, width, height, options)
  const updatedById = new Map(updatedActive.map(node => [node.id, node]))
  return nodes.map(node => updatedById.get(node.id) ?? node)
}

function runPhysicsTick(nodes: GraphNode[], edges: GraphEdge[], width: number, height: number, options: PhysicsOptions = {}): GraphNode[] {
  const updated = nodes.map(n => ({ ...n }))
  const index = new Map(updated.map(n => [n.id, n]))
  const forces = new Map(updated.map(n => [n.id, { x: 0, y: 0 }]))
  const degree = buildDegreeMap(updated, edges)
  const layout = options.layoutSettings ?? DEFAULT_GRAPH_LAYOUT_SETTINGS
  const graphScale = graphSizeScale(updated.length)
  const densityScale = Math.min(4.2, Math.max(1, Math.sqrt(updated.length / 72)))
  const spacingScale = Math.max(0.55, layout.spacing) * graphScale
  const repulsion = BASE_REPULSION * densityScale * densityScale * graphScale * Math.max(0.25, layout.repulsion) * spacingScale * spacingScale
  const springLength = BASE_SPRING_LENGTH * Math.min(2.9, densityScale) * Math.max(0.45, layout.linkLength) * spacingScale
  const centerGravity = CENTER_GRAVITY / densityScale
  const maxSpeed = MAX_SPEED * Math.min(1.8, Math.sqrt(densityScale)) * (options.maxSpeedScale ?? 1)
  const springDamping = SPRING_DAMPING * Math.max(0.35, layout.damping) * (options.dampingScale ?? 1)
  const mediumDrag = MEDIUM_DRAG_B * Math.max(0.35, layout.damping) * (options.dragScale ?? 1)
  const maxForceScale = options.maxForceScale ?? 1

  forRepulsionPairs(updated, (i, j, a, b) => {
    const dx = b.x - a.x
    const dy = b.y - a.y
    const distSq = dx * dx + dy * dy
    const jitterAngle = ((i * 92821 + j * 68917) % 360) * Math.PI / 180
    const dist = Math.sqrt(distSq) || 0.1
    const nx = distSq < 0.01 ? Math.cos(jitterAngle) : dx / dist
    const ny = distSq < 0.01 ? Math.sin(jitterAngle) : dy / dist
    const minDist = (NODE_SIZES[a.type] ?? 16) + (NODE_SIZES[b.type] ?? 16) + COLLISION_PADDING * spacingScale
    const collision = Math.max(0, minDist - dist) * COLLISION_STRENGTH
    const massScale = Math.sqrt(nodeMass(a, degree) * nodeMass(b, degree))
    const force = clampForce(((repulsion / Math.max(180, distSq) + collision) / massScale) * maxForceScale)
    addForce(forces, a.id, -nx * force, -ny * force)
    addForce(forces, b.id, nx * force, ny * force)
  })

  // spring-damper attraction along edges
  for (const edge of edges) {
    const a = index.get(edge.source)
    const b = index.get(edge.target)
    if (!a || !b) continue
    const dx = b.x - a.x
    const dy = b.y - a.y
    const dist = Math.sqrt(dx * dx + dy * dy) || 1
    const nx = dx / dist
    const ny = dy / dist
    const desiredLength = springLengthFor(edge.type, springLength)
    const stretch = dist - desiredLength
    const relativeVelocity = (b.vx - a.vx) * nx + (b.vy - a.vy) * ny
    const springScale = springDegreeScale(edge.type, a, b, degree)
    const force = clampForce((SPRING_STRENGTH * stretch + springDamping * relativeVelocity) * springScale * maxForceScale)
    addForce(forces, a.id, nx * force, ny * force)
    addForce(forces, b.id, -nx * force, -ny * force)
  }

  // center gravity + viscous drag + semi-implicit Euler integration
  for (const n of updated) {
    if (n.pinned) { n.vx = 0; n.vy = 0; continue }
    const force = forces.get(n.id) ?? { x: 0, y: 0 }
    force.x += -n.x * centerGravity
    force.y += -n.y * centerGravity
    const nodeDegree = degree.get(n.id) ?? 0
    const drag = mediumDrag + Math.sqrt(nodeDegree) * HUB_DRAG_B * Math.max(0.35, layout.damping) * (options.dragScale ?? 1)
    force.x += -drag * n.vx
    force.y += -drag * n.vy
    const maxNodeForce = MAX_NODE_FORCE * maxForceScale * Math.sqrt(nodeMass(n, degree))
    clampVector(force, maxNodeForce)
    const mass = nodeMass(n, degree)
    n.vx += force.x / mass
    n.vy += force.y / mass
    const speed = Math.sqrt(n.vx * n.vx + n.vy * n.vy)
    const nodeMaxSpeed = maxSpeed / Math.sqrt(mass)
    if (speed > nodeMaxSpeed) { n.vx = (n.vx / speed) * nodeMaxSpeed; n.vy = (n.vy / speed) * nodeMaxSpeed }
    n.x += n.vx
    n.y += n.vy
  }
  return updated
}

function buildDegreeMap(nodes: GraphNode[], edges: GraphEdge[]) {
  const degree = new Map(nodes.map(node => [node.id, 0]))
  for (const edge of edges) {
    degree.set(edge.source, (degree.get(edge.source) ?? 0) + 1)
    degree.set(edge.target, (degree.get(edge.target) ?? 0) + 1)
  }
  return degree
}

function forRepulsionPairs(
  nodes: GraphNode[],
  visit: (i: number, j: number, a: GraphNode, b: GraphNode) => void,
) {
  if (nodes.length < SPATIAL_GRID_THRESHOLD) {
    for (let i = 0; i < nodes.length; i++) {
      for (let j = i + 1; j < nodes.length; j++) {
        visit(i, j, nodes[i], nodes[j])
      }
    }
    return
  }

  const grid = new Map<string, number[]>()
  for (let i = 0; i < nodes.length; i++) {
    const node = nodes[i]
    const cellX = Math.floor(node.x / SPATIAL_CELL_SIZE)
    const cellY = Math.floor(node.y / SPATIAL_CELL_SIZE)
    const key = `${cellX}:${cellY}`
    const bucket = grid.get(key) ?? []
    bucket.push(i)
    grid.set(key, bucket)
  }

  for (let i = 0; i < nodes.length; i++) {
    const node = nodes[i]
    const cellX = Math.floor(node.x / SPATIAL_CELL_SIZE)
    const cellY = Math.floor(node.y / SPATIAL_CELL_SIZE)
    for (let dx = -SPATIAL_SEARCH_RADIUS; dx <= SPATIAL_SEARCH_RADIUS; dx++) {
      for (let dy = -SPATIAL_SEARCH_RADIUS; dy <= SPATIAL_SEARCH_RADIUS; dy++) {
        const bucket = grid.get(`${cellX + dx}:${cellY + dy}`)
        if (!bucket) continue
        for (const j of bucket) {
          if (j <= i) continue
          visit(i, j, node, nodes[j])
        }
      }
    }
  }
}

function nodeMass(node: GraphNode, degree: Map<string, number>) {
  const links = degree.get(node.id) ?? 0
  const typeMass = node.type === 'Module' ? 1.5 : node.type === 'File' ? 1.25 : 1
  return typeMass + Math.sqrt(links) * HUB_MASS
}

function springDegreeScale(edgeType: EdgeType, a: GraphNode, b: GraphNode, degree: Map<string, number>) {
  const aDegree = Math.max(1, degree.get(a.id) ?? 1)
  const bDegree = Math.max(1, degree.get(b.id) ?? 1)
  const hubScale = 1 / Math.sqrt(Math.max(aDegree, bDegree))
  const typeScale = edgeType === 'Contains' ? 0.82 : edgeType === 'ApiCall' || edgeType === 'EndpointHandler' ? 0.72 : 1
  return Math.max(0.08, hubScale * typeScale)
}

function addForce(forces: Map<string, { x: number; y: number }>, id: string, x: number, y: number) {
  const force = forces.get(id)
  if (!force) return
  force.x += x
  force.y += y
}

function clampForce(force: number) {
  return Math.max(-MAX_FORCE, Math.min(MAX_FORCE, force))
}

function clampVector(vector: { x: number; y: number }, maxLength: number) {
  const length = Math.sqrt(vector.x * vector.x + vector.y * vector.y)
  if (length <= maxLength || length === 0) return
  vector.x = (vector.x / length) * maxLength
  vector.y = (vector.y / length) * maxLength
}

function springLengthFor(edgeType: EdgeType, base: number) {
  switch (edgeType) {
    case 'Contains':
      return base * 0.86
    case 'ApiCall':
    case 'EndpointHandler':
    case 'ExternalDependency':
      return base * 1.45
    case 'Renders':
      return base * 1.18
    case 'Calls':
      return base * 1.08
    default:
      return base
  }
}

function prepareInitialLayout(
  nodes: GraphNode[],
  edges: GraphEdge[],
  previous: Map<string, GraphNode>,
  layoutSettings: GraphLayoutSettings = DEFAULT_GRAPH_LAYOUT_SETTINGS,
) {
  const seeded = seedLayout(nodes, edges, previous)
  const relaxed = solveEquilibriumLayout(seeded, edges, layoutSettings)
  return relaxed.map((node, index) => {
    const drift = unitVectorFromId(node.id)
    const shouldDrift = hash01(`${node.id}:drift`) > 0.72
    const driftSize = shouldDrift ? 8 + (index % 5) * 1.5 : 2
    return {
      ...node,
      x: node.x + drift.x * driftSize,
      y: node.y + drift.y * driftSize,
      vx: shouldDrift ? drift.x * 0.18 : drift.x * 0.035,
      vy: shouldDrift ? drift.y * 0.18 : drift.y * 0.035,
    }
  })
}

function solveEquilibriumLayout(
  nodes: GraphNode[],
  edges: GraphEdge[],
  layoutSettings: GraphLayoutSettings = DEFAULT_GRAPH_LAYOUT_SETTINGS,
) {
  const iterations = nodes.length > 500 ? 128 : nodes.length > 220 ? 96 : 64
  let current = nodes.map(node => ({ ...node, vx: 0, vy: 0 }))
  for (let i = 0; i < iterations; i++) {
    const progress = i / Math.max(1, iterations - 1)
    const temperature = 1 - progress
    const step = 0.72 * temperature + 0.08
    current = runEquilibriumStep(current, edges, step, layoutSettings)
  }
  return current.map(node => ({ ...node, vx: 0, vy: 0 }))
}

function runEquilibriumStep(
  nodes: GraphNode[],
  edges: GraphEdge[],
  step: number,
  layoutSettings: GraphLayoutSettings = DEFAULT_GRAPH_LAYOUT_SETTINGS,
) {
  const updated = nodes.map(node => ({ ...node }))
  const index = new Map(updated.map(node => [node.id, node]))
  const forces = new Map(updated.map(node => [node.id, { x: 0, y: 0 }]))
  const degree = buildDegreeMap(updated, edges)
  const graphScale = graphSizeScale(updated.length)
  const densityScale = Math.min(4.2, Math.max(1, Math.sqrt(updated.length / 72)))
  const spacingScale = Math.max(0.55, layoutSettings.spacing) * graphScale
  const repulsion = BASE_REPULSION * densityScale * densityScale * graphScale * Math.max(0.25, layoutSettings.repulsion) * spacingScale * spacingScale
  const springLength = BASE_SPRING_LENGTH * Math.min(2.9, densityScale) * Math.max(0.45, layoutSettings.linkLength) * spacingScale
  const centerGravity = CENTER_GRAVITY / densityScale

  forRepulsionPairs(updated, (i, j, a, b) => {
    const dx = b.x - a.x
    const dy = b.y - a.y
    const distSq = dx * dx + dy * dy
    const jitterAngle = ((i * 92821 + j * 68917) % 360) * Math.PI / 180
    const dist = Math.sqrt(distSq) || 0.1
    const nx = distSq < 0.01 ? Math.cos(jitterAngle) : dx / dist
    const ny = distSq < 0.01 ? Math.sin(jitterAngle) : dy / dist
    const minDist = (NODE_SIZES[a.type] ?? 16) + (NODE_SIZES[b.type] ?? 16) + COLLISION_PADDING * spacingScale
    const collision = Math.max(0, minDist - dist) * COLLISION_STRENGTH
    const massScale = Math.sqrt(nodeMass(a, degree) * nodeMass(b, degree))
    const force = clampForce((repulsion / Math.max(180, distSq) + collision) / massScale)
    addForce(forces, a.id, -nx * force, -ny * force)
    addForce(forces, b.id, nx * force, ny * force)
  })

  for (const edge of edges) {
    const a = index.get(edge.source)
    const b = index.get(edge.target)
    if (!a || !b) continue
    const dx = b.x - a.x
    const dy = b.y - a.y
    const dist = Math.sqrt(dx * dx + dy * dy) || 1
    const nx = dx / dist
    const ny = dy / dist
    const stretch = dist - springLengthFor(edge.type, springLength)
    const force = clampForce(SPRING_STRENGTH * stretch * springDegreeScale(edge.type, a, b, degree))
    addForce(forces, a.id, nx * force, ny * force)
    addForce(forces, b.id, -nx * force, -ny * force)
  }

  for (const node of updated) {
    const force = forces.get(node.id) ?? { x: 0, y: 0 }
    force.x += -node.x * centerGravity
    force.y += -node.y * centerGravity
    const mass = nodeMass(node, degree)
    clampVector(force, MAX_NODE_FORCE * Math.sqrt(mass))
    node.x += (force.x / mass) * step
    node.y += (force.y / mass) * step
    node.vx = 0
    node.vy = 0
  }

  return updated
}

function averageSpeed(nodes: GraphNode[]) {
  if (nodes.length === 0) return 0
  const total = nodes.reduce((sum, node) => sum + Math.sqrt(node.vx * node.vx + node.vy * node.vy), 0)
  return total / nodes.length
}

function seedLayout(nodes: GraphNode[], edges: GraphEdge[], previous: Map<string, GraphNode>) {
  const neighbors = buildNeighborIds(edges)
  const groups = groupNodes(nodes)
  const groupCenters = groupCenterMap(groups)
  const prepared = new Map<string, GraphNode>()
  const result = nodes.map(node => {
    const prev = previous.get(node.id)
    if (prev) {
      const next = { ...node, x: prev.x, y: prev.y, vx: 0, vy: 0 }
      prepared.set(node.id, next)
      return next
    }
    const placed = placeNewNode(node, prepared, previous, neighbors, groups, groupCenters)
    prepared.set(node.id, placed)
    return placed
  })
  return result
}

function buildNeighborIds(edges: GraphEdge[]) {
  const neighbors = new Map<string, string[]>()
  for (const edge of edges) {
    const source = neighbors.get(edge.source) ?? []
    source.push(edge.target)
    neighbors.set(edge.source, source)
    const target = neighbors.get(edge.target) ?? []
    target.push(edge.source)
    neighbors.set(edge.target, target)
  }
  return neighbors
}

function groupNodes(nodes: GraphNode[]) {
  const groups = new Map<string, GraphNode[]>()
  for (const node of nodes) {
    const key = node.crate ?? node.module ?? 'workspace'
    const group = groups.get(key) ?? []
    group.push(node)
    groups.set(key, group)
  }
  return [...groups.entries()].sort(([a], [b]) => a.localeCompare(b))
}

function groupCenterMap(groups: Array<[string, GraphNode[]]>) {
  const centers = new Map<string, { x: number; y: number }>()
  if (groups.length <= 1) {
    if (groups[0]) centers.set(groups[0][0], { x: 0, y: 0 })
    return centers
  }
  const radius = Math.max(360, groups.length * 115)
  groups.forEach(([key], index) => {
    const angle = (Math.PI * 2 * index) / groups.length
    centers.set(key, { x: Math.cos(angle) * radius, y: Math.sin(angle) * radius })
  })
  return centers
}

function placeNewNode(
  node: GraphNode,
  prepared: Map<string, GraphNode>,
  previous: Map<string, GraphNode>,
  neighbors: Map<string, string[]>,
  groups: Array<[string, GraphNode[]]>,
  groupCenters: Map<string, { x: number; y: number }>,
) {
  const neighborPositions = (neighbors.get(node.id) ?? [])
    .map(id => prepared.get(id) ?? previous.get(id))
    .filter((candidate): candidate is GraphNode => !!candidate)

  if (neighborPositions.length > 0) {
    const center = neighborPositions.reduce((acc, n) => ({ x: acc.x + n.x, y: acc.y + n.y }), { x: 0, y: 0 })
    center.x /= neighborPositions.length
    center.y /= neighborPositions.length
    const jitter = unitVectorFromId(node.id)
    const radius = 72 + Math.min(120, neighborPositions.length * 8)
    return { ...node, x: center.x + jitter.x * radius, y: center.y + jitter.y * radius, vx: 0, vy: 0 }
  }

  const groupKey = node.crate ?? node.module ?? 'workspace'
  const group = groups.find(([key]) => key === groupKey)?.[1] ?? []
  const localIndex = Math.max(0, group.findIndex(candidate => candidate.id === node.id))
  const center = groupCenters.get(groupKey) ?? { x: 0, y: 0 }
  const isHub = node.type === 'Module' || node.type === 'File'
  const ringRadius = isHub ? 55 + Math.sqrt(localIndex + 1) * 18 : 120 + Math.sqrt(localIndex + 1) * 58
  const angle = localIndex * GOLDEN_ANGLE + hash01(node.id) * Math.PI * 2
  return {
    ...node,
    x: center.x + Math.cos(angle) * ringRadius,
    y: center.y + Math.sin(angle) * ringRadius,
    vx: 0,
    vy: 0,
  }
}

function unitVectorFromId(id: string) {
  const angle = hash01(id) * Math.PI * 2
  return { x: Math.cos(angle), y: Math.sin(angle) }
}

function hash01(text: string) {
  let hash = 2166136261
  for (let i = 0; i < text.length; i++) {
    hash ^= text.charCodeAt(i)
    hash = Math.imul(hash, 16777619)
  }
  return (hash >>> 0) / 4294967295
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

  if (n.type === 'ExternalCrate' || n.type === 'Interface') {
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
    case 'Component': {
      ctx.beginPath()
      ctx.roundRect(n.x - size, n.y - size * 0.7, size * 2, size * 1.4, 5)
      ctx.fill()
      ctx.stroke()
      ctx.strokeStyle = color
      ctx.globalAlpha = alpha * 0.45
      ctx.strokeRect(n.x - size * 0.45, n.y - size * 0.3, size * 0.9, size * 0.6)
      break
    }
    case 'Endpoint': {
      ctx.beginPath()
      ctx.roundRect(n.x - size * 1.35, n.y - size * 0.65, size * 2.7, size * 1.3, 9)
      ctx.fill()
      ctx.stroke()
      ctx.fillStyle = color
      ctx.globalAlpha = alpha * 0.85
      ctx.beginPath()
      ctx.arc(n.x - size * 0.85, n.y, 3, 0, Math.PI * 2)
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
      // Function, Hook, Interface, TypeAlias, Struct, Class, Object, ExternalCrate
      ctx.beginPath()
      if (n.type === 'Struct' || n.type === 'Class' || n.type === 'Object' || n.type === 'Interface' || n.type === 'TypeAlias' || n.type === 'Property') {
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

function drawDiagnosticBadge(ctx: CanvasRenderingContext2D, n: GraphNode, severity: 'Error' | 'Warning') {
  const size = NODE_SIZES[n.type]
  const color = severity === 'Error' ? '#F87171' : '#F59E0B'
  ctx.save()
  ctx.strokeStyle = color
  ctx.fillStyle = color
  ctx.globalAlpha = 0.95
  ctx.lineWidth = 2
  ctx.beginPath()
  ctx.arc(n.x, n.y, size + 4, 0, Math.PI * 2)
  ctx.stroke()
  ctx.beginPath()
  ctx.arc(n.x + size * 0.75, n.y - size * 0.75, 3.5, 0, Math.PI * 2)
  ctx.fill()
  ctx.restore()
}

function labelFont(isSelected: boolean, isHovered: boolean) {
  return `${isSelected ? '700' : '500'} ${isSelected || isHovered ? 12 : 10}px Inter, sans-serif`
}

function labelLineHeight(isSelected: boolean, isHovered: boolean) {
  return isSelected || isHovered ? 14 : 12
}

function splitLabelParts(label: string) {
  if (!label.includes('_')) return [label]
  const parts = label.split('_')
  return parts.map((part, index) => index < parts.length - 1 ? `${part}_` : part)
}

function fitLabelLine(ctx: CanvasRenderingContext2D, text: string, maxWidth: number) {
  if (ctx.measureText(text).width <= maxWidth) return text
  let fitted = text
  while (fitted.length > 1 && ctx.measureText(`${fitted}…`).width > maxWidth) {
    fitted = fitted.slice(0, -1)
  }
  return `${fitted}…`
}

function wrapLabel(ctx: CanvasRenderingContext2D, label: string, isImportant: boolean) {
  const maxWidth = isImportant ? 160 : 118
  const parts = splitLabelParts(label)
  const lines: string[] = []
  let current = ''

  for (const part of parts) {
    const next = current ? `${current}${part}` : part
    if (ctx.measureText(next).width <= maxWidth || !current) {
      current = next
      continue
    }
    lines.push(current)
    current = part
    if (lines.length === 2) break
  }
  if (current && lines.length < 2) lines.push(current)

  const compacted = lines.length > 0 ? lines : [label]
  if (parts.join('') !== compacted.join('') && compacted.length > 0) {
    compacted[compacted.length - 1] = `${compacted[compacted.length - 1].replace(/…$/, '')}…`
  }
  return compacted.slice(0, 2).map(line => fitLabelLine(ctx, line, maxWidth))
}

function labelBounds(ctx: CanvasRenderingContext2D, n: GraphNode, isSelected: boolean, isHovered: boolean) {
  const size = NODE_SIZES[n.type]
  ctx.font = labelFont(isSelected, isHovered)
  const lines = wrapLabel(ctx, n.label, isSelected || isHovered)
  const width = Math.max(...lines.map(line => ctx.measureText(line).width)) + 10
  const height = lines.length * labelLineHeight(isSelected, isHovered) + 2
  const offsetY = n.type === 'Module' ? size * 0.7 + 5 : size + 5
  return {
    lines,
    x1: n.x - width / 2,
    y1: n.y + offsetY - 1,
    x2: n.x + width / 2,
    y2: n.y + offsetY + height,
  }
}

function boxesOverlap(a: { x1: number; y1: number; x2: number; y2: number }, b: { x1: number; y1: number; x2: number; y2: number }) {
  return a.x1 < b.x2 && a.x2 > b.x1 && a.y1 < b.y2 && a.y2 > b.y1
}

function labelPriority(node: GraphNode, degree: number, isSelected: boolean, isHovered: boolean) {
  if (isSelected) return 10000
  if (isHovered) return 9000
  const typeBoost: Partial<Record<NodeType, number>> = {
    Module: 110,
    File: 90,
    Endpoint: 86,
    Struct: 72,
    Class: 72,
    Object: 64,
    Enum: 70,
    Trait: 70,
    Impl: 52,
    Component: 48,
  }
  return degree * 34 + (typeBoost[node.type] ?? 0) + (node.pinned ? 600 : 0)
}

function labelBudget(visibleCount: number, zoom: number) {
  if (visibleCount <= 45) return visibleCount
  const zoomScale = zoom >= 1 ? 1.35 : zoom >= 0.75 ? 1 : zoom >= 0.5 ? 0.72 : 0.45
  return Math.round(Math.min(visibleCount, Math.max(24, Math.sqrt(visibleCount) * 7.5 * zoomScale)))
}

function isKeyLabel(node: GraphNode, degree: number) {
  return node.pinned
    || degree >= 5
    || node.type === 'Module'
    || node.type === 'File'
    || node.type === 'Endpoint'
    || node.type === 'Struct'
    || node.type === 'Class'
    || node.type === 'Object'
    || node.type === 'Trait'
}

function drawLabels(
  ctx: CanvasRenderingContext2D,
  candidates: LabelCandidate[],
  visibleCount: number,
  zoom: number,
  labelMode: GraphLabelMode,
  theme: CanvasTheme,
) {
  const occupied: Array<{ x1: number; y1: number; x2: number; y2: number }> = []
  const sorted = [...candidates].sort((a, b) => b.priority - a.priority)
  const budget = labelMode === 'key'
    ? Math.round(Math.max(12, Math.sqrt(visibleCount) * 3.8))
    : labelBudget(visibleCount, zoom)
  let drawn = 0

  for (const candidate of sorted) {
    const force = candidate.isSelected || candidate.isHovered || candidate.node.pinned
    if (labelMode === 'key' && !force && !isKeyLabel(candidate.node, candidate.degree)) continue
    if (labelMode !== 'all' && !force && drawn >= budget) continue
    if (labelMode !== 'all' && !force && visibleCount > 70 && zoom < 0.62 && candidate.degree < 3) continue

    const box = labelBounds(ctx, candidate.node, candidate.isSelected, candidate.isHovered)
    if (labelMode !== 'all' && !force && occupied.some(existing => boxesOverlap(existing, box))) continue

    drawLabel(ctx, candidate.node, candidate.isSelected, candidate.isHovered, theme, box.lines)
    occupied.push(box)
    drawn++
  }
}

function drawLabel(ctx: CanvasRenderingContext2D, n: GraphNode, isSelected: boolean, isHovered: boolean, theme: CanvasTheme, lines: string[]) {
  const size = NODE_SIZES[n.type]
  const color = NODE_COLORS[n.type]
  ctx.save()
  ctx.font = labelFont(isSelected, isHovered)
  ctx.textAlign = 'center'
  ctx.textBaseline = 'top'
  const offsetY = n.type === 'Module' ? size * 0.7 + 5 : size + 5
  // shadow for readability
  ctx.shadowColor = theme.card
  ctx.shadowBlur = 5
  ctx.fillStyle = isSelected ? theme.text : isHovered ? color : theme.textMuted
  lines.forEach((line, index) => {
    ctx.fillText(line, n.x, n.y + offsetY + index * labelLineHeight(isSelected, isHovered))
  })
  ctx.restore()
}

// ── MiniMap ────────────────────────────────────────────────────────────────
function drawMiniMap(ctx: CanvasRenderingContext2D, nodes: GraphNode[], pan: { x: number; y: number }, zoom: number, canvasW: number, canvasH: number, theme: CanvasTheme) {
  if (nodes.length < 25) return
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

function drawEdgeBundleBadge(ctx: CanvasRenderingContext2D, src: GraphNode, tgt: GraphNode, count: number, theme: CanvasTheme) {
  const x = (src.x + tgt.x) / 2
  const y = (src.y + tgt.y) / 2
  ctx.save()
  ctx.fillStyle = theme.card
  ctx.strokeStyle = theme.border
  ctx.lineWidth = 1
  ctx.beginPath()
  ctx.roundRect(x - 10, y - 8, 20, 16, 8)
  ctx.fill()
  ctx.stroke()
  ctx.fillStyle = theme.textMuted
  ctx.font = '10px Inter, sans-serif'
  ctx.textAlign = 'center'
  ctx.textBaseline = 'middle'
  ctx.fillText(String(count), x, y)
  ctx.restore()
}

// ── Component ──────────────────────────────────────────────────────────────
export function LiveCodeGraph({ nodes, edges, filters, selectedNodeId, recenterKey, theme, layoutSettings, labelMode, diagnosticsByNode, onSelectNode, onUpdateNodes }: LiveCodeGraphProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const nodesRef = useRef<GraphNode[]>(nodes)
  const animFrameRef = useRef<number>(0)
  const animTimeRef = useRef<number>(0)
  const panRef = useRef({ x: 0, y: 0 })
  const zoomRef = useRef(1)
  const isDraggingRef = useRef(false)
  const dragStartRef = useRef({ x: 0, y: 0, panX: 0, panY: 0 })
  const dragNodeRef = useRef<string | null>(null)
  const suppressNextClickRef = useRef(false)
  const hoveredNodeRef = useRef<string | null>(null)
  const selectedNodeIdRef = useRef<string | null>(selectedNodeId)
  const userNavigatedRef = useRef(false)
  const graphSignatureRef = useRef('')
  const visibleSignatureRef = useRef('')
  const layoutWorkerSignatureRef = useRef('')
  const layoutWorkerRef = useRef<Worker | null>(null)
  const layoutSettingsRef = useRef(layoutSettings)
  const onUpdateNodesRef = useRef(onUpdateNodes)
  const [hoveredNode, setHoveredNode] = useState<string | null>(null)
  const physicsTicksRef = useRef(0)
  const settleStartedAtRef = useRef<number | null>(null)
  const settledRef = useRef(false)

  layoutSettingsRef.current = layoutSettings
  onUpdateNodesRef.current = onUpdateNodes
  selectedNodeIdRef.current = selectedNodeId

  const fitCurrentGraph = useCallback((force = false) => {
    const canvas = canvasRef.current
    if (!canvas || nodesRef.current.length === 0) return
    if (!force && userNavigatedRef.current) return
    const rect = canvas.getBoundingClientRect()
    const visibleIds = visibleNodeIdsFor(nodesRef.current, edges, filters.depth)
    const fitNodes = nodesRef.current.filter(node => visibleIds.has(node.id) && filters.nodeTypes.has(node.type))
    fitGraphToView(fitNodes.length > 0 ? fitNodes : nodesRef.current, rect.width, rect.height, panRef.current, zoomRef)
  }, [edges, filters.depth, filters.nodeTypes])

  useEffect(() => {
    let worker: Worker | null = null
    try {
      worker = new Worker(new URL('../physics/graphLayoutWorker.ts', import.meta.url), { type: 'module' })
      layoutWorkerRef.current = worker
      worker.onmessage = (event: MessageEvent<LayoutWorkerResponse>) => {
        const message = event.data
        if (message.type !== 'layout' || message.signature !== layoutWorkerSignatureRef.current) return
        nodesRef.current = message.nodes
        physicsTicksRef.current = 0
        settleStartedAtRef.current = null
        settledRef.current = false
        userNavigatedRef.current = false
        requestAnimationFrame(() => fitCurrentGraph(true))
      }
    } catch (error) {
      console.warn('Graph layout worker is unavailable, falling back to main-thread layout.', error)
      layoutWorkerRef.current = null
    }

    return () => {
      worker?.terminate()
      if (layoutWorkerRef.current === worker) {
        layoutWorkerRef.current = null
      }
    }
  }, [fitCurrentGraph])

  // keep nodesRef in sync
  useEffect(() => {
    const signature = `${nodes.map(n => n.id).join('|')}::${edges.map(e => e.id).join('|')}`
    if (signature !== graphSignatureRef.current) {
      const previousNodes = nodesRef.current
      const previous = new Map(previousNodes.map(node => [node.id, node]))
      graphSignatureRef.current = signature
      visibleSignatureRef.current = visibleGraphSignature(nodes, edges, filters)
      if (layoutWorkerRef.current && nodes.length >= SPATIAL_GRID_THRESHOLD) {
        const workerSignature = `${signature}::${layoutSettingsSignature(layoutSettings)}`
        layoutWorkerSignatureRef.current = workerSignature
        nodesRef.current = seedLayout(nodes, edges, previous)
        layoutWorkerRef.current.postMessage({
          type: 'layout',
          signature: workerSignature,
          nodes,
          edges,
          previousNodes,
          layoutSettings,
        })
      } else {
        nodesRef.current = prepareInitialLayout(nodes, edges, previous, layoutSettings)
      }
      physicsTicksRef.current = 0
      settleStartedAtRef.current = null
      settledRef.current = false
      userNavigatedRef.current = false
      requestAnimationFrame(() => fitCurrentGraph(true))
    } else {
      const current = new Map(nodesRef.current.map(node => [node.id, node]))
      nodesRef.current = nodes.map(node => {
        const existing = current.get(node.id)
        return existing ? { ...node, x: existing.x, y: existing.y, vx: existing.vx, vy: existing.vy } : { ...node, vx: 0, vy: 0 }
      })
    }
  }, [edges, filters, fitCurrentGraph, nodes])

  useEffect(() => {
    const signature = visibleGraphSignature(nodesRef.current, edges, filters)
    if (signature === visibleSignatureRef.current) return
    visibleSignatureRef.current = signature
    layoutWorkerSignatureRef.current = ''
    physicsTicksRef.current = 0
    settleStartedAtRef.current = null
    settledRef.current = false
    userNavigatedRef.current = false
    requestAnimationFrame(() => fitCurrentGraph(true))
  }, [edges, filters, fitCurrentGraph])

  useEffect(() => {
    physicsTicksRef.current = 0
    settleStartedAtRef.current = null
    settledRef.current = false
  }, [layoutSettings])

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
    return visibleNodeIdsFor(nodesRef.current, edges, filters.depth)
  }, [edges, filters.depth])

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
      const visibleIds = getVisibleNodeIds()
      const currentLayoutSettings = layoutSettingsRef.current

      if (settleStartedAtRef.current === null) {
        settleStartedAtRef.current = ts
      }
      const settleElapsed = ts - settleStartedAtRef.current
      if (settleElapsed <= VISIBLE_SETTLE_MS) {
        nodesRef.current = runVisiblePhysicsTick(nodesRef.current, edges, visibleIds, filters, W, H, { layoutSettings: currentLayoutSettings })
        physicsTicksRef.current++
        if (!userNavigatedRef.current && settleElapsed < FIT_SETTLE_MS) {
          const fitNodes = nodesRef.current.filter(node => visibleIds.has(node.id) && filters.nodeTypes.has(node.type))
          fitGraphToView(fitNodes.length > 0 ? fitNodes : nodesRef.current, W, H, panRef.current, zoomRef)
        }
      } else if (!settledRef.current) {
        const fadeProgress = Math.min(1, (settleElapsed - VISIBLE_SETTLE_MS) / SETTLE_FADE_MS)
        const dampingScale = 2.4 + fadeProgress * 4.6
        const dragScale = 2.2 + fadeProgress * 5.2
        const velocityFade = 1 - 0.08 - fadeProgress * 0.18
        nodesRef.current = runVisiblePhysicsTick(nodesRef.current, edges, visibleIds, filters, W, H, {
          dampingScale,
          dragScale,
          maxSpeedScale: 0.55,
          maxForceScale: 0.45,
          layoutSettings: currentLayoutSettings,
        }).map(node => ({
          ...node,
          vx: node.vx * Math.max(0.68, velocityFade),
          vy: node.vy * Math.max(0.68, velocityFade),
        }))
        physicsTicksRef.current++
        if (averageSpeed(nodesRef.current) < 0.012 || fadeProgress >= 1) {
          settledRef.current = true
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

      const nodeMap = new Map(nodesRef.current.map(n => [n.id, n]))
      const degree = buildDegreeMap(nodesRef.current, edges)
      const hoveredConnections = new Set<string>()
      const selectedConnections = new Set<string>()
      const activeSelectedNodeId = selectedNodeIdRef.current
      if (hoveredNodeRef.current) {
        edges.forEach(e => {
          if (e.source === hoveredNodeRef.current || e.target === hoveredNodeRef.current) {
            hoveredConnections.add(e.source)
            hoveredConnections.add(e.target)
          }
        })
      }
      if (activeSelectedNodeId) {
        edges.forEach(e => {
          if (e.source === activeSelectedNodeId || e.target === activeSelectedNodeId) {
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
        const width = edge.type === 'DataFlow' || edge.type === 'ApiCall' || edge.type === 'EndpointHandler' ? 2.5 : edge.type === 'Calls' || edge.type === 'Renders' ? 1.8 : 1.2
        const dashed = edge.type === 'Implements' || edge.type === 'ExternalDependency' || edge.type === 'Renders'
        const animated = edge.type === 'DataFlow' || edge.type === 'ApiCall' || edge.type === 'EndpointHandler'

        drawArrow(ctx, src.x, src.y, tgt.x, tgt.y, color, width, dashed, animated, ts, NODE_SIZES[src.type], NODE_SIZES[tgt.type])
        if ((edge.bundledCount ?? 1) > 1) {
          drawEdgeBundleBadge(ctx, src, tgt, edge.bundledCount ?? 1, canvasColors)
        }
      }

      // draw nodes
      const labelCandidates: LabelCandidate[] = []
      for (const n of nodesRef.current) {
        if (!filters.nodeTypes.has(n.type)) continue
        if (!visibleIds.has(n.id)) continue
        const isSelected = n.id === activeSelectedNodeId
        const isHovered = n.id === hoveredNodeRef.current
        const isFocusContext = visibleIds.has(n.id)
        const isFaded = hoveredNodeRef.current !== null && !hoveredConnections.has(n.id) && !isHovered && n.id !== hoveredNodeRef.current

        drawNode(ctx, n, isSelected, isHovered, isFocusContext, isFaded, canvasColors)
        const diagnostics = diagnosticsByNode?.get(n.id) ?? []
        if (diagnostics.length > 0) {
          drawDiagnosticBadge(ctx, n, diagnostics.some(diagnostic => diagnostic.severity === 'Error') ? 'Error' : 'Warning')
        }
        if (n.description?.startsWith('Collapsed:') && (n.connections ?? 0) > 0) {
          drawGroupCountBadge(ctx, n, n.connections ?? 0, canvasColors)
        }
        labelCandidates.push({
          node: n,
          isSelected,
          isHovered,
          degree: degree.get(n.id) ?? 0,
          priority: labelPriority(n, degree.get(n.id) ?? 0, isSelected, isHovered),
        })

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
      drawLabels(ctx, labelCandidates, labelCandidates.length, zoomRef.current, labelMode, canvasColors)

      ctx.restore()

      // minimap (screen-space)
      drawMiniMap(ctx, nodesRef.current.filter(n => visibleIds.has(n.id)), panRef.current, zoomRef.current, W, H, canvasColors)

      // "You are here" breadcrumb for selected
      if (activeSelectedNodeId) {
        const sel = nodeMap.get(activeSelectedNodeId)
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
  }, [diagnosticsByNode, edges, filters, getVisibleNodeIds, labelMode, theme])

  // mouse events
  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    const hit = hitTest(e.clientX, e.clientY)
    if (hit) {
      dragNodeRef.current = hit.id
      isDraggingRef.current = false
      suppressNextClickRef.current = false
      dragStartRef.current = { x: e.clientX, y: e.clientY, panX: panRef.current.x, panY: panRef.current.y }
    } else {
      dragNodeRef.current = null
      isDraggingRef.current = true
      suppressNextClickRef.current = false
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
      const dx = e.clientX - dragStartRef.current.x
      const dy = e.clientY - dragStartRef.current.y
      if (dx * dx + dy * dy > 16) {
        suppressNextClickRef.current = true
        userNavigatedRef.current = true
      }
      const { x, y } = toWorld(e.clientX, e.clientY)
      nodesRef.current = nodesRef.current.map(n =>
        n.id === dragNodeRef.current ? { ...n, x, y, vx: 0, vy: 0, pinned: true } : n
      )
    } else if (isDraggingRef.current) {
      panRef.current.x = dragStartRef.current.panX + (e.clientX - dragStartRef.current.x)
      panRef.current.y = dragStartRef.current.panY + (e.clientY - dragStartRef.current.y)
    }
  }, [hitTest, toWorld])

  const handleMouseUp = useCallback(() => {
    if (dragNodeRef.current) {
      onUpdateNodesRef.current([...nodesRef.current])
    }
    dragNodeRef.current = null
    isDraggingRef.current = false
  }, [])

  const handleClick = useCallback((e: React.MouseEvent) => {
    if (suppressNextClickRef.current) {
      suppressNextClickRef.current = false
      return
    }
    const hit = hitTest(e.clientX, e.clientY)
    onSelectNode(hit?.id ?? null)
  }, [hitTest, onSelectNode])

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
        onWheel={handleWheel}
      />
    </div>
  )
}

function drawGroupCountBadge(ctx: CanvasRenderingContext2D, n: GraphNode, count: number, theme: CanvasTheme) {
  const size = NODE_SIZES[n.type]
  const x = n.x + size * 0.65
  const y = n.y - size * 0.65
  ctx.save()
  ctx.fillStyle = theme.card
  ctx.strokeStyle = NODE_COLORS[n.type]
  ctx.lineWidth = 1
  ctx.beginPath()
  ctx.roundRect(x - 11, y - 8, 22, 16, 8)
  ctx.fill()
  ctx.stroke()
  ctx.fillStyle = theme.text
  ctx.font = '9px Inter, sans-serif'
  ctx.textAlign = 'center'
  ctx.textBaseline = 'middle'
  ctx.fillText(String(count), x, y)
  ctx.restore()
}
