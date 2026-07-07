import type { EdgeType, GraphEdge, GraphNode } from '../types'
import type { LocalNeighborhoodGroup, LocalNeighborhoodModel } from '../views/architecture/architectureTypes'

export type LocalNeighborhoodRadius = 1 | 2 | 3

const DENSE_NODE_LIMIT = 80
const TYPE_NODE_TYPES = new Set(['Struct', 'Class', 'Enum', 'Trait', 'Interface', 'TypeAlias'])
const EDGE_LABELS: Partial<Record<EdgeType, string>> = {
  Calls: 'Calls',
  Imports: 'Imports',
  Uses: 'Uses',
  TypeReference: 'Type references',
  ApiCall: 'API calls',
  DataFlow: 'Data flow',
  EndpointHandler: 'Endpoint handlers',
  Renders: 'Renders',
  Implements: 'Implements',
  ExternalDependency: 'External dependencies',
  ModDeclaration: 'Module declarations',
  Contains: 'Contains',
}
const EDGE_ORDER: EdgeType[] = [
  'Calls',
  'Imports',
  'Uses',
  'TypeReference',
  'ApiCall',
  'DataFlow',
  'EndpointHandler',
  'Renders',
  'Implements',
  'ExternalDependency',
  'ModDeclaration',
  'Contains',
]

export function buildLocalNeighborhoodModel(
  nodes: GraphNode[],
  edges: GraphEdge[],
  selectedNodeId: string,
  radius: LocalNeighborhoodRadius,
): LocalNeighborhoodModel {
  const byId = new Map(nodes.map(node => [node.id, node]))
  const centerNode = byId.get(selectedNodeId) ?? null
  if (!centerNode) return emptyModel()

  const incomingEdges = edges.filter(edge => edge.target === selectedNodeId)
  const outgoingEdges = edges.filter(edge => edge.source === selectedNodeId)
  const expandedIds = expandNeighborhood(edges, selectedNodeId, radius)
  const limitedIds = radius === 3 && expandedIds.size > DENSE_NODE_LIMIT
    ? limitDenseNeighborhood(expandedIds, edges, selectedNodeId, DENSE_NODE_LIMIT)
    : expandedIds
  const visibleNodes = nodes.filter(node => limitedIds.has(node.id))
  const visibleEdges = edges.filter(edge => limitedIds.has(edge.source) && limitedIds.has(edge.target))
  const relatedApiNodes = visibleNodes.filter(node => node.type === 'Endpoint')
  const relatedTypeNodes = visibleNodes.filter(node => TYPE_NODE_TYPES.has(node.type))
  const relatedTestNodes = visibleNodes.filter(node => isTestNode(node))

  return {
    centerNode,
    incomingNodes: uniqueNodes(incomingEdges.map(edge => byId.get(edge.source))),
    outgoingNodes: uniqueNodes(outgoingEdges.map(edge => byId.get(edge.target))),
    incomingGroups: groupEdgesByType(incomingEdges, byId, 'source'),
    outgoingGroups: groupEdgesByType(outgoingEdges, byId, 'target'),
    relatedApiNodes,
    relatedTypeNodes,
    relatedTestNodes,
    visibleNodes,
    visibleEdges,
    incomingCountByType: countEdgesByType(incomingEdges),
    outgoingCountByType: countEdgesByType(outgoingEdges),
    isDense: limitedIds.size < expandedIds.size,
    denseNodeLimit: limitedIds.size < expandedIds.size ? DENSE_NODE_LIMIT : undefined,
  }
}

function emptyModel(): LocalNeighborhoodModel {
  return {
    centerNode: null,
    incomingNodes: [],
    outgoingNodes: [],
    incomingGroups: [],
    outgoingGroups: [],
    relatedApiNodes: [],
    relatedTypeNodes: [],
    relatedTestNodes: [],
    visibleNodes: [],
    visibleEdges: [],
    incomingCountByType: {},
    outgoingCountByType: {},
    isDense: false,
  }
}

function groupEdgesByType(edges: GraphEdge[], byId: Map<string, GraphNode>, nodeSide: 'source' | 'target'): LocalNeighborhoodGroup[] {
  const groups = new Map<EdgeType, { nodes: Map<string, GraphNode>; edges: GraphEdge[] }>()
  for (const edge of edges) {
    const node = byId.get(edge[nodeSide])
    if (!node) continue
    const group = groups.get(edge.type) ?? { nodes: new Map<string, GraphNode>(), edges: [] }
    group.nodes.set(node.id, node)
    group.edges.push(edge)
    groups.set(edge.type, group)
  }
  return [...groups.entries()]
    .map(([edgeType, group]) => ({
      label: EDGE_LABELS[edgeType] ?? edgeType,
      edgeType,
      nodes: [...group.nodes.values()].sort(compareNodes),
      edges: group.edges,
    }))
    .sort((a, b) => edgeRank(a.edgeType as EdgeType) - edgeRank(b.edgeType as EdgeType) || a.label.localeCompare(b.label))
}

function countEdgesByType(edges: GraphEdge[]) {
  const counts: Partial<Record<EdgeType, number>> = {}
  for (const edge of edges) {
    counts[edge.type] = (counts[edge.type] ?? 0) + Math.max(1, edge.bundledCount ?? 1)
  }
  return counts
}

function expandNeighborhood(edges: GraphEdge[], selectedNodeId: string, radius: LocalNeighborhoodRadius) {
  const visible = new Set<string>([selectedNodeId])
  let frontier = new Set<string>([selectedNodeId])
  for (let depth = 0; depth < radius; depth += 1) {
    const next = new Set<string>()
    for (const edge of edges) {
      if (frontier.has(edge.source) && !visible.has(edge.target)) next.add(edge.target)
      if (frontier.has(edge.target) && !visible.has(edge.source)) next.add(edge.source)
    }
    next.forEach(id => visible.add(id))
    frontier = next
    if (!frontier.size) break
  }
  return visible
}

function limitDenseNeighborhood(expandedIds: Set<string>, edges: GraphEdge[], selectedNodeId: string, limit: number) {
  const degree = new Map<string, number>()
  for (const id of expandedIds) degree.set(id, id === selectedNodeId ? Number.MAX_SAFE_INTEGER : 0)
  for (const edge of edges) {
    if (!expandedIds.has(edge.source) || !expandedIds.has(edge.target)) continue
    const weight = Math.max(1, edge.bundledCount ?? 1)
    degree.set(edge.source, (degree.get(edge.source) ?? 0) + weight)
    degree.set(edge.target, (degree.get(edge.target) ?? 0) + weight)
  }
  return new Set(
    [...expandedIds]
      .sort((a, b) => (degree.get(b) ?? 0) - (degree.get(a) ?? 0) || a.localeCompare(b))
      .slice(0, limit),
  )
}

function uniqueNodes(nodes: Array<GraphNode | undefined>) {
  const byId = new Map<string, GraphNode>()
  for (const node of nodes) {
    if (node) byId.set(node.id, node)
  }
  return [...byId.values()].sort(compareNodes)
}

function compareNodes(a: GraphNode, b: GraphNode) {
  return a.label.localeCompare(b.label) || a.id.localeCompare(b.id)
}

function edgeRank(edgeType: EdgeType) {
  const index = EDGE_ORDER.indexOf(edgeType)
  return index === -1 ? EDGE_ORDER.length : index
}

function isTestNode(node: GraphNode) {
  const file = (node.file ?? '').toLowerCase()
  return file.includes('/test/') || file.includes('/tests/') || file.endsWith('.test.ts') || file.endsWith('.spec.ts') || file.endsWith('_test.rs') || node.label.toLowerCase().includes('test')
}
