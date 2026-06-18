import { DEFAULT_GRAPH_LAYOUT_SETTINGS } from '../types'
import type { GraphEdge, GraphNode, NodeType, EdgeType, GraphLayoutSettings } from '../types'

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
  Component: 18,
  Hook: 13,
  Interface: 16,
  TypeAlias: 15,
  Endpoint: 16,
  Macro: 12,
}

const BASE_REPULSION = 9000
const SPRING_STRENGTH = 0.028
const BASE_SPRING_LENGTH = 132
const HUB_MASS = 0.72
const CENTER_GRAVITY = 0.0035
const COLLISION_PADDING = 18
const COLLISION_STRENGTH = 0.18
const MAX_FORCE = 48
const MAX_NODE_FORCE = 24
const GOLDEN_ANGLE = Math.PI * (3 - Math.sqrt(5))
const SPATIAL_GRID_THRESHOLD = 220
const SPATIAL_CELL_SIZE = 260
const SPATIAL_SEARCH_RADIUS = 2

type LayoutRequest = {
  type: 'layout'
  signature: string
  nodes: GraphNode[]
  edges: GraphEdge[]
  previousNodes: GraphNode[]
  layoutSettings?: GraphLayoutSettings
}

type LayoutResponse = {
  type: 'layout'
  signature: string
  nodes: GraphNode[]
}

self.onmessage = (event: MessageEvent<LayoutRequest>) => {
  const message = event.data
  if (message.type !== 'layout') return
  const previous = new Map(message.previousNodes.map(node => [node.id, node]))
  const nodes = prepareInitialLayout(message.nodes, message.edges, previous, message.layoutSettings ?? DEFAULT_GRAPH_LAYOUT_SETTINGS)
  const response: LayoutResponse = { type: 'layout', signature: message.signature, nodes }
  self.postMessage(response)
}

function prepareInitialLayout(
  nodes: GraphNode[],
  edges: GraphEdge[],
  previous: Map<string, GraphNode>,
  layoutSettings: GraphLayoutSettings,
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

function solveEquilibriumLayout(nodes: GraphNode[], edges: GraphEdge[], layoutSettings: GraphLayoutSettings) {
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

function runEquilibriumStep(nodes: GraphNode[], edges: GraphEdge[], step: number, layoutSettings: GraphLayoutSettings) {
  const updated = nodes.map(node => ({ ...node }))
  const index = new Map(updated.map(node => [node.id, node]))
  const forces = new Map(updated.map(node => [node.id, { x: 0, y: 0 }]))
  const degree = buildDegreeMap(updated, edges)
  const densityScale = Math.min(3.4, Math.max(1, Math.sqrt(updated.length / 80)))
  const spacingScale = Math.max(0.55, layoutSettings.spacing)
  const repulsion = BASE_REPULSION * densityScale * densityScale * Math.max(0.25, layoutSettings.repulsion) * spacingScale * spacingScale
  const springLength = BASE_SPRING_LENGTH * Math.min(2.6, densityScale) * Math.max(0.45, layoutSettings.linkLength) * spacingScale
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

function seedLayout(nodes: GraphNode[], edges: GraphEdge[], previous: Map<string, GraphNode>) {
  const neighbors = buildNeighborIds(edges)
  const groups = groupNodes(nodes)
  const groupCenters = groupCenterMap(groups)
  const prepared = new Map<string, GraphNode>()
  return nodes.map(node => {
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
  const typeScale = edgeType === 'Contains' ? 0.82 : edgeType === 'ApiCall' ? 0.72 : 1
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
