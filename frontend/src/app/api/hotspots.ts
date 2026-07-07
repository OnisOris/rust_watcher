import type { GraphEdge, GraphNode } from '../types'
import type { HotspotIssue, HotspotSeverity } from '../views/architecture/architectureTypes'

const SEVERITY_RANK: Record<HotspotSeverity, number> = {
  critical: 0,
  warning: 1,
  info: 2,
  noise: 3,
}

export function buildHotspotIssues(nodes: GraphNode[], edges: GraphEdge[]): HotspotIssue[] {
  const incoming = new Map<string, GraphEdge[]>()
  const outgoing = new Map<string, GraphEdge[]>()
  const byId = new Map(nodes.map(node => [node.id, node]))

  for (const edge of edges) {
    incoming.set(edge.target, [...(incoming.get(edge.target) ?? []), edge])
    outgoing.set(edge.source, [...(outgoing.get(edge.source) ?? []), edge])
  }

  const issues: HotspotIssue[] = []

  for (const node of nodes) {
    const inEdges = incoming.get(node.id) ?? []
    const outEdges = outgoing.get(node.id) ?? []
    const incomingCount = weightedCount(inEdges)
    const outgoingCount = weightedCount(outEdges)
    const degree = incomingCount + outgoingCount
    const label = node.label.toLowerCase()
    const file = (node.file ?? '').toLowerCase()

    if ((node.type === 'File' || node.type === 'Module') && degree > 80) {
      issues.push({
        id: `god-module:${node.id}`,
        nodeId: node.id,
        title: `${node.label} has ${degree} dependencies`,
        description: 'This file or module is central enough to be hard to reason about in the raw graph.',
        severity: degree > 200 ? 'critical' : 'warning',
        kind: 'god-module',
        connections: degree,
        suggestion: 'Split responsibilities or inspect its local neighborhood before editing.',
      })
    }

    if (isNoiseUtility(label, file) && degree > 20) {
      issues.push({
        id: `noise-utility:${node.id}`,
        nodeId: node.id,
        title: `${node.label} looks like graph noise`,
        description: 'This helper appears in many places and can obscure more meaningful architecture edges.',
        severity: 'noise',
        kind: 'noise-utility',
        connections: degree,
        suggestion: 'Hide as noise by default or collapse into a utility group.',
      })
    }

    if (isCentralApiClient(label, file, inEdges, outEdges)) {
      issues.push({
        id: `central-api-client:${node.id}`,
        nodeId: node.id,
        title: `${node.label} is a central API client`,
        description: 'Many API, endpoint or data-flow relationships pass through this node.',
        severity: degree > 80 ? 'warning' : 'info',
        kind: 'central-api-client',
        connections: degree,
        suggestion: 'Use API/Data Flow to inspect unresolved endpoints and high-traffic calls.',
      })
    }

    if (incomingCount > 60) {
      issues.push({
        id: `incoming:${node.id}`,
        nodeId: node.id,
        edgeIds: inEdges.map(edge => edge.id),
        title: `${node.label} has many dependents`,
        description: `${incomingCount} incoming relationships point to this node.`,
        severity: incomingCount > 140 ? 'critical' : 'warning',
        kind: 'too-many-incoming',
        connections: incomingCount,
      })
    }

    if (outgoingCount > 60) {
      issues.push({
        id: `outgoing:${node.id}`,
        nodeId: node.id,
        edgeIds: outEdges.map(edge => edge.id),
        title: `${node.label} depends on many nodes`,
        description: `${outgoingCount} outgoing relationships leave this node.`,
        severity: outgoingCount > 140 ? 'critical' : 'warning',
        kind: 'too-many-outgoing',
        connections: outgoingCount,
      })
    }

    if (node.type === 'Endpoint' && !inEdges.some(edge => edge.type === 'ApiCall')) {
      issues.push({
        id: `unused-endpoint:${node.id}`,
        nodeId: node.id,
        title: `${node.label} has no frontend caller`,
        description: 'No incoming ApiCall edge points to this endpoint in the current graph.',
        severity: 'info',
        kind: 'unused-endpoint',
        connections: degree,
        suggestion: 'Remove it, document it as internal, or add the missing caller edge.',
      })
    }
  }

  const endpointHandlerTargets = new Set(edges.filter(edge => edge.type === 'EndpointHandler').flatMap(edge => [edge.source, edge.target]))
  for (const edge of edges.filter(edge => edge.type === 'ApiCall')) {
    const target = byId.get(edge.target)
    if (target?.type === 'Endpoint' && !endpointHandlerTargets.has(target.id)) {
      issues.push({
        id: `frontend-call-without-handler:${edge.id}`,
        nodeId: edge.source,
        edgeIds: [edge.id],
        title: `${target.label} has no handler`,
        description: 'A frontend API call reaches an endpoint without a matched EndpointHandler edge.',
        severity: 'warning',
        kind: 'frontend-call-without-handler',
        connections: 1,
      })
    }
  }

  const seenPairs = new Set<string>()
  const edgePairKeys = new Set(edges.filter(edge => edge.type !== 'Contains').map(edge => `${edge.source}->${edge.target}`))
  for (const edge of edges.filter(edge => edge.type !== 'Contains')) {
    const reverse = `${edge.target}->${edge.source}`
    const pairKey = [edge.source, edge.target].sort().join('<->')
    if (!edgePairKeys.has(reverse) || seenPairs.has(pairKey)) continue
    seenPairs.add(pairKey)
    issues.push({
      id: `cycle:${pairKey}`,
      nodeId: edge.source,
      edgeIds: [edge.id],
      title: `${byId.get(edge.source)?.label ?? edge.source} and ${byId.get(edge.target)?.label ?? edge.target} reference each other`,
      description: 'This mutual dependency is a cycle candidate.',
      severity: 'warning',
      kind: 'cycle-candidate',
      connections: 2,
      suggestion: 'Inspect both directions in the dependency matrix.',
    })
  }

  return issues.sort((a, b) =>
    SEVERITY_RANK[a.severity] - SEVERITY_RANK[b.severity]
    || b.connections - a.connections
    || a.title.localeCompare(b.title)
  )
}

function weightedCount(edges: GraphEdge[]) {
  return edges.reduce((count, edge) => count + Math.max(1, edge.bundledCount ?? edge.bundledEdgeIds?.length ?? 1), 0)
}

function isNoiseUtility(label: string, file: string) {
  return ['utils', 'helpers', 'cn', 'clsx', 'format', 'constants', 'mock'].some(part =>
    label === part || label.includes(part) || file.includes(`/${part}`),
  )
}

function isCentralApiClient(label: string, file: string, incoming: GraphEdge[], outgoing: GraphEdge[]) {
  const apiEdges = [...incoming, ...outgoing].filter(edge =>
    edge.type === 'ApiCall' || edge.type === 'DataFlow' || edge.type === 'EndpointHandler',
  )
  return apiEdges.length >= 8
    || ['requestjson', 'apiclient', 'client.ts', 'http', 'fetch'].some(part => label.includes(part) || file.includes(part))
}
