import type { EdgeType, GraphEdge, GraphNode } from '../types'
import type { ApiDataFlowRow, ApiEndpointGroup, ApiEndpointParticipant } from '../views/architecture/architectureTypes'

const METHOD_RE = /\b(GET|POST|PUT|PATCH|DELETE)\b/i
const PATH_RE = /(\/api\/[A-Za-z0-9_/:{}.-]+|\/[A-Za-z0-9_/:{}.-]+)/
const TYPE_NODE_TYPES = new Set(['Struct', 'Class', 'Interface', 'TypeAlias', 'Enum'])

export function buildApiEndpointGroups(nodes: GraphNode[], edges: GraphEdge[]): ApiEndpointGroup[] {
  const byId = new Map(nodes.map(node => [node.id, node]))
  const endpointNodes = nodes.filter(node => node.type === 'Endpoint')
  const endpointIds = new Set(endpointNodes.map(node => node.id))
  const groups = new Map<string, MutableApiEndpointGroup>()

  for (const endpoint of endpointNodes) {
    const method = extractMethod(endpoint.label, endpoint.description)
    const path = extractPath(endpoint.label, endpoint.description) ?? endpoint.label
    const routeKey = normalizeRouteKey(method, path)
    const group = ensureGroup(groups, routeKey, method, normalizedPathOnly(path))
    group.endpointNodeIds.add(endpoint.id)
  }

  for (const edge of edges) {
    if (edge.type !== 'ApiCall') continue
    const caller = byId.get(edge.source)
    const endpoint = byId.get(edge.target)
    const method = extractMethod(edge.label, edge.description, endpoint?.label, endpoint?.description)
    const path = extractPath(edge.label, edge.description, endpoint?.label, endpoint?.description)
    const routeKey = endpoint?.type === 'Endpoint'
      ? normalizeRouteKey(method, path ?? endpoint.label)
      : normalizeRouteKey(method, path ?? endpoint?.label ?? edge.label ?? edge.target)
    const group = ensureGroup(groups, routeKey, method, normalizedPathOnly(path ?? endpoint?.label ?? edge.label ?? edge.target))
    if (endpoint?.type === 'Endpoint') group.endpointNodeIds.add(endpoint.id)
    if (caller) addParticipant(group.callers, caller)
    addEdge(group, edge)
  }

  for (const edge of edges) {
    if (edge.type !== 'EndpointHandler') continue
    const source = byId.get(edge.source)
    const target = byId.get(edge.target)
    const endpoint = source?.type === 'Endpoint' ? source : target?.type === 'Endpoint' ? target : undefined
    const handler = endpoint?.id === source?.id ? target : source
    if (!endpoint) continue
    const method = extractMethod(endpoint.label, endpoint.description, edge.label)
    const path = extractPath(endpoint.label, endpoint.description, edge.label) ?? endpoint.label
    const group = ensureGroup(groups, normalizeRouteKey(method, path), method, normalizedPathOnly(path))
    group.endpointNodeIds.add(endpoint.id)
    if (handler) addParticipant(group.handlers, handler)
    addEdge(group, edge)
  }

  for (const edge of edges) {
    if (edge.type !== 'DataFlow' && edge.type !== 'TypeReference') continue
    const source = byId.get(edge.source)
    const target = byId.get(edge.target)
    if (!source || !target) continue
    const endpoint = source.type === 'Endpoint' ? source : target.type === 'Endpoint' ? target : undefined
    const other = endpoint?.id === source.id ? target : source
    const handlerEndpoint = !endpoint ? endpointForHandler(source, target, edges, byId) : undefined
    const relatedEndpoint = endpoint ?? handlerEndpoint.endpoint
    const relatedOther = endpoint ? other : handlerEndpoint.other
    if (!relatedEndpoint || !relatedOther || !TYPE_NODE_TYPES.has(relatedOther.type)) continue
    const method = extractMethod(relatedEndpoint.label, relatedEndpoint.description)
    const path = extractPath(relatedEndpoint.label, relatedEndpoint.description) ?? relatedEndpoint.label
    const group = ensureGroup(groups, normalizeRouteKey(method, path), method, normalizedPathOnly(path))
    group.endpointNodeIds.add(relatedEndpoint.id)
    addParticipant(group.dataTypes, relatedOther)
    addEdge(group, edge)
  }

  for (const group of groups.values()) {
    for (const endpointId of group.endpointNodeIds) {
      const endpoint = byId.get(endpointId)
      if (!endpoint) continue
      collectRelatedTypesForEndpoint(group, endpoint, edges, byId)
    }
  }

  return [...groups.values()]
    .map(finalizeGroup)
    .sort((a, b) => statusRank(a.status) - statusRank(b.status) || a.routeKey.localeCompare(b.routeKey))
}

export function buildApiDataFlowRows(nodes: GraphNode[], edges: GraphEdge[]): ApiDataFlowRow[] {
  return buildApiEndpointGroups(nodes, edges).map(group => ({
    id: group.id,
    status: group.status,
    frontendCallerNodeId: group.callers[0]?.nodeId,
    frontendCallerLabel: group.callers[0]?.label,
    method: group.method,
    endpointPath: group.path,
    endpointNodeId: group.endpointNodeIds[0],
    backendHandlerNodeId: group.handlers[0]?.nodeId,
    backendHandlerLabel: group.handlers[0]?.label,
    dataTypeNodeId: group.dataTypes[0]?.nodeId,
    dataTypeLabel: group.dataTypes[0]?.label,
    underlyingEdgeIds: group.underlyingEdgeIds,
  }))
}

export function normalizeRouteKey(method?: string, path?: string): string {
  const normalizedMethod = (method || extractMethod(path) || 'ANY').trim().toUpperCase()
  return `${normalizedMethod} ${normalizedPathOnly(path)}`
}

interface MutableApiEndpointGroup {
  routeKey: string
  method?: string
  path: string
  callers: Map<string, ApiEndpointParticipant>
  handlers: Map<string, ApiEndpointParticipant>
  dataTypes: Map<string, ApiEndpointParticipant>
  endpointNodeIds: Set<string>
  underlyingEdgeIds: Set<string>
  edgeTypeCounts: Partial<Record<EdgeType, number>>
}

function ensureGroup(groups: Map<string, MutableApiEndpointGroup>, routeKey: string, method?: string, path?: string) {
  const existing = groups.get(routeKey)
  if (existing) {
    if (!existing.method && method) existing.method = method.toUpperCase()
    if (existing.path === '/' && path) existing.path = normalizedPathOnly(path)
    return existing
  }
  const group: MutableApiEndpointGroup = {
    routeKey,
    method: method?.toUpperCase() ?? routeKey.split(' ')[0],
    path: path ? normalizedPathOnly(path) : routeKey.replace(/^[A-Z]+ /, ''),
    callers: new Map(),
    handlers: new Map(),
    dataTypes: new Map(),
    endpointNodeIds: new Set(),
    underlyingEdgeIds: new Set(),
    edgeTypeCounts: {},
  }
  groups.set(routeKey, group)
  return group
}

function finalizeGroup(group: MutableApiEndpointGroup): ApiEndpointGroup {
  const callers = [...group.callers.values()].sort(compareParticipants)
  const handlers = [...group.handlers.values()].sort(compareParticipants)
  const dataTypes = [...group.dataTypes.values()].sort(compareParticipants)
  return {
    id: `endpoint-group:${group.routeKey}`,
    routeKey: group.routeKey,
    method: group.method,
    path: group.path,
    status: endpointStatus(group.endpointNodeIds.size, callers.length, handlers.length),
    callers,
    handlers,
    dataTypes,
    endpointNodeIds: [...group.endpointNodeIds],
    underlyingEdgeIds: [...group.underlyingEdgeIds],
    edgeTypeCounts: group.edgeTypeCounts,
  }
}

function endpointStatus(endpointCount: number, callerCount: number, handlerCount: number): ApiEndpointGroup['status'] {
  if (!endpointCount) return 'unresolved'
  if (callerCount > 0 && handlerCount > 0) return 'ok'
  if (callerCount > 0) return 'no-handler'
  if (handlerCount > 0) return 'no-caller'
  return 'unused'
}

function addEdge(group: MutableApiEndpointGroup, edge: GraphEdge) {
  const ids = edge.bundledEdgeIds?.length ? edge.bundledEdgeIds : [edge.id]
  const newIds = ids.filter(id => !group.underlyingEdgeIds.has(id))
  if (!newIds.length) return
  for (const id of newIds) {
    group.underlyingEdgeIds.add(id)
  }
  group.edgeTypeCounts[edge.type] = (group.edgeTypeCounts[edge.type] ?? 0) + Math.max(1, edge.bundledCount ?? newIds.length)
}

function collectRelatedTypesForEndpoint(group: MutableApiEndpointGroup, endpoint: GraphNode, edges: GraphEdge[], byId: Map<string, GraphNode>) {
  const relatedIds = new Set<string>([endpoint.id])
  for (const edge of edges) {
    if (edge.type !== 'EndpointHandler') continue
    if (edge.source === endpoint.id) relatedIds.add(edge.target)
    if (edge.target === endpoint.id) relatedIds.add(edge.source)
  }
  for (const edge of edges) {
    if (edge.type !== 'DataFlow' && edge.type !== 'TypeReference') continue
    const sourceRelated = relatedIds.has(edge.source)
    const targetRelated = relatedIds.has(edge.target)
    if (!sourceRelated && !targetRelated) continue
    const candidate = byId.get(sourceRelated ? edge.target : edge.source)
    if (!candidate || relatedIds.has(candidate.id) || !TYPE_NODE_TYPES.has(candidate.type)) continue
    addParticipant(group.dataTypes, candidate)
    addEdge(group, edge)
  }
}

function endpointForHandler(source: GraphNode, target: GraphNode, edges: GraphEdge[], byId: Map<string, GraphNode>) {
  for (const candidate of [source, target]) {
    for (const edge of edges) {
      if (edge.type !== 'EndpointHandler') continue
      if (edge.source !== candidate.id && edge.target !== candidate.id) continue
      const other = byId.get(edge.source === candidate.id ? edge.target : edge.source)
      if (other?.type === 'Endpoint') {
        return { endpoint: other, other: source.id === candidate.id ? target : source }
      }
    }
  }
  return { endpoint: undefined, other: undefined }
}

function participantFor(node: GraphNode): ApiEndpointParticipant {
  return {
    nodeId: node.id,
    label: node.label,
    file: node.file,
    type: node.type,
    language: node.language,
  }
}

function addParticipant(participants: Map<string, ApiEndpointParticipant>, node: GraphNode) {
  const participant = participantFor(node)
  participants.set(participantKey(participant), participant)
}

function participantKey(participant: ApiEndpointParticipant) {
  return `${participant.type ?? ''}:${participant.label}:${participant.file ?? ''}`
}

function extractMethod(...values: Array<string | undefined>) {
  const joined = values.filter(Boolean).join(' ')
  return METHOD_RE.exec(joined)?.[1]?.toUpperCase()
}

function extractPath(...values: Array<string | undefined>) {
  const joined = values.filter(Boolean).join(' ')
  return PATH_RE.exec(joined)?.[1]
}

function normalizedPathOnly(path?: string) {
  const extracted = extractPath(path) ?? path ?? '/'
  let normalized = extracted.trim()
    .replace(/\{([A-Za-z0-9_]+)\}/g, ':$1')
    .replace(/\/+/g, '/')
  if (!normalized.startsWith('/')) normalized = `/${normalized}`
  if (normalized.length > 1) normalized = normalized.replace(/\/+$/, '')
  return normalized || '/'
}

function compareParticipants(a: ApiEndpointParticipant, b: ApiEndpointParticipant) {
  return a.label.localeCompare(b.label) || (a.file ?? '').localeCompare(b.file ?? '')
}

function statusRank(status: ApiEndpointGroup['status']) {
  switch (status) {
    case 'no-handler': return 0
    case 'no-caller': return 1
    case 'unresolved': return 2
    case 'unused': return 3
    case 'ok': return 4
  }
}
