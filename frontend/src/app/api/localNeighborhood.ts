import type { GraphEdge, GraphNode } from '../types'
import type { LocalNeighborhoodModel } from '../views/architecture/architectureTypes'

export function buildLocalNeighborhoodModel(
  nodes: GraphNode[],
  edges: GraphEdge[],
  selectedNodeId: string,
  radius: 1 | 2 | 3,
): LocalNeighborhoodModel {
  const byId = new Map(nodes.map(node => [node.id, node]))
  const centerNode = byId.get(selectedNodeId) ?? null
  if (!centerNode) {
    return {
      centerNode: null,
      incomingNodes: [],
      outgoingNodes: [],
      relatedApiNodes: [],
      relatedTypeNodes: [],
      relatedTestNodes: [],
      visibleNodes: [],
      visibleEdges: [],
    }
  }

  const incomingEdges = edges.filter(edge => edge.target === selectedNodeId)
  const outgoingEdges = edges.filter(edge => edge.source === selectedNodeId)
  const visibleIds = expandNeighborhood(edges, selectedNodeId, radius)
  const visibleNodes = nodes.filter(node => visibleIds.has(node.id))
  const visibleEdges = edges.filter(edge => visibleIds.has(edge.source) && visibleIds.has(edge.target))

  return {
    centerNode,
    incomingNodes: uniqueNodes(incomingEdges.map(edge => byId.get(edge.source))),
    outgoingNodes: uniqueNodes(outgoingEdges.map(edge => byId.get(edge.target))),
    relatedApiNodes: visibleNodes.filter(node => node.type === 'Endpoint'),
    relatedTypeNodes: visibleNodes.filter(node => ['Struct', 'Class', 'Enum', 'Trait', 'Interface', 'TypeAlias'].includes(node.type)),
    relatedTestNodes: visibleNodes.filter(node => isTestNode(node)),
    visibleNodes,
    visibleEdges,
  }
}

function expandNeighborhood(edges: GraphEdge[], selectedNodeId: string, radius: 1 | 2 | 3) {
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

function uniqueNodes(nodes: Array<GraphNode | undefined>) {
  const byId = new Map<string, GraphNode>()
  for (const node of nodes) {
    if (node) byId.set(node.id, node)
  }
  return [...byId.values()]
}

function isTestNode(node: GraphNode) {
  const file = (node.file ?? '').toLowerCase()
  return file.includes('/test/') || file.includes('/tests/') || file.endsWith('.test.ts') || file.endsWith('_test.rs') || node.label.toLowerCase().includes('test')
}
