import type { EdgeType, GraphEdge, GraphFilters, GraphMode, GraphNode, NodeType } from '../types'

export function applyGraphMode(graph: { nodes: GraphNode[]; edges: GraphEdge[] }, mode: GraphMode) {
  const { nodeTypes, edgeTypes } = modeVisibility(mode)
  let nodes = graph.nodes.filter(node => nodeTypes.has(node.type))
  const nodeIds = new Set(nodes.map(node => node.id))
  let edges = graph.edges.filter(edge =>
    edgeTypes.has(edge.type) && nodeIds.has(edge.source) && nodeIds.has(edge.target)
  )

  if (mode === 'CallFlow' && edges.length === 0) {
    edges = graph.edges.filter(edge =>
      edge.type === 'Contains' && nodeIds.has(edge.source) && nodeIds.has(edge.target)
    )
  }

  if (mode === 'CallFlow' || mode === 'DataFlow' || mode === 'Traits') {
    const semanticEdgeTypes = mode === 'CallFlow'
      ? new Set<EdgeType>(['Calls', 'Renders', 'ApiCall', 'EndpointHandler'])
      : mode === 'DataFlow'
        ? new Set<EdgeType>(['DataFlow', 'ApiCall', 'EndpointHandler'])
        : null
    const semanticNodeIds = new Set<string>()
    for (const edge of edges) {
      if (semanticEdgeTypes && !semanticEdgeTypes.has(edge.type)) continue
      semanticNodeIds.add(edge.source)
      semanticNodeIds.add(edge.target)
    }
    if (semanticNodeIds.size) {
      nodes = nodes.filter(node => semanticNodeIds.has(node.id))
      edges = edges.filter(edge => semanticNodeIds.has(edge.source) && semanticNodeIds.has(edge.target))
    }
  }

  return { nodes, edges }
}

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

function modeVisibility(mode: GraphMode): { nodeTypes: Set<NodeType>; edgeTypes: Set<EdgeType> } {
  if (mode === 'Macro') {
    return {
      nodeTypes: new Set(['Module', 'File', 'Endpoint', 'ExternalCrate']),
      edgeTypes: new Set(['Contains', 'Imports', 'Uses', 'ApiCall', 'EndpointHandler', 'ModDeclaration', 'ExternalDependency']),
    }
  }
  if (mode === 'CallFlow') {
    return {
      nodeTypes: new Set(['Function', 'Method', 'Handler', 'Component', 'Hook', 'Endpoint']),
      edgeTypes: new Set(['Calls', 'Renders', 'ApiCall', 'EndpointHandler']),
    }
  }
  if (mode === 'DataFlow') {
    return {
      nodeTypes: new Set(['Function', 'Method', 'Component', 'Hook', 'Endpoint', 'Struct', 'Class', 'Object', 'Enum', 'Trait', 'Interface', 'TypeAlias', 'Property', 'Signal', 'Handler']),
      edgeTypes: new Set(['DataFlow', 'ApiCall', 'EndpointHandler', 'Calls', 'Contains']),
    }
  }
  if (mode === 'Traits') {
    return {
      nodeTypes: new Set(['Trait', 'Impl', 'Struct', 'Class', 'Object', 'Enum', 'Method']),
      edgeTypes: new Set(['Implements', 'Contains', 'TypeReference']),
    }
  }
  return {
    nodeTypes: new Set(['File', 'Module', 'Struct', 'Class', 'Object', 'Enum', 'Trait', 'Impl', 'Function', 'Method', 'Component', 'Hook', 'Interface', 'TypeAlias', 'Property', 'Signal', 'Handler', 'Endpoint', 'Macro']),
    edgeTypes: new Set(['Contains', 'Calls', 'Renders', 'ApiCall', 'EndpointHandler', 'TypeReference', 'Implements', 'Imports', 'Uses']),
  }
}
