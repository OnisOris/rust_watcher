import type {
  DiagnosticRecord,
  EdgeType,
  GraphEdge,
  GraphNode,
  GraphRegion,
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

const TOP_LEVEL_LAYOUT: Record<string, { label: string; language?: string; x: number; y: number; width: number; height: number; color: string }> = {
  'language:typescript': { label: 'TypeScript / JavaScript', language: 'typescript', x: -760, y: -260, width: 520, height: 360, color: REGION_COLORS.typescript },
  'language:qml': { label: 'QML', language: 'qml', x: -760, y: 170, width: 520, height: 320, color: REGION_COLORS.qml },
  'boundary:api': { label: 'API Boundary', x: -115, y: -260, width: 230, height: 720, color: REGION_COLORS.api },
  'language:rust': { label: 'Rust', language: 'rust', x: 260, y: -300, width: 560, height: 390, color: REGION_COLORS.rust },
  'language:python': { label: 'Python', language: 'python', x: 260, y: 170, width: 560, height: 330, color: REGION_COLORS.python },
  'external:external': { label: 'External', x: 920, y: -130, width: 330, height: 280, color: REGION_COLORS.external },
  'detached:detached': { label: 'Detached', x: 920, y: 230, width: 330, height: 240, color: REGION_COLORS.detached },
  'generated:generated': { label: 'Generated', x: 920, y: 530, width: 330, height: 220, color: REGION_COLORS.generated },
}

const ZERO_STATS: RegionStats = {
  fileCount: 0,
  symbolCount: 0,
  endpointCount: 0,
  diagnosticCount: 0,
  incomingEdgeCount: 0,
  outgoingEdgeCount: 0,
}

export function buildSemanticLayout(
  nodes: GraphNode[],
  edges: GraphEdge[],
  diagnosticsByNode: Map<string, DiagnosticRecord[]> = new Map(),
): SemanticLayoutResult {
  const assignments = assignRegions(nodes)
  const assignmentByNode = new Map(assignments.map(assignment => [assignment.nodeId, assignment.regionId]))
  const regions = buildRegions(nodes, edges, assignments, diagnosticsByNode)
  const regionById = new Map(regions.map(region => [region.id, region]))
  const positionedNodes = positionNodes(nodes, assignmentByNode, regionById)
  const routedEdges = routeSemanticEdges(edges, positionedNodes, assignmentByNode)

  return { nodes: positionedNodes, edges: routedEdges, regions, assignments }
}

export function assignRegions(nodes: GraphNode[]): LayoutRegionAssignment[] {
  return nodes.map(node => {
    if (node.reachability === 'Generated') return assignment(node, 'generated:generated', 'generated source')
    if (node.reachability === 'Detached') return assignment(node, 'detached:detached', 'detached source')
    if (node.type === 'Endpoint') return assignment(node, 'boundary:api', 'endpoint route boundary')
    if (node.type === 'ExternalCrate' || node.reachability === 'External' || node.crate === 'external') {
      return assignment(node, 'external:external', 'external dependency')
    }
    const language = normalizedLanguage(node)
    if (language === 'typescript' || language === 'javascript') return assignment(node, 'language:typescript', 'TypeScript/JavaScript language')
    if (language === 'qml') return assignment(node, 'language:qml', 'QML language')
    if (language === 'rust') return assignment(node, 'language:rust', 'Rust language')
    if (language === 'python') return assignment(node, 'language:python', 'Python language')
    return assignment(node, 'external:external', 'unknown or external language')
  })
}

export function packageRegionId(node: GraphNode, parentRegionId: string) {
  if (!node.file) return null
  if (!parentRegionId.startsWith('language:')) return null
  const packagePath = packagePathFor(node.file)
  if (!packagePath) return null
  return `${parentRegionId}:package:${packagePath}`
}

function buildRegions(
  nodes: GraphNode[],
  edges: GraphEdge[],
  assignments: LayoutRegionAssignment[],
  diagnosticsByNode: Map<string, DiagnosticRecord[]>,
) {
  const nodeById = new Map(nodes.map(node => [node.id, node]))
  const assignmentByNode = new Map(assignments.map(assignment => [assignment.nodeId, assignment.regionId]))
  const usedTopRegions = new Set(assignments.map(assignment => assignment.regionId))
  if (edges.some(edge => edge.type === 'ApiCall' || edge.type === 'EndpointHandler')) {
    usedTopRegions.add('boundary:api')
  }

  const regions = new Map<string, GraphRegion>()
  for (const id of usedTopRegions) {
    const template = TOP_LEVEL_LAYOUT[id] ?? TOP_LEVEL_LAYOUT['external:external']
    regions.set(id, {
      id,
      label: template.label,
      kind: regionKindFor(id),
      language: template.language,
      bounds: { x: template.x, y: template.y, width: template.width, height: template.height },
      colorToken: template.color,
      nodeIds: [],
      childRegionIds: [],
      stats: { ...ZERO_STATS },
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
    if (!regions.has(packageId)) {
      regions.set(packageId, packageRegion(packageId, node, region))
      region.childRegionIds.push(packageId)
    }
    const child = regions.get(packageId)!
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

  return [...regions.values()].sort((a, b) => a.id.localeCompare(b.id))
}

function positionNodes(nodes: GraphNode[], assignmentByNode: Map<string, string>, regionById: Map<string, GraphRegion>) {
  const grouped = new Map<string, GraphNode[]>()
  for (const node of nodes) {
    const regionId = assignmentByNode.get(node.id) ?? 'external:external'
    const group = grouped.get(regionId) ?? []
    group.push(node)
    grouped.set(regionId, group)
  }

  const next: GraphNode[] = []
  for (const [regionId, group] of grouped) {
    const region = regionById.get(regionId) ?? regionById.get('external:external')
    if (!region) continue
    const sorted = [...group].sort(nodeSort)
    const cols = Math.max(1, Math.ceil(Math.sqrt(sorted.length)))
    const cellW = Math.max(68, (region.bounds.width - 80) / cols)
    const rows = Math.max(1, Math.ceil(sorted.length / cols))
    const cellH = Math.max(62, (region.bounds.height - 90) / rows)
    sorted.forEach((node, index) => {
      if (node.pinned) {
        next.push(node)
        return
      }
      const col = index % cols
      const row = Math.floor(index / cols)
      const jitter = stableJitter(node.id)
      next.push({
        ...node,
        x: region.bounds.x + 45 + col * cellW + cellW * 0.5 + jitter.x,
        y: region.bounds.y + 62 + row * cellH + cellH * 0.5 + jitter.y,
        vx: 0,
        vy: 0,
      })
    })
  }
  return nodes.map(node => next.find(nextNode => nextNode.id === node.id) ?? node)
}

function routeSemanticEdges(edges: GraphEdge[], nodes: GraphNode[], assignmentByNode: Map<string, string>) {
  const nodeById = new Map(nodes.map(node => [node.id, node]))
  return edges.map(edge => {
    const source = nodeById.get(edge.source)
    const target = nodeById.get(edge.target)
    if (!source || !target) return edge
    const sourceRegion = assignmentByNode.get(edge.source)
    const targetRegion = assignmentByNode.get(edge.target)
    if (!sourceRegion || !targetRegion || sourceRegion === targetRegion) return { ...edge, routedPath: undefined }
    if (!isCrossZoneEdge(edge.type)) return { ...edge, routedPath: undefined }
    const laneX = edge.type === 'ApiCall' || edge.type === 'EndpointHandler' || sourceRegion === 'boundary:api' || targetRegion === 'boundary:api'
      ? 0
      : (source.x + target.x) / 2
    return {
      ...edge,
      routedPath: [
        { x: source.x, y: source.y },
        { x: laneX, y: source.y },
        { x: laneX, y: target.y },
        { x: target.x, y: target.y },
      ],
    }
  })
}

function assignment(node: GraphNode, regionId: string, reason: string): LayoutRegionAssignment {
  return { nodeId: node.id, regionId, reason }
}

function regionKindFor(id: string): GraphRegion['kind'] {
  if (id === 'boundary:api') return 'Boundary'
  if (id.startsWith('external:')) return 'External'
  if (id.startsWith('detached:')) return 'Detached'
  if (id.startsWith('generated:')) return 'Generated'
  return 'Language'
}

function packageRegion(id: string, node: GraphNode, parent: GraphRegion): GraphRegion {
  const index = parent.childRegionIds.length
  const cols = Math.max(1, Math.ceil(Math.sqrt(parent.childRegionIds.length + 1)))
  const width = Math.max(180, (parent.bounds.width - 50) / cols)
  const height = 120
  const col = index % cols
  const row = Math.floor(index / cols)
  return {
    id,
    label: packagePathFor(node.file ?? '') ?? parent.label,
    kind: 'Package',
    language: parent.language,
    bounds: {
      x: parent.bounds.x + 22 + col * width,
      y: parent.bounds.y + 58 + row * (height + 16),
      width: width - 12,
      height,
    },
    colorToken: parent.colorToken,
    nodeIds: [],
    childRegionIds: [],
    stats: { ...ZERO_STATS },
  }
}

function addNodeStats(stats: RegionStats, node: GraphNode, diagnostics: number) {
  if (node.type === 'File') stats.fileCount++
  else if (node.type === 'Endpoint') stats.endpointCount++
  else stats.symbolCount++
  stats.diagnosticCount += diagnostics
}

function normalizedLanguage(node: GraphNode) {
  return (node.language ?? '').toLowerCase()
}

function packagePathFor(file: string) {
  const parts = file.split('/').filter(Boolean)
  if (!parts.length) return null
  if (parts[0] === 'src' && parts.length >= 2) return parts.slice(0, Math.min(3, parts.length - 1)).join('/')
  return parts.slice(0, Math.min(2, Math.max(1, parts.length - 1))).join('/')
}

function nodeSort(a: GraphNode, b: GraphNode) {
  const typeWeight = (node: GraphNode) => node.type === 'Module' ? 0 : node.type === 'File' ? 1 : node.type === 'Endpoint' ? 2 : 3
  return typeWeight(a) - typeWeight(b)
    || (a.file ?? '').localeCompare(b.file ?? '')
    || a.label.localeCompare(b.label)
    || a.id.localeCompare(b.id)
}

function stableJitter(id: string) {
  let hash = 0
  for (let i = 0; i < id.length; i++) hash = ((hash << 5) - hash + id.charCodeAt(i)) | 0
  return {
    x: ((hash & 0xff) / 255 - 0.5) * 18,
    y: (((hash >> 8) & 0xff) / 255 - 0.5) * 14,
  }
}

function isCrossZoneEdge(type: EdgeType) {
  return type === 'ApiCall'
    || type === 'EndpointHandler'
    || type === 'DataFlow'
    || type === 'Imports'
    || type === 'Uses'
    || type === 'ExternalDependency'
}
