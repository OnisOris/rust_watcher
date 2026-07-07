import type { EdgeType, GraphEdge, GraphNode } from '../types'
import type { HotspotConfidence, HotspotIssue, HotspotSeverity } from '../views/architecture/architectureTypes'

interface HotspotOptions {
  includeLowConfidence?: boolean
}

const SEVERITY_RANK: Record<HotspotSeverity, number> = {
  critical: 0,
  warning: 1,
  info: 2,
  noise: 3,
}

const CONFIDENCE_RANK: Record<HotspotConfidence, number> = {
  high: 0,
  medium: 1,
  low: 2,
}

const CYCLE_EDGE_TYPES = new Set<EdgeType>(['Imports', 'Uses', 'Calls', 'ApiCall', 'ExternalDependency'])
const LOW_CONFIDENCE_MUTUAL_TYPES = new Set<EdgeType>(['TypeReference', 'DataFlow'])
const BOUNDARY_EDGE_TYPES = new Set<EdgeType>(['Imports', 'Uses', 'Calls', 'ApiCall', 'ExternalDependency'])

export function buildHotspotIssues(nodes: GraphNode[], edges: GraphEdge[], options: HotspotOptions = {}): HotspotIssue[] {
  const includeLowConfidence = options.includeLowConfidence ?? false
  const incoming = new Map<string, GraphEdge[]>()
  const outgoing = new Map<string, GraphEdge[]>()
  const byId = new Map(nodes.map(node => [node.id, node]))

  for (const edge of edges) {
    incoming.set(edge.target, [...(incoming.get(edge.target) ?? []), edge])
    outgoing.set(edge.source, [...(outgoing.get(edge.source) ?? []), edge])
  }

  const degreeByNode = new Map<string, number>()
  for (const node of nodes) {
    degreeByNode.set(node.id, weightedCount(incoming.get(node.id) ?? []) + weightedCount(outgoing.get(node.id) ?? []))
  }
  const degrees = [...degreeByNode.values()].sort((a, b) => a - b)
  const averageDegree = degrees.length ? degrees.reduce((sum, degree) => sum + degree, 0) / degrees.length : 0
  const p95Degree = percentile(degrees, 0.95)

  const issues: HotspotIssue[] = []
  issues.push(...detectNodeHotspots(nodes, incoming, outgoing, degreeByNode, averageDegree, p95Degree))
  issues.push(...detectEndpointIssues(nodes, edges, incoming, outgoing, byId))
  issues.push(...detectBoundaryViolations(edges, byId))
  issues.push(...detectGroupCycles(edges, byId))
  if (includeLowConfidence) {
    issues.push(...detectLowConfidenceMutualReferences(edges, byId))
  }

  return dedupeIssues(issues)
    .filter(issue => includeLowConfidence || issue.confidence !== 'low')
    .sort((a, b) =>
      SEVERITY_RANK[a.severity] - SEVERITY_RANK[b.severity]
      || CONFIDENCE_RANK[a.confidence ?? 'medium'] - CONFIDENCE_RANK[b.confidence ?? 'medium']
      || b.connections - a.connections
      || a.title.localeCompare(b.title)
    )
}

function detectNodeHotspots(
  nodes: GraphNode[],
  incoming: Map<string, GraphEdge[]>,
  outgoing: Map<string, GraphEdge[]>,
  degreeByNode: Map<string, number>,
  averageDegree: number,
  p95Degree: number,
) {
  const issues: HotspotIssue[] = []
  const noiseBuckets = new Map<string, { nodes: GraphNode[]; degree: number; edgeIds: string[] }>()

  for (const node of nodes) {
    const inEdges = incoming.get(node.id) ?? []
    const outEdges = outgoing.get(node.id) ?? []
    const degree = degreeByNode.get(node.id) ?? 0
    const label = node.label.toLowerCase()
    const file = normalizePath(node.file)

    if ((node.type === 'File' || node.type === 'Module') && isGodModuleDegree(degree, averageDegree, p95Degree)) {
      issues.push({
        id: `god-module:${node.id}`,
        nodeId: node.id,
        edgeIds: [...inEdges, ...outEdges].map(edge => edge.id),
        title: `${node.label} has unusually high coupling`,
        description: `${degree} relationships make this ${node.type.toLowerCase()} central compared with the rest of the graph.`,
        severity: degree > Math.max(140, averageDegree * 8) ? 'critical' : 'warning',
        kind: 'god-module',
        confidence: 'medium',
        connections: degree,
        files: node.file ? [node.file] : [],
        modules: node.module ? [node.module] : [],
        suggestion: 'Split responsibilities or inspect its local neighborhood before editing.',
      })
    }

    if (isNoiseUtility(label, file) && degree > 20) {
      const key = noiseBucketKey(label, file)
      const bucket = noiseBuckets.get(key) ?? { nodes: [], degree: 0, edgeIds: [] }
      bucket.nodes.push(node)
      bucket.degree += degree
      bucket.edgeIds.push(...[...inEdges, ...outEdges].map(edge => edge.id))
      noiseBuckets.set(key, bucket)
    }

    if (isCentralApiClient(node, label, file, inEdges, outEdges)) {
      issues.push({
        id: `central-api-client:${node.id}`,
        nodeId: node.id,
        edgeIds: [...inEdges, ...outEdges].map(edge => edge.id),
        title: `${node.label} is a central API client`,
        description: 'Many API, endpoint or data-flow relationships pass through this node.',
        severity: degree > Math.max(80, averageDegree * 5) ? 'warning' : 'info',
        kind: 'central-api-client',
        confidence: 'medium',
        connections: degree,
        files: node.file ? [node.file] : [],
        modules: node.module ? [node.module] : [],
        suggestion: 'Use API/Data Flow to inspect unresolved endpoints and high-traffic calls.',
      })
    }
  }

  for (const [key, bucket] of noiseBuckets) {
    const primary = bucket.nodes[0]
    const files = uniqueStrings(bucket.nodes.map(node => node.file).filter(Boolean) as string[])
    const modules = uniqueStrings(bucket.nodes.map(node => node.module).filter(Boolean) as string[])
    issues.push({
      id: `noise-utility:${key}`,
      nodeId: primary?.id,
      edgeIds: uniqueStrings(bucket.edgeIds),
      title: `${primary?.label ?? key} looks like graph noise`,
      description: `${bucket.nodes.length} utility node${bucket.nodes.length === 1 ? '' : 's'} create ${bucket.degree} relationships and can obscure architecture edges.`,
      severity: 'noise',
      kind: 'noise-utility',
      confidence: 'medium',
      connections: bucket.degree,
      files,
      modules,
      suggestion: 'Hide as noise by default or collapse into a utility group.',
    })
  }

  return issues
}

function detectEndpointIssues(
  nodes: GraphNode[],
  edges: GraphEdge[],
  incoming: Map<string, GraphEdge[]>,
  outgoing: Map<string, GraphEdge[]>,
  byId: Map<string, GraphNode>,
) {
  const issues: HotspotIssue[] = []
  const endpointHandlerTargets = new Set(edges.filter(edge => edge.type === 'EndpointHandler').flatMap(edge => [edge.source, edge.target]))

  for (const node of nodes) {
    const inEdges = incoming.get(node.id) ?? []
    const outEdges = outgoing.get(node.id) ?? []
    const degree = weightedCount(inEdges) + weightedCount(outEdges)
    if (node.type === 'Endpoint' && !inEdges.some(edge => edge.type === 'ApiCall')) {
      issues.push({
        id: `unused-endpoint:${node.id}`,
        nodeId: node.id,
        edgeIds: [...inEdges, ...outEdges].map(edge => edge.id),
        title: `${node.label} has no frontend caller`,
        description: 'No incoming ApiCall edge points to this endpoint in the current graph.',
        severity: 'info',
        kind: 'unused-endpoint',
        confidence: 'medium',
        connections: degree,
        files: node.file ? [node.file] : [],
        modules: node.module ? [node.module] : [],
        suggestion: 'Remove it, document it as internal, or add the missing caller edge.',
      })
    }
  }

  for (const edge of edges.filter(edge => edge.type === 'ApiCall')) {
    const target = byId.get(edge.target)
    const source = byId.get(edge.source)
    if (target?.type === 'Endpoint' && !endpointHandlerTargets.has(target.id)) {
      issues.push({
        id: `frontend-call-without-handler:${edge.id}`,
        nodeId: edge.source,
        edgeIds: [edge.id],
        title: `${target.label} has no handler`,
        description: 'A frontend API call reaches an endpoint without a matched EndpointHandler edge.',
        severity: 'warning',
        kind: 'frontend-call-without-handler',
        confidence: 'high',
        connections: 1,
        files: uniqueStrings([source?.file, target.file].filter(Boolean) as string[]),
        modules: uniqueStrings([source?.module, target.module].filter(Boolean) as string[]),
        suggestion: 'Open API/Data Flow to verify the route mapping or add the missing backend handler edge.',
      })
    }
  }

  return issues
}

function detectGroupCycles(edges: GraphEdge[], byId: Map<string, GraphNode>) {
  const directional = new Map<string, { source: DependencyGroup; target: DependencyGroup; edgeIds: string[]; count: number; edgeTypes: Set<EdgeType> }>()

  for (const edge of edges) {
    if (!CYCLE_EDGE_TYPES.has(edge.type)) continue
    const sourceNode = byId.get(edge.source)
    const targetNode = byId.get(edge.target)
    if (!sourceNode || !targetNode) continue
    const source = dependencyGroupFor(sourceNode)
    const target = dependencyGroupFor(targetNode)
    if (!source || !target || source.key === target.key) continue
    const key = `${source.key}->${target.key}`
    const bucket = directional.get(key) ?? { source, target, edgeIds: [], count: 0, edgeTypes: new Set<EdgeType>() }
    bucket.edgeIds.push(...edgeIdsFor(edge))
    bucket.count += edgeWeight(edge)
    bucket.edgeTypes.add(edge.type)
    directional.set(key, bucket)
  }

  const issues: HotspotIssue[] = []
  const seenPairs = new Set<string>()
  for (const bucket of directional.values()) {
    const reverse = directional.get(`${bucket.target.key}->${bucket.source.key}`)
    if (!reverse) continue
    const pairKey = [bucket.source.key, bucket.target.key].sort().join('<->')
    if (seenPairs.has(pairKey)) continue
    seenPairs.add(pairKey)
    const edgeIds = uniqueStrings([...bucket.edgeIds, ...reverse.edgeIds])
    const count = bucket.count + reverse.count
    const files = uniqueStrings([bucket.source.file, bucket.target.file].filter(Boolean) as string[])
    const modules = uniqueStrings([bucket.source.module, bucket.target.module].filter(Boolean) as string[])
    issues.push({
      id: `cycle:${pairKey}`,
      nodeId: byGroupRepresentative(bucket.source, bucket.target),
      edgeIds,
      title: `Cycle candidate: ${bucket.source.label} <-> ${bucket.target.label}`,
      description: `${count} relationships connect both directions between ${files.length || modules.length} grouped item${(files.length || modules.length) === 1 ? '' : 's'}.`,
      severity: 'warning',
      kind: 'cycle-candidate',
      confidence: 'high',
      connections: count,
      files,
      modules,
      suggestion: 'Inspect the dependency matrix cell and move shared contracts behind a lower-level module.',
    })
  }
  return issues
}

function detectLowConfidenceMutualReferences(edges: GraphEdge[], byId: Map<string, GraphNode>) {
  const allowed = edges.filter(edge => LOW_CONFIDENCE_MUTUAL_TYPES.has(edge.type))
  const pairs = new Set(allowed.map(edge => `${edge.source}->${edge.target}`))
  const seen = new Set<string>()
  const issues: HotspotIssue[] = []
  for (const edge of allowed) {
    const reverse = `${edge.target}->${edge.source}`
    const pairKey = [edge.source, edge.target].sort().join('<->')
    if (!pairs.has(reverse) || seen.has(pairKey)) continue
    seen.add(pairKey)
    const source = byId.get(edge.source)
    const target = byId.get(edge.target)
    issues.push({
      id: `low-cycle:${pairKey}`,
      nodeId: edge.source,
      edgeIds: [edge.id],
      title: `${source?.label ?? edge.source} and ${target?.label ?? edge.target} reference each other`,
      description: 'This is a low-confidence symbol/type mutual reference, hidden by default.',
      severity: 'info',
      kind: 'cycle-candidate',
      confidence: 'low',
      connections: 2,
      files: uniqueStrings([source?.file, target?.file].filter(Boolean) as string[]),
      modules: uniqueStrings([source?.module, target?.module].filter(Boolean) as string[]),
      suggestion: 'Only inspect this if you are already debugging this symbol pair.',
    })
  }
  return issues
}

function detectBoundaryViolations(edges: GraphEdge[], byId: Map<string, GraphNode>) {
  const buckets = new Map<string, HotspotIssue>()
  for (const edge of edges) {
    if (!BOUNDARY_EDGE_TYPES.has(edge.type)) continue
    const source = byId.get(edge.source)
    const target = byId.get(edge.target)
    if (!source || !target) continue
    const sourceZone = zoneFor(source)
    const targetZone = zoneFor(target)
    const sourcePath = normalizePath(source.file)
    const targetPath = normalizePath(target.file)

    let violation: { key: string; title: string; severity: HotspotSeverity; description: string; suggestion: string; confidence: HotspotConfidence } | null = null

    if (sourceZone === 'backend' && targetZone === 'frontend') {
      violation = {
        key: 'backend-to-frontend',
        title: 'Backend depends on frontend code',
        severity: 'critical',
        confidence: 'high',
        description: 'Backend code should not import, call or depend on frontend implementation files.',
        suggestion: 'Move shared contracts to a shared module or invert the dependency.',
      }
    } else if (sourcePath.includes('backend/') && sourcePath.includes('/models') && targetPath.includes('backend/') && targetPath.includes('/routes')) {
      violation = {
        key: 'backend-models-to-routes',
        title: 'Backend models depend on routes',
        severity: 'warning',
        confidence: 'high',
        description: 'Model/domain code should not depend on route/controller code.',
        suggestion: 'Move route-specific behavior out of the model layer.',
      }
    } else if (sourceZone === 'frontend' && targetZone === 'backend' && (edge.type === 'Imports' || edge.type === 'Uses')) {
      violation = {
        key: 'frontend-to-backend-internals',
        title: 'Frontend imports backend internals',
        severity: 'warning',
        confidence: 'high',
        description: 'Frontend code should reach backend through API calls, not direct imports or symbol uses.',
        suggestion: 'Replace direct backend dependency with an API/Data Flow edge or shared DTO contract.',
      }
    }

    if (!violation) continue
    const key = `boundary:${violation.key}:${groupPathFor(source)}->${groupPathFor(target)}`
    const existing = buckets.get(key)
    if (existing) {
      existing.connections += edgeWeight(edge)
      existing.edgeIds?.push(...edgeIdsFor(edge))
      existing.files = uniqueStrings([...(existing.files ?? []), source.file, target.file].filter(Boolean) as string[])
      existing.modules = uniqueStrings([...(existing.modules ?? []), source.module, target.module].filter(Boolean) as string[])
      continue
    }
    buckets.set(key, {
      id: key,
      nodeId: source.id,
      edgeIds: edgeIdsFor(edge),
      title: violation.title,
      description: violation.description,
      severity: violation.severity,
      kind: 'boundary-violation',
      confidence: violation.confidence,
      connections: edgeWeight(edge),
      files: uniqueStrings([source.file, target.file].filter(Boolean) as string[]),
      modules: uniqueStrings([source.module, target.module].filter(Boolean) as string[]),
      suggestion: violation.suggestion,
    })
  }
  return [...buckets.values()]
}

function weightedCount(edges: GraphEdge[]) {
  return edges.reduce((count, edge) => count + edgeWeight(edge), 0)
}

function edgeWeight(edge: GraphEdge) {
  return Math.max(1, edge.bundledCount ?? edge.bundledEdgeIds?.length ?? 1)
}

function edgeIdsFor(edge: GraphEdge) {
  return edge.bundledEdgeIds?.length ? edge.bundledEdgeIds : [edge.id]
}

function isGodModuleDegree(degree: number, averageDegree: number, p95Degree: number) {
  return (degree >= p95Degree && degree >= Math.max(8, averageDegree * 2))
    || degree > Math.max(30, averageDegree * 4)
}

function percentile(sortedValues: number[], percentileValue: number) {
  if (!sortedValues.length) return 0
  const index = Math.min(sortedValues.length - 1, Math.max(0, Math.ceil(sortedValues.length * percentileValue) - 1))
  return sortedValues[index]
}

function isNoiseUtility(label: string, file: string) {
  return ['utils', 'helpers', 'cn', 'clsx', 'format', 'constants', 'mock'].some(part =>
    label === part || label.includes(part) || file.includes(`/${part}`),
  )
}

function noiseBucketKey(label: string, file: string) {
  const match = ['utils', 'helpers', 'cn', 'clsx', 'format', 'constants', 'mock'].find(part =>
    label === part || label.includes(part) || file.includes(`/${part}`),
  )
  return match ?? label
}

function isCentralApiClient(node: GraphNode, label: string, file: string, incoming: GraphEdge[], outgoing: GraphEdge[]) {
  if (node.type === 'Endpoint' || node.type === 'Handler') return false
  const apiEdges = [...incoming, ...outgoing].filter(edge =>
    edge.type === 'ApiCall' || edge.type === 'DataFlow' || edge.type === 'EndpointHandler',
  )
  const looksLikeClient = ['requestjson', 'apiclient', 'api/client', 'client.ts', 'http', 'fetch', 'loadperson'].some(part => label.includes(part) || file.includes(part))
  return (looksLikeClient && weightedCount(apiEdges) >= 2)
    || (looksLikeClient && apiEdges.length > 0)
}

interface DependencyGroup {
  key: string
  label: string
  file?: string
  module?: string
  representativeNodeId: string
}

function dependencyGroupFor(node: GraphNode): DependencyGroup | null {
  const file = normalizePath(node.file)
  if (file) {
    return { key: `file:${file}`, label: file, file: node.file, module: node.module, representativeNodeId: node.id }
  }
  if (node.module) {
    return { key: `module:${node.module}`, label: node.module, module: node.module, representativeNodeId: node.id }
  }
  return null
}

function byGroupRepresentative(source: DependencyGroup, target: DependencyGroup) {
  return source.representativeNodeId || target.representativeNodeId
}

function groupPathFor(node: GraphNode) {
  return normalizePath(node.file) || node.module || node.id
}

function zoneFor(node: GraphNode): 'frontend' | 'backend' | 'external' | 'unknown' {
  const file = normalizePath(node.file)
  const language = node.language
  if (node.type === 'ExternalCrate' || node.reachability === 'External') return 'external'
  if (file.startsWith('frontend/') || file.includes('/frontend/') || file.endsWith('.tsx') || file.endsWith('.ts') || language === 'typescript' || language === 'qml') return 'frontend'
  if (file.startsWith('backend/') || file.includes('/backend/') || file.endsWith('.rs') || file.endsWith('.py') || language === 'rust' || language === 'python') return 'backend'
  return 'unknown'
}

function dedupeIssues(issues: HotspotIssue[]) {
  const byId = new Map<string, HotspotIssue>()
  for (const issue of issues) {
    const existing = byId.get(issue.id)
    if (!existing) {
      byId.set(issue.id, { ...issue, edgeIds: uniqueStrings(issue.edgeIds ?? []), files: uniqueStrings(issue.files ?? []), modules: uniqueStrings(issue.modules ?? []) })
      continue
    }
    existing.connections += issue.connections
    existing.edgeIds = uniqueStrings([...(existing.edgeIds ?? []), ...(issue.edgeIds ?? [])])
    existing.files = uniqueStrings([...(existing.files ?? []), ...(issue.files ?? [])])
    existing.modules = uniqueStrings([...(existing.modules ?? []), ...(issue.modules ?? [])])
  }
  return [...byId.values()]
}

function uniqueStrings(values: string[]) {
  return [...new Set(values.filter(Boolean))]
}

function normalizePath(path?: string | null) {
  return (path ?? '').replaceAll('\\', '/').replace(/^\/+/, '')
}
