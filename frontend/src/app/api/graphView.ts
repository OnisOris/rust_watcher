import type { GraphEdge, GraphFilters, GraphNode } from '../types'

export function applyGraphFilters(graph: { nodes: GraphNode[]; edges: GraphEdge[] }, filters: GraphFilters) {
  const nodes = graph.nodes.filter(node => matchesLanguageFilter(node, filters))
  const nodeIds = new Set(nodes.map(node => node.id))
  const edges = graph.edges.filter(edge => nodeIds.has(edge.source) && nodeIds.has(edge.target))
  return { nodes, edges }
}

export function applyCollapsedGroups(
  graph: { nodes: GraphNode[]; edges: GraphEdge[] },
  collapsedGroups: Set<string>,
) {
  if (!collapsedGroups.size) return graph
  const visibleGroupIds = new Set(graph.nodes.map(node => node.id))
  const ownerByHiddenNode = new Map<string, string>()
  const containsBySource = new Map<string, string[]>()

  for (const edge of graph.edges) {
    if (edge.type !== 'Contains') continue
    const children = containsBySource.get(edge.source) ?? []
    children.push(edge.target)
    containsBySource.set(edge.source, children)
  }

  for (const groupId of collapsedGroups) {
    if (!visibleGroupIds.has(groupId)) continue
    const queue = [...(containsBySource.get(groupId) ?? [])]
    while (queue.length) {
      const childId = queue.shift()
      if (!childId || childId === groupId || ownerByHiddenNode.has(childId)) continue
      ownerByHiddenNode.set(childId, groupId)
      queue.push(...(containsBySource.get(childId) ?? []))
    }
  }

  const nodes = graph.nodes.filter(node => !ownerByHiddenNode.has(node.id))
  const nodeIds = new Set(nodes.map(node => node.id))
  const edgeById = new Map<string, GraphEdge>()

  for (const edge of graph.edges) {
    const source = ownerByHiddenNode.get(edge.source) ?? edge.source
    const target = ownerByHiddenNode.get(edge.target) ?? edge.target
    if (source === target || !nodeIds.has(source) || !nodeIds.has(target)) continue
    const id = source === edge.source && target === edge.target
      ? edge.id
      : `${edge.type}:${source}->${target}`
    edgeById.set(id, { ...edge, id, source, target })
  }

  return { nodes, edges: [...edgeById.values()] }
}

export function matchesLanguageFilter(node: GraphNode, filters: GraphFilters) {
  if (node.type === 'Endpoint') return filters.languages.has('endpoints')
  if (node.type === 'ExternalCrate' || node.crate === 'external') return filters.languages.has('external')
  const language = (node.language ?? '').toLowerCase()
  if (language === 'javascript' || language === 'typescript') return filters.languages.has('typescript')
  if (language === 'rust') return filters.languages.has('rust')
  if (language === 'python') return filters.languages.has('python')
  if (language === 'qml') return filters.languages.has('qml')
  return true
}
