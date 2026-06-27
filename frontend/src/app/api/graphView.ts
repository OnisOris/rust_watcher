import type { DiagnosticRecord, EdgeType, GraphEdge, GraphFilters, GraphNode } from '../types'
import { inferNodeLanguage, languageFilterKey } from './language'

export interface CollapsedGroupStats {
  groupId: string
  hiddenNodeCount: number
  hiddenDiagnosticCount: number
  incomingEdgeTypes: EdgeType[]
  outgoingEdgeTypes: EdgeType[]
  language?: string
}

export function applyGraphFilters(graph: { nodes: GraphNode[]; edges: GraphEdge[] }, filters: GraphFilters) {
  const nodes = graph.nodes.filter(node => matchesLanguageFilter(node, filters) && matchesReachabilityFilter(node, filters))
  const nodeIds = new Set(nodes.map(node => node.id))
  const edges = applyEdgeVisibilityLevel(
    graph.edges.filter(edge => nodeIds.has(edge.source) && nodeIds.has(edge.target)),
    filters,
  )
  return { nodes, edges }
}

export function applyEdgeVisibilityLevel(edges: GraphEdge[], filters: Pick<GraphFilters, 'edgeTypes' | 'edgeVisibility'>) {
  return edges.filter(edge => {
    if (!filters.edgeTypes.has(edge.type)) return false
    if (filters.edgeVisibility === 'All') return true
    if (isEssentialEdge(edge)) return true
    if (filters.edgeVisibility === 'Semantic') {
      return edge.type === 'Calls'
        || edge.type === 'Imports'
        || edge.type === 'Uses'
        || edge.type === 'DataFlow'
        || edge.type === 'TypeReference'
    }
    return false
  })
}

export function bundleEdges(edges: GraphEdge[]) {
  const byKey = new Map<string, GraphEdge[]>()
  for (const edge of edges) {
    const dataFlowPart = edge.type === 'DataFlow'
      ? `::${edge.dataFlowKind ?? 'Unknown'}::${edge.label ?? ''}`
      : ''
    const key = `${edge.source}::${edge.target}::${edge.type}${dataFlowPart}`
    const group = byKey.get(key) ?? []
    group.push(edge)
    byKey.set(key, group)
  }
  return [...byKey.values()].map(group => {
    const [first] = group
    if (group.length === 1) return first
    return {
      ...first,
      id: `${first.type}:${first.source}->${first.target}:bundle:${group.length}`,
      bundledCount: group.length,
      bundledTypes: [...new Set(group.map(edge => edge.type))],
      bundledEdgeIds: group.map(edge => edge.id),
    }
  })
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
  const remappedEdges: GraphEdge[] = []

  for (const edge of graph.edges) {
    const source = ownerByHiddenNode.get(edge.source) ?? edge.source
    const target = ownerByHiddenNode.get(edge.target) ?? edge.target
    if (source === target || !nodeIds.has(source) || !nodeIds.has(target)) continue
    const id = source === edge.source && target === edge.target
      ? edge.id
      : `${edge.type}:${source}->${target}:${edge.id}`
    remappedEdges.push({ ...edge, id, source, target })
  }

  return { nodes, edges: remappedEdges }
}

export function buildCollapsedGroupStats(
  graph: { nodes: GraphNode[]; edges: GraphEdge[] },
  collapsedGroups: Set<string>,
  diagnosticsByNode: Map<string, DiagnosticRecord[]> = new Map(),
) {
  const hiddenByGroup = hiddenNodesByCollapsedGroup(graph.edges, collapsedGroups)
  const nodesById = new Map(graph.nodes.map(node => [node.id, node]))
  const stats = new Map<string, CollapsedGroupStats>()

  hiddenByGroup.forEach((hiddenIds, groupId) => {
    let hiddenDiagnosticCount = 0
    const incoming = new Set<EdgeType>()
    const outgoing = new Set<EdgeType>()
    for (const hiddenId of hiddenIds) {
      hiddenDiagnosticCount += diagnosticsByNode.get(hiddenId)?.length ?? 0
    }
    for (const edge of graph.edges) {
      const sourceHidden = hiddenIds.has(edge.source)
      const targetHidden = hiddenIds.has(edge.target)
      if (sourceHidden && !targetHidden && edge.target !== groupId) outgoing.add(edge.type)
      if (!sourceHidden && targetHidden && edge.source !== groupId) incoming.add(edge.type)
    }
    stats.set(groupId, {
      groupId,
      hiddenNodeCount: hiddenIds.size,
      hiddenDiagnosticCount,
      incomingEdgeTypes: [...incoming],
      outgoingEdgeTypes: [...outgoing],
      language: nodesById.get(groupId)?.language,
    })
  })

  return stats
}

export function buildRouteFlowGraph(graph: { nodes: GraphNode[]; edges: GraphEdge[] }) {
  const keepEdges = new Set<string>()
  const keepNodes = new Set<string>()
  const nodesById = new Map(graph.nodes.map(node => [node.id, node]))
  const outgoing = edgesBySource(graph.edges)
  const byPair = edgesByPair(graph.edges)

  for (const edge of graph.edges) {
    if (edge.type !== 'ApiCall') continue
    if (isDetachedNode(nodesById.get(edge.source)) || isDetachedNode(nodesById.get(edge.target))) continue
    keepEdges.add(edge.id)
    keepNodes.add(edge.source)
    keepNodes.add(edge.target)
    keepDataFlowBetween(keepEdges, byPair, edge.source, edge.target, ['ApiRequest'])
    for (const handler of outgoing.get(edge.target) ?? []) {
      if (handler.type !== 'EndpointHandler') continue
      if (isDetachedNode(nodesById.get(handler.target))) continue
      keepEdges.add(handler.id)
      keepNodes.add(handler.target)
      keepDataFlowBetween(keepEdges, byPair, handler.target, edge.target, ['ApiResponse', 'ReturnValue'])
      for (const call of outgoing.get(handler.target) ?? []) {
        if (call.type === 'Calls') {
          keepEdges.add(call.id)
          keepNodes.add(call.target)
          keepDataFlowBetween(keepEdges, byPair, call.target, handler.target, ['ReturnValue', 'Assignment', 'ModelUse'])
          keepDataFlowBetween(keepEdges, byPair, handler.target, call.target, ['Argument', 'ModelUse'])
        }
      }
    }
  }

  let grew = true
  while (grew) {
    grew = false
    for (const edge of graph.edges) {
      if (edge.type !== 'DataFlow') continue
      if (!keepNodes.has(edge.source) && !keepNodes.has(edge.target)) continue
      if (!keepEdges.has(edge.id)) {
        keepEdges.add(edge.id)
        grew = true
      }
      if (!keepNodes.has(edge.source)) {
        keepNodes.add(edge.source)
        grew = true
      }
      if (!keepNodes.has(edge.target)) {
        keepNodes.add(edge.target)
        grew = true
      }
    }
  }

  return {
    nodes: graph.nodes.filter(node =>
      !isDetachedNode(node)
      && (keepNodes.has(node.id) || (node.type === 'Endpoint' && hasApiEdge(graph.edges, node.id)))
    ),
    edges: graph.edges.filter(edge =>
      keepEdges.has(edge.id)
      && nodesById.has(edge.source)
      && nodesById.has(edge.target)
    ),
  }
}

export function matchesLanguageFilter(node: GraphNode, filters: GraphFilters) {
  const key = languageFilterKey(inferNodeLanguage(node))
  if (!key) return true
  return filters.languages.has(key)
}

export function matchesReachabilityFilter(node: GraphNode, filters: Pick<GraphFilters, 'showDetached'>) {
  return filters.showDetached || node.reachability !== 'Detached'
}

function isEssentialEdge(edge: GraphEdge) {
  return edge.type === 'Contains'
    || edge.type === 'EndpointHandler'
    || edge.type === 'ApiCall'
    || edge.type === 'Renders'
    || (edge.type === 'Calls' && (edge.confidence === 'Exact' || edge.confidence === 'Semantic'))
}

function keepDataFlowBetween(
  keepEdges: Set<string>,
  byPair: Map<string, GraphEdge[]>,
  source: string,
  target: string,
  kinds: string[],
) {
  for (const edge of byPair.get(`${source}->${target}`) ?? []) {
    if (edge.type !== 'DataFlow') continue
    if (!edge.dataFlowKind || kinds.includes(edge.dataFlowKind)) {
      keepEdges.add(edge.id)
    }
  }
}

function hiddenNodesByCollapsedGroup(edges: GraphEdge[], collapsedGroups: Set<string>) {
  const containsBySource = new Map<string, string[]>()
  for (const edge of edges) {
    if (edge.type !== 'Contains') continue
    const children = containsBySource.get(edge.source) ?? []
    children.push(edge.target)
    containsBySource.set(edge.source, children)
  }
  const hiddenByGroup = new Map<string, Set<string>>()
  for (const groupId of collapsedGroups) {
    const hidden = new Set<string>()
    const queue = [...(containsBySource.get(groupId) ?? [])]
    while (queue.length) {
      const childId = queue.shift()
      if (!childId || hidden.has(childId) || childId === groupId) continue
      hidden.add(childId)
      queue.push(...(containsBySource.get(childId) ?? []))
    }
    hiddenByGroup.set(groupId, hidden)
  }
  return hiddenByGroup
}

function edgesBySource(edges: GraphEdge[]) {
  const bySource = new Map<string, GraphEdge[]>()
  for (const edge of edges) {
    const list = bySource.get(edge.source) ?? []
    list.push(edge)
    bySource.set(edge.source, list)
  }
  return bySource
}

function edgesByPair(edges: GraphEdge[]) {
  const byPair = new Map<string, GraphEdge[]>()
  for (const edge of edges) {
    const key = `${edge.source}->${edge.target}`
    const list = byPair.get(key) ?? []
    list.push(edge)
    byPair.set(key, list)
  }
  return byPair
}

function hasApiEdge(edges: GraphEdge[], nodeId: string) {
  return edges.some(edge => edge.type === 'ApiCall' && (edge.source === nodeId || edge.target === nodeId))
}

function isDetachedNode(node?: GraphNode) {
  return node?.reachability === 'Detached'
}
