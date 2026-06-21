import type {
  DiagnosticRecord,
  EdgeType,
  GraphEdge,
  GraphMode,
  GraphNode,
  GraphRegion,
  GraphLayoutMode,
  LayoutRegionAssignment,
  RegionStats,
  SemanticLayoutResult,
} from '../types'

const REGION_COLORS: Record<string, string> = {
  typescript: '#14B8A6',
  qml: '#8B5CF6',
  rust: '#3B82F6',
  python: '#F97316',
  api: '#E11D48',
  external: '#7D8795',
  detached: '#94A3B8',
  generated: '#64748B',
}

const REGION_LABELS: Record<string, { label: string; language?: string; color: string }> = {
  'language:typescript': { label: 'TypeScript / JavaScript', language: 'typescript', color: REGION_COLORS.typescript },
  'language:qml': { label: 'QML', language: 'qml', color: REGION_COLORS.qml },
  'language:rust': { label: 'Rust', language: 'rust', color: REGION_COLORS.rust },
  'language:python': { label: 'Python', language: 'python', color: REGION_COLORS.python },
  'boundary:api': { label: 'API Boundary', color: REGION_COLORS.api },
  'external:external': { label: 'External', color: REGION_COLORS.external },
  'detached:detached': { label: 'Detached', color: REGION_COLORS.detached },
  'generated:generated': { label: 'Generated', color: REGION_COLORS.generated },
}

const REGION_ANCHORS: Record<string, { x: number; y: number; width: number; height: number }> = {
  'language:typescript': { x: -900, y: -360, width: 520, height: 360 },
  'language:qml': { x: -900, y: 150, width: 520, height: 330 },
  'boundary:api': { x: -125, y: -380, width: 250, height: 860 },
  'language:rust': { x: 360, y: -380, width: 620, height: 420 },
  'language:python': { x: 360, y: 160, width: 620, height: 360 },
  'external:external': { x: 1130, y: -250, width: 360, height: 300 },
  'detached:detached': { x: 1130, y: 170, width: 360, height: 250 },
  'generated:generated': { x: 1130, y: 500, width: 360, height: 230 },
}

const ZERO_STATS: RegionStats = {
  fileCount: 0,
  symbolCount: 0,
  endpointCount: 0,
  diagnosticCount: 0,
  incomingEdgeCount: 0,
  outgoingEdgeCount: 0,
}

const TOP_REGION_ORDER = [
  'language:typescript',
  'language:qml',
  'boundary:api',
  'language:rust',
  'language:python',
  'external:external',
  'detached:detached',
  'generated:generated',
]

export interface SemanticLayoutOptions {
  layoutMode?: GraphLayoutMode
  graphMode?: GraphMode
  selectedNodeId?: string | null
  diagnosticsByNode?: Map<string, DiagnosticRecord[]>
}

export function buildSemanticLayout(
  nodes: GraphNode[],
  edges: GraphEdge[],
  optionsOrDiagnostics: SemanticLayoutOptions | Map<string, DiagnosticRecord[]> = {},
): SemanticLayoutResult {
  const options = normalizeOptions(optionsOrDiagnostics)
  if (options.layoutMode === 'PackageMap') return buildPackageMapLayout(nodes, edges, options)
  if (options.layoutMode === 'Neighborhood') return buildNeighborhoodLayout(nodes, edges, options)
  return buildSemanticZonesLayout(nodes, edges, options)
}

export function buildSemanticZonesLayout(
  nodes: GraphNode[],
  edges: GraphEdge[],
  options: SemanticLayoutOptions = {},
): SemanticLayoutResult {
  const diagnosticsByNode = options.diagnosticsByNode ?? new Map()
  const assignments = assignRegions(nodes)
  const topAssignmentByNode = new Map(assignments.map(assignment => [assignment.nodeId, assignment.regionId]))
  const regions = buildRegions(nodes, edges, assignments, diagnosticsByNode)
  const regionById = new Map(regions.map(region => [region.id, region]))
  const packageAssignmentByNode = new Map<string, string>()
  for (const node of nodes) {
    const parentRegionId = topAssignmentByNode.get(node.id) ?? 'external:external'
    packageAssignmentByNode.set(node.id, packageRegionId(node, parentRegionId) ?? parentRegionId)
  }
  const positionedNodes = positionEndpointsInRouteRows(positionNodes(nodes, packageAssignmentByNode, regionById), edges, regionById)
  const routedEdges = routeSemanticEdges(edges, positionedNodes, topAssignmentByNode, regionById, options.graphMode)

  return { nodes: positionedNodes, edges: routedEdges, regions, assignments }
}

export function buildPackageMapLayout(
  nodes: GraphNode[],
  edges: GraphEdge[],
  options: SemanticLayoutOptions = {},
): SemanticLayoutResult {
  const zones = buildSemanticZonesLayout(nodes, edges, options)
  const assignmentByNode = new Map(zones.assignments.map(assignment => [assignment.nodeId, assignment.regionId]))
  const packageByNode = new Map<string, string>()
  const packageNodes = new Map<string, GraphNode>()
  const underlyingEdgesByPackageNode = new Map<string, Set<string>>()
  const packageRegions = zones.regions.filter(region => region.kind === 'Package')

  for (const region of packageRegions) {
    const firstNode = region.nodeIds.map(id => nodes.find(node => node.id === id)).find(Boolean)
    const packageNode: GraphNode = {
      id: `package:${region.id}`,
      language: region.language,
      type: 'Module',
      label: region.label,
      file: firstNode?.file,
      module: region.label,
      packagePath: region.label,
      regionId: region.id,
      underlyingNodeIds: [...region.nodeIds],
      underlyingEdgeIds: [],
      packageStats: packageStatsFromRegion(region, nodes, edges),
      description: `Package map group: ${region.nodeIds.length} nodes`,
      connections: region.nodeIds.length,
      x: region.bounds.x + region.bounds.width / 2,
      y: region.bounds.y + region.bounds.height / 2,
      vx: 0,
      vy: 0,
    }
    packageNodes.set(packageNode.id, packageNode)
    for (const nodeId of region.nodeIds) packageByNode.set(nodeId, packageNode.id)
  }

  for (const node of nodes) {
    if (node.type === 'Endpoint' || node.type === 'ExternalCrate') {
      packageNodes.set(node.id, zones.nodes.find(positioned => positioned.id === node.id) ?? node)
    } else if (!packageByNode.has(node.id)) {
      const regionId = assignmentByNode.get(node.id) ?? 'external:external'
      const region = zones.regions.find(item => item.id === regionId)
      const groupedNode: GraphNode = {
        ...node,
        id: `package:${regionId}:${node.id}`,
        type: node.type === 'File' ? 'File' : 'Module',
        label: region?.label ?? node.label,
        packagePath: region?.label ?? node.label,
        regionId,
        underlyingNodeIds: [node.id],
        underlyingEdgeIds: [],
        packageStats: {
          fileCount: node.type === 'File' ? 1 : 0,
          symbolCount: node.type === 'File' || node.type === 'Endpoint' ? 0 : 1,
          endpointCount: node.type === 'Endpoint' ? 1 : 0,
          diagnosticCount: options.diagnosticsByNode?.get(node.id)?.length ?? 0,
          exportedSymbolCount: isExportedNode(node) ? 1 : 0,
          incomingEdgeCount: edges.filter(edge => edge.target === node.id).length,
          outgoingEdgeCount: edges.filter(edge => edge.source === node.id).length,
        },
        description: `Package map singleton: ${node.label}`,
        connections: 1,
        x: region ? region.bounds.x + region.bounds.width / 2 : node.x,
        y: region ? region.bounds.y + region.bounds.height / 2 : node.y,
        vx: 0,
        vy: 0,
      }
      packageNodes.set(groupedNode.id, groupedNode)
      packageByNode.set(node.id, groupedNode.id)
    }
  }

  const packageEdges = new Map<string, GraphEdge>()
  for (const edge of edges) {
    const sourceNode = nodes.find(node => node.id === edge.source)
    const targetNode = nodes.find(node => node.id === edge.target)
    const source = sourceNode?.type === 'Endpoint'
      ? edge.source
      : packageByNode.get(edge.source) ?? edge.source
    const target = targetNode?.type === 'Endpoint'
      ? edge.target
      : packageByNode.get(edge.target) ?? edge.target
    if (source === target || !packageNodes.has(source) || !packageNodes.has(target)) continue
    const key = `${edge.type}:${source}->${target}`
    if (source !== edge.source) {
      const ids = underlyingEdgesByPackageNode.get(source) ?? new Set<string>()
      ids.add(edge.id)
      underlyingEdgesByPackageNode.set(source, ids)
    }
    if (target !== edge.target) {
      const ids = underlyingEdgesByPackageNode.get(target) ?? new Set<string>()
      ids.add(edge.id)
      underlyingEdgesByPackageNode.set(target, ids)
    }
    const existing = packageEdges.get(key)
    if (existing) {
      existing.bundledCount = (existing.bundledCount ?? 1) + 1
      existing.bundledEdgeIds = [...(existing.bundledEdgeIds ?? []), edge.id]
      existing.bundledTypes = [...new Set([...(existing.bundledTypes ?? [existing.type]), edge.type])]
    } else {
      packageEdges.set(key, {
        ...edge,
        id: key,
        source,
        target,
        bundledCount: 1,
        bundledEdgeIds: [edge.id],
      })
    }
  }

  const packageNodeList = [...packageNodes.values()].map(node => ({
    ...node,
    underlyingEdgeIds: [...(underlyingEdgesByPackageNode.get(node.id) ?? new Set(node.underlyingEdgeIds ?? []))],
  }))
  const assignments = assignRegions(packageNodeList)
  const assignmentByPackageNode = new Map(assignments.map(assignment => [assignment.nodeId, assignment.regionId]))
  const routedEdges = routeSemanticEdges([...packageEdges.values()], packageNodeList, assignmentByPackageNode, new Map(zones.regions.map(region => [region.id, region])), options.graphMode)

  return { nodes: packageNodeList, edges: routedEdges, regions: zones.regions, assignments }
}

export function buildNeighborhoodLayout(
  nodes: GraphNode[],
  edges: GraphEdge[],
  options: SemanticLayoutOptions = {},
): SemanticLayoutResult {
  if (!options.selectedNodeId || !nodes.some(node => node.id === options.selectedNodeId)) {
    const guide: GraphNode = {
      id: 'layout-guide:local-neighborhood',
      type: 'Module',
      label: 'Select a node',
      description: 'Select a node to open local neighborhood',
      layoutGuide: 'Select a node to open local neighborhood',
      x: 0,
      y: 0,
      vx: 0,
      vy: 0,
    }
    const region: GraphRegion = {
      id: 'neighborhood:guide',
      label: 'Local Neighborhood',
      kind: 'Layer',
      bounds: { x: -240, y: -140, width: 480, height: 280 },
      colorToken: '#0EA5E9',
      nodeIds: [guide.id],
      childRegionIds: [],
      stats: { ...ZERO_STATS, symbolCount: 1 },
    }
    return { nodes: [guide], edges: [], regions: [region], assignments: [assignment(guide, region.id, 'no node selected')] }
  }
  if (!nodes.length) return buildSemanticZonesLayout(nodes, edges, options)
  const selectedId = options.selectedNodeId

  const importantEdges = edges.filter(edge => edge.source === selectedId || edge.target === selectedId || isTraceLikeEdge(edge.type))
  const visibleIds = new Set<string>([selectedId])
  for (const edge of importantEdges) {
    if (edge.source === selectedId || edge.target === selectedId) {
      visibleIds.add(edge.source)
      visibleIds.add(edge.target)
    }
  }
  for (const edge of edges) {
    if (visibleIds.has(edge.source) || visibleIds.has(edge.target)) {
      if (edge.type === 'ApiCall' || edge.type === 'EndpointHandler' || edge.type === 'DataFlow' || edge.type === 'Calls') {
        visibleIds.add(edge.source)
        visibleIds.add(edge.target)
      }
    }
  }

  const visibleNodes = nodes.filter(node => visibleIds.has(node.id))
  const visibleEdges = edges.filter(edge => visibleIds.has(edge.source) && visibleIds.has(edge.target))
  const zones = buildSemanticZonesLayout(visibleNodes, visibleEdges, options)
  const incoming = visibleEdges.filter(edge => edge.target === selectedId).map(edge => edge.source)
  const outgoing = visibleEdges.filter(edge => edge.source === selectedId).map(edge => edge.target)
  const api = visibleEdges.filter(edge => edge.type === 'ApiCall' || edge.type === 'EndpointHandler').flatMap(edge => [edge.source, edge.target])
  const centered = zones.nodes.map(node => {
    if (node.id === selectedId) return { ...node, x: 0, y: 0, vx: 0, vy: 0 }
    const index = zones.nodes.findIndex(item => item.id === node.id)
    if (api.includes(node.id)) return { ...node, x: 0, y: -220 + index * 38, vx: 0, vy: 0 }
    if (incoming.includes(node.id)) return { ...node, x: -310, y: -150 + incoming.indexOf(node.id) * 86, vx: 0, vy: 0 }
    if (outgoing.includes(node.id)) return { ...node, x: 310, y: -150 + outgoing.indexOf(node.id) * 86, vx: 0, vy: 0 }
    const jitter = stableJitter(node.id)
    return { ...node, x: jitter.x * 8, y: 250 + index * 40 + jitter.y, vx: 0, vy: 0 }
  })
  const regions: GraphRegion[] = [{
    id: 'neighborhood:selected',
    label: 'Local Neighborhood',
    kind: 'Layer',
    bounds: { x: -440, y: -300, width: 880, height: 650 },
    colorToken: '#0EA5E9',
    nodeIds: centered.map(node => node.id),
    childRegionIds: [],
    stats: regionStatsFor(centered, visibleEdges, options.diagnosticsByNode ?? new Map()),
  }]
  const assignmentByNode = new Map(centered.map(node => [node.id, 'neighborhood:selected']))
  const routedEdges = routeSemanticEdges(visibleEdges, centered, assignmentByNode, new Map(regions.map(region => [region.id, region])), options.graphMode)
  return { nodes: centered, edges: routedEdges, regions, assignments: centered.map(node => assignment(node, 'neighborhood:selected', node.id === selectedId ? 'selected node neighborhood' : 'connected to selected node')) }
}

export function assignRegions(nodes: GraphNode[]): LayoutRegionAssignment[] {
  return nodes.map(node => {
    if (node.reachability === 'Generated') return assignment(node, 'generated:generated', 'generated source')
    if (node.reachability === 'Detached') return assignment(node, 'detached:detached', 'detached source')
    if (node.type === 'Endpoint') return assignment(node, 'boundary:api', 'endpoint route boundary')
    if (node.type === 'ExternalCrate' || node.reachability === 'External' || node.crate === 'external') {
      return assignment(node, 'external:external', 'external dependency')
    }
    const language = inferNodeLanguage(node)
    if (language === 'typescript' || language === 'javascript') return assignment(node, 'language:typescript', languageReason(node, 'TypeScript/JavaScript'))
    if (language === 'qml') return assignment(node, 'language:qml', languageReason(node, 'QML'))
    if (language === 'rust') return assignment(node, 'language:rust', languageReason(node, 'Rust'))
    if (language === 'python') return assignment(node, 'language:python', languageReason(node, 'Python'))
    return assignment(node, 'external:external', 'unknown or external language')
  })
}

export function inferNodeLanguage(node: GraphNode) {
  const explicit = (node.language ?? '').toLowerCase()
  if (explicit === 'rust' || explicit === 'python' || explicit === 'qml' || explicit === 'typescript' || explicit === 'javascript') return explicit
  const file = (node.file ?? '').toLowerCase()
  if (file.endsWith('.rs')) return 'rust'
  if (file.endsWith('.py')) return 'python'
  if (file.endsWith('.qml')) return 'qml'
  if (file.endsWith('.ts') || file.endsWith('.tsx')) return 'typescript'
  if (file.endsWith('.js') || file.endsWith('.jsx')) return 'javascript'
  return explicit || 'unknown'
}

export function semanticNodeSubtitle(node: GraphNode) {
  const language = languageLabel(inferNodeLanguage(node))
  return `${language} ${node.type}`.trim()
}

export function semanticNodeDetail(node: GraphNode) {
  if (node.type === 'Endpoint') return node.description ?? node.label
  return node.file ?? node.module ?? node.crate ?? ''
}

export function packageRegionId(node: GraphNode, parentRegionId: string) {
  if (!node.file) return null
  if (!parentRegionId.startsWith('language:')) return null
  const packagePath = packagePathFor(node.file)
  if (!packagePath) return null
  return `${parentRegionId}:package:${packagePath}`
}

export function assignedSemanticRegionId(node: GraphNode) {
  const topRegionId = assignRegions([node])[0]?.regionId ?? 'external:external'
  return packageRegionId(node, topRegionId) ?? topRegionId
}

export function clampPointToRegion(x: number, y: number, region: GraphRegion) {
  const pad = region.kind === 'Package' ? 26 : 38
  return {
    x: clamp(x, region.bounds.x + pad, region.bounds.x + region.bounds.width - pad),
    y: clamp(y, region.bounds.y + pad + 22, region.bounds.y + region.bounds.height - pad),
  }
}

export function regionLabel(regionId: string) {
  if (regionId.includes(':package:')) return regionId.split(':package:')[1] ?? regionId
  return REGION_LABELS[regionId]?.label ?? regionId
}

function buildRegions(
  nodes: GraphNode[],
  edges: GraphEdge[],
  assignments: LayoutRegionAssignment[],
  diagnosticsByNode: Map<string, DiagnosticRecord[]>,
) {
  const nodeById = new Map(nodes.map(node => [node.id, node]))
  const assignmentByNode = new Map(assignments.map(assignment => [assignment.nodeId, assignment.regionId]))
  const packagesByParent = new Map<string, Set<string>>()
  const usedTopRegions = new Set(assignments.map(assignment => assignment.regionId))
  if (edges.some(edge => edge.type === 'ApiCall' || edge.type === 'EndpointHandler')) usedTopRegions.add('boundary:api')

  for (const assignment of assignments) {
    const node = nodeById.get(assignment.nodeId)
    if (!node) continue
    const packageId = packageRegionId(node, assignment.regionId)
    if (!packageId) continue
    const packages = packagesByParent.get(assignment.regionId) ?? new Set<string>()
    packages.add(packageId)
    packagesByParent.set(assignment.regionId, packages)
  }

  const topNodeCounts = new Map<string, number>()
  for (const assignment of assignments) {
    topNodeCounts.set(assignment.regionId, (topNodeCounts.get(assignment.regionId) ?? 0) + 1)
  }

  const regions = new Map<string, GraphRegion>()
  for (const id of TOP_REGION_ORDER.filter(id => usedTopRegions.has(id))) {
    const meta = REGION_LABELS[id] ?? REGION_LABELS['external:external']
    const bounds = dynamicTopBounds(id, topNodeCounts.get(id) ?? 0, packagesByParent.get(id)?.size ?? 0)
    regions.set(id, {
      id,
      label: meta.label,
      kind: regionKindFor(id),
      language: meta.language,
      bounds,
      colorToken: meta.color,
      nodeIds: [],
      childRegionIds: [],
      stats: { ...ZERO_STATS },
    })
  }

  for (const [parentId, packageIds] of packagesByParent) {
    const parent = regions.get(parentId)
    if (!parent) continue
    const sorted = [...packageIds].sort()
    parent.childRegionIds = sorted
    sorted.forEach((packageId, index) => {
      regions.set(packageId, packageRegion(packageId, parent, index, sorted.length))
    })
  }

  for (const assignment of assignments) {
    const node = nodeById.get(assignment.nodeId)
    const region = regions.get(assignment.regionId)
    if (!node || !region) continue
    region.nodeIds.push(node.id)
    addNodeStats(region.stats, node, diagnosticsByNode.get(node.id)?.length ?? 0)
    const packageId = packageRegionId(node, assignment.regionId)
    if (!packageId) continue
    const child = regions.get(packageId)
    if (!child) continue
    child.nodeIds.push(node.id)
    addNodeStats(child.stats, node, diagnosticsByNode.get(node.id)?.length ?? 0)
  }

  for (const edge of edges) {
    const sourceRegion = assignmentByNode.get(edge.source)
    const targetRegion = assignmentByNode.get(edge.target)
    if (!sourceRegion || !targetRegion || sourceRegion === targetRegion) continue
    regions.get(sourceRegion)!.stats.outgoingEdgeCount++
    regions.get(targetRegion)!.stats.incomingEdgeCount++
  }

  return [...regions.values()].sort((a, b) => regionSortKey(a.id).localeCompare(regionSortKey(b.id)))
}

function positionNodes(nodes: GraphNode[], assignmentByNode: Map<string, string>, regionById: Map<string, GraphRegion>) {
  const grouped = new Map<string, GraphNode[]>()
  for (const node of nodes) {
    const regionId = assignmentByNode.get(node.id) ?? 'external:external'
    const group = grouped.get(regionId) ?? []
    group.push(node)
    grouped.set(regionId, group)
  }

  const next = new Map<string, GraphNode>()
  for (const [regionId, group] of grouped) {
    const region = regionById.get(regionId) ?? regionById.get('external:external')
    if (!region) continue
    const sorted = [...group].sort(nodeSort)
    const cols = Math.max(1, Math.ceil(Math.sqrt(sorted.length * Math.max(1, region.bounds.width / Math.max(1, region.bounds.height)))))
    const rows = Math.max(1, Math.ceil(sorted.length / cols))
    const cellW = Math.max(58, (region.bounds.width - 52) / cols)
    const cellH = Math.max(52, (region.bounds.height - 68) / rows)
    sorted.forEach((node, index) => {
      if (node.pinned) {
        const clamped = clampPointToRegion(node.x, node.y, region)
        next.set(node.id, { ...node, ...clamped, vx: 0, vy: 0 })
        return
      }
      const col = index % cols
      const row = Math.floor(index / cols)
      const jitter = stableJitter(node.id)
      next.set(node.id, {
        ...node,
        x: region.bounds.x + 26 + col * cellW + cellW * 0.5 + jitter.x,
        y: region.bounds.y + 48 + row * cellH + cellH * 0.5 + jitter.y,
        vx: 0,
        vy: 0,
      })
    })
  }
  return nodes.map(node => next.get(node.id) ?? node)
}

function positionEndpointsInRouteRows(nodes: GraphNode[], edges: GraphEdge[], regionById: Map<string, GraphRegion>) {
  const apiRegion = regionById.get('boundary:api')
  if (!apiRegion) return nodes
  const endpointIds = new Set(nodes.filter(node => node.type === 'Endpoint').map(node => node.id))
  if (!endpointIds.size) return nodes
  const endpointRows = routeRowsForEndpoints(nodes.filter(node => endpointIds.has(node.id)))
  return nodes.map(node => {
    const row = endpointRows.get(node.id)
    if (!row || node.pinned) return node.pinned ? { ...node, ...clampPointToRegion(node.x, node.y, apiRegion), vx: 0, vy: 0 } : node
    return {
      ...node,
      x: apiRegion.bounds.x + apiRegion.bounds.width / 2,
      y: row.y,
      vx: 0,
      vy: 0,
    }
  })
}

function routeSemanticEdges(
  edges: GraphEdge[],
  nodes: GraphNode[],
  assignmentByNode: Map<string, string>,
  regionById: Map<string, GraphRegion>,
  graphMode?: GraphMode,
) {
  const nodeById = new Map(nodes.map(node => [node.id, node]))
  const apiLaneByKey = buildApiLaneMap(edges, nodes, regionById)
  return edges.map(edge => {
    const source = nodeById.get(edge.source)
    const target = nodeById.get(edge.target)
    if (!source || !target) return edge
    const sourceRegion = assignmentByNode.get(edge.source)
    const targetRegion = assignmentByNode.get(edge.target)
    if (!sourceRegion || !targetRegion || sourceRegion === targetRegion) return { ...edge, routedPath: undefined }
    if (!isCrossZoneEdge(edge.type, graphMode)) return { ...edge, routedPath: undefined }
    const apiLane = apiLaneByKey.get(apiLaneKey(edge, source, target))
    const laneX = apiLane?.x ?? (source.x + target.x) / 2
    const laneY = apiLane?.y
    const midA = { x: laneX, y: source.y }
    const midB = laneY === undefined ? { x: laneX, y: target.y } : { x: laneX, y: laneY }
    const midC = laneY === undefined ? null : { x: laneX, y: target.y }
    return {
      ...edge,
      routedPath: midC
        ? [{ x: source.x, y: source.y }, midA, midB, midC, { x: target.x, y: target.y }]
        : [{ x: source.x, y: source.y }, midA, midB, { x: target.x, y: target.y }],
    }
  })
}

function buildApiLaneMap(edges: GraphEdge[], nodes: GraphNode[], regionById: Map<string, GraphRegion>) {
  const nodeById = new Map(nodes.map(node => [node.id, node]))
  const apiRegion = regionById.get('boundary:api')
  const centerX = apiRegion ? apiRegion.bounds.x + apiRegion.bounds.width / 2 : 0
  const topY = apiRegion ? apiRegion.bounds.y + 78 : -280
  const lanes = new Map<string, { x: number; y: number }>()
  const endpointRows = routeRowsForEndpoints(nodes.filter(node => node.type === 'Endpoint'))
  const keys = new Set<string>()
  for (const edge of edges) {
    const source = nodeById.get(edge.source)
    const target = nodeById.get(edge.target)
    if (!source || !target) continue
    if (edge.type === 'ApiCall' || edge.type === 'EndpointHandler' || edge.type === 'DataFlow') {
      if (source.type === 'Endpoint' || target.type === 'Endpoint' || edge.label?.startsWith('/api') || edge.description?.includes('/api')) {
        keys.add(apiLaneKey(edge, source, target))
      }
    }
  }
  ;[...keys].sort().forEach((key, index) => {
    const [endpointId, edgeType] = key.split('::')
    const row = endpointRows.get(endpointId)
    const laneOffset = edgeType === 'ApiCall' ? -82 : edgeType === 'EndpointHandler' ? 82 : (index % 5 - 2) * 24
    const returnOffset = edgeType === 'DataFlow' ? 18 : 0
    lanes.set(key, { x: centerX + laneOffset, y: row ? row.y + returnOffset : topY + index * 48 })
  })
  return lanes
}

function apiLaneKey(edge: GraphEdge, source: GraphNode, target: GraphNode) {
  const endpoint = source.type === 'Endpoint' ? source : target.type === 'Endpoint' ? target : undefined
  return `${endpoint?.id ?? endpoint?.label ?? edge.label ?? edge.description ?? edge.id}::${edge.type}`
}

function assignment(node: GraphNode, regionId: string, reason: string): LayoutRegionAssignment {
  return { nodeId: node.id, regionId, reason }
}

function normalizeOptions(optionsOrDiagnostics: SemanticLayoutOptions | Map<string, DiagnosticRecord[]>): SemanticLayoutOptions {
  if (optionsOrDiagnostics instanceof Map) return { diagnosticsByNode: optionsOrDiagnostics, layoutMode: 'SemanticZones' }
  return { layoutMode: 'SemanticZones', ...optionsOrDiagnostics }
}

function regionKindFor(id: string): GraphRegion['kind'] {
  if (id === 'boundary:api') return 'Boundary'
  if (id.startsWith('external:')) return 'External'
  if (id.startsWith('detached:')) return 'Detached'
  if (id.startsWith('generated:')) return 'Generated'
  return 'Language'
}

function packageRegion(id: string, parent: GraphRegion, index: number, total: number): GraphRegion {
  const label = id.split(':package:')[1] ?? parent.label
  const cols = Math.max(1, Math.ceil(Math.sqrt(total * Math.max(1, parent.bounds.width / Math.max(1, parent.bounds.height)))))
  const rows = Math.max(1, Math.ceil(total / cols))
  const gap = 18
  const innerX = parent.bounds.x + 24
  const innerY = parent.bounds.y + 58
  const innerW = parent.bounds.width - 48
  const innerH = parent.bounds.height - 82
  const width = Math.max(180, (innerW - gap * (cols - 1)) / cols)
  const height = Math.max(120, (innerH - gap * (rows - 1)) / rows)
  const col = index % cols
  const row = Math.floor(index / cols)
  return {
    id,
    label,
    kind: 'Package',
    language: parent.language,
    bounds: {
      x: innerX + col * (width + gap),
      y: innerY + row * (height + gap),
      width,
      height,
    },
    colorToken: parent.colorToken,
    nodeIds: [],
    childRegionIds: [],
    stats: { ...ZERO_STATS },
  }
}

function dynamicTopBounds(regionId: string, nodeCount: number, packageCount: number) {
  const anchor = REGION_ANCHORS[regionId] ?? REGION_ANCHORS['external:external']
  const density = Math.max(nodeCount, packageCount * 6)
  const width = anchor.width + Math.min(360, Math.max(0, Math.ceil(Math.sqrt(density)) - 4) * 42)
  const height = anchor.height + Math.min(420, Math.max(0, Math.ceil(density / 18) - 1) * 62)
  return { ...anchor, width, height }
}

function routeRowsForEndpoints(endpoints: GraphNode[]) {
  const rows = new Map<string, { y: number; routeKey: string }>()
  const sorted = [...endpoints].sort((a, b) => routeKeyForEndpoint(a).localeCompare(routeKeyForEndpoint(b)))
  sorted.forEach((endpoint, index) => {
    rows.set(endpoint.id, { y: -310 + index * 76, routeKey: routeKeyForEndpoint(endpoint) })
  })
  return rows
}

function routeKeyForEndpoint(node: GraphNode) {
  return node.description ?? node.label ?? node.id
}

function packageStatsFromRegion(region: GraphRegion, nodes: GraphNode[], edges: GraphEdge[]) {
  const nodeIds = new Set(region.nodeIds)
  const packageNodes = nodes.filter(node => nodeIds.has(node.id))
  const incoming = edges.filter(edge => !nodeIds.has(edge.source) && nodeIds.has(edge.target))
  const outgoing = edges.filter(edge => nodeIds.has(edge.source) && !nodeIds.has(edge.target))
  return {
    fileCount: region.stats.fileCount,
    symbolCount: region.stats.symbolCount,
    endpointCount: region.stats.endpointCount,
    diagnosticCount: region.stats.diagnosticCount,
    exportedSymbolCount: packageNodes.filter(isExportedNode).length,
    incomingEdgeCount: incoming.length,
    outgoingEdgeCount: outgoing.length,
  }
}

function isExportedNode(node: GraphNode) {
  return node.visibility === 'pub'
    || node.visibility === 'pub(crate)'
    || node.type === 'Endpoint'
    || /^[A-Z]/.test(node.label)
}

function regionStatsFor(nodes: GraphNode[], edges: GraphEdge[], diagnosticsByNode: Map<string, DiagnosticRecord[]>): RegionStats {
  const stats = { ...ZERO_STATS }
  for (const node of nodes) addNodeStats(stats, node, diagnosticsByNode.get(node.id)?.length ?? 0)
  stats.incomingEdgeCount = edges.length
  stats.outgoingEdgeCount = edges.length
  return stats
}

function addNodeStats(stats: RegionStats, node: GraphNode, diagnostics: number) {
  if (node.type === 'File') stats.fileCount++
  else if (node.type === 'Endpoint') stats.endpointCount++
  else stats.symbolCount++
  stats.diagnosticCount += diagnostics
}

function languageReason(node: GraphNode, label: string) {
  const explicit = (node.language ?? '').toLowerCase()
  const file = node.file ?? ''
  const source = explicit ? 'node metadata' : file ? `file extension (${file.split('.').pop()})` : 'fallback inference'
  const ambiguousLabel = node.label.toLowerCase() === 'qml' && label !== 'QML' ? ', label is a symbol name, not QML language' : ''
  return `${label} language from ${source}${ambiguousLabel}`
}

function languageLabel(language: string) {
  if (language === 'typescript') return 'TypeScript'
  if (language === 'javascript') return 'JavaScript'
  if (language === 'qml') return 'QML'
  if (language === 'rust') return 'Rust'
  if (language === 'python') return 'Python'
  return 'Unknown'
}

function packagePathFor(file: string) {
  const parts = file.split('/').filter(Boolean)
  if (!parts.length) return null
  const fileName = parts.at(-1) ?? ''
  if (parts.length === 1) return fileName.replace(/\.[^.]+$/, '')
  if (parts[0] === 'src' && parts.length >= 2) return parts.slice(0, Math.min(3, parts.length - 1)).join('/')
  if (parts[0] === 'qml' && parts.length >= 2) return parts.slice(0, Math.min(3, parts.length - 1)).join('/')
  if (parts[0] === 'frontend' && parts.length >= 3) return parts.slice(0, Math.min(4, parts.length - 1)).join('/')
  return parts.slice(0, Math.min(2, Math.max(1, parts.length - 1))).join('/')
}

function nodeSort(a: GraphNode, b: GraphNode) {
  const typeWeight = (node: GraphNode) => node.type === 'Module' ? 0 : node.type === 'File' ? 1 : node.type === 'Endpoint' ? 2 : node.type === 'Class' ? 3 : 4
  return typeWeight(a) - typeWeight(b)
    || (a.file ?? '').localeCompare(b.file ?? '')
    || a.label.localeCompare(b.label)
    || a.id.localeCompare(b.id)
}

function strongestNode(nodes: GraphNode[], edges: GraphEdge[]) {
  const degree = new Map<string, number>()
  for (const edge of edges) {
    degree.set(edge.source, (degree.get(edge.source) ?? 0) + 1)
    degree.set(edge.target, (degree.get(edge.target) ?? 0) + 1)
  }
  return [...nodes].sort((a, b) => (degree.get(b.id) ?? 0) - (degree.get(a.id) ?? 0))[0]
}

function stableJitter(id: string) {
  let hash = 0
  for (let i = 0; i < id.length; i++) hash = ((hash << 5) - hash + id.charCodeAt(i)) | 0
  return {
    x: ((hash & 0xff) / 255 - 0.5) * 18,
    y: (((hash >> 8) & 0xff) / 255 - 0.5) * 14,
  }
}

function clamp(value: number, min: number, max: number) {
  if (max < min) return (min + max) / 2
  return Math.min(max, Math.max(min, value))
}

function isCrossZoneEdge(type: EdgeType, graphMode?: GraphMode) {
  if (graphMode === 'DataFlow') return type === 'DataFlow' || type === 'ApiCall' || type === 'EndpointHandler' || type === 'Calls'
  if (graphMode === 'CallFlow') return type === 'Calls' || type === 'ApiCall' || type === 'EndpointHandler'
  return type === 'ApiCall'
    || type === 'EndpointHandler'
    || type === 'DataFlow'
    || type === 'Imports'
    || type === 'Uses'
    || type === 'ExternalDependency'
}

function isTraceLikeEdge(type: EdgeType) {
  return type === 'ApiCall' || type === 'EndpointHandler' || type === 'DataFlow' || type === 'Calls'
}

function regionSortKey(id: string) {
  const top = TOP_REGION_ORDER.findIndex(regionId => id === regionId || id.startsWith(`${regionId}:`))
  return `${top < 0 ? 99 : top}:${id}`
}
