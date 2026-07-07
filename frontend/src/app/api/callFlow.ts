import type { EdgeType, GraphEdge, GraphNode } from '../types'
import { normalizeRouteKey } from './apiDataFlow'
import type { CallFlowGroup, CallFlowPath, CallFlowStep } from '../views/architecture/architectureTypes'

export interface CallFlowOptions {
  includeTypeOnly?: boolean
  maxPaths?: number
}

interface RelatedNode {
  node: GraphNode
  edge?: GraphEdge
}

interface RouteInfo {
  routeKey: string
  method?: string
  path?: string
}

const COLUMNS: CallFlowStep['role'][] = ['frontend', 'state', 'api-client', 'endpoint', 'handler', 'service', 'model']
const PLACEHOLDER_LABELS: Partial<Record<CallFlowStep['role'], string>> = {
  state: 'No hook detected',
  'api-client': 'No API client detected',
  service: 'No service detected',
  model: 'No model detected',
}
const MODEL_TYPES = new Set(['Struct', 'Class', 'Interface', 'TypeAlias', 'Enum'])
const CHAIN_EDGE_TYPES = new Set<EdgeType>(['Calls', 'Uses', 'Renders', 'DataFlow'])
const METHOD_RE = /\b(GET|POST|PUT|PATCH|DELETE)\b/i
const PATH_RE = /(\/api\/[A-Za-z0-9_/:{}.-]+|\/[A-Za-z0-9_/:{}.-]+)/

export function buildCallFlowPaths(
  nodes: GraphNode[],
  edges: GraphEdge[],
  options: CallFlowOptions = {},
): CallFlowPath[] {
  const includeTypeOnly = options.includeTypeOnly ?? true
  const maxPaths = options.maxPaths ?? 80
  const byId = new Map(nodes.map(node => [node.id, node]))
  const endpointNodes = nodes.filter(node => node.type === 'Endpoint')
  const endpointIds = new Set(endpointNodes.map(node => node.id))
  const paths: CallFlowPath[] = []

  for (const endpoint of endpointNodes) {
    const apiEdges = edges.filter(edge => edge.type === 'ApiCall' && edge.target === endpoint.id)
    const handlerEdge = findHandlerEdge(endpoint, edges, byId)
    const route = routeInfoFor(endpoint)

    if (apiEdges.length) {
      for (const apiEdge of apiEdges) {
        const caller = byId.get(apiEdge.source)
        if (!caller) continue
        paths.push(buildPathForEndpoint({
          endpoint,
          caller,
          route,
          apiEdge,
          handlerEdge,
          nodesById: byId,
          edges,
          includeTypeOnly,
        }))
      }
      continue
    }

    if (handlerEdge) {
      paths.push(buildPathForEndpoint({
        endpoint,
        route,
        handlerEdge,
        nodesById: byId,
        edges,
        includeTypeOnly,
      }))
    }
  }

  for (const apiEdge of edges.filter(edge => edge.type === 'ApiCall' && !endpointIds.has(edge.target))) {
    const caller = byId.get(apiEdge.source)
    const target = byId.get(apiEdge.target)
    if (!caller || !target) continue
    const route = routeInfoFor(target, apiEdge)
    paths.push(buildPathForEndpoint({
      endpoint: target,
      caller,
      route,
      apiEdge,
      nodesById: byId,
      edges,
      includeTypeOnly,
    }))
  }

  return paths
    .sort((a, b) => a.routeKey.localeCompare(b.routeKey) || a.label.localeCompare(b.label))
    .slice(0, maxPaths)
}

export function buildCallFlowGroups(paths: CallFlowPath[]): CallFlowGroup[] {
  const groups = new Map<string, CallFlowGroup>()
  for (const path of paths) {
    const group = groups.get(path.routeKey) ?? {
      id: `call-flow-group:${path.routeKey}`,
      routeKey: path.routeKey,
      method: path.method,
      path: path.path,
      paths: [],
      callerLabels: [],
      edgeIds: [],
    }
    group.paths.push(path)
    const caller = firstCallerLabel(path)
    if (caller && !group.callerLabels.includes(caller)) group.callerLabels.push(caller)
    for (const edgeId of path.edgeIds) {
      if (!group.edgeIds.includes(edgeId)) group.edgeIds.push(edgeId)
    }
    groups.set(path.routeKey, group)
  }

  return [...groups.values()]
    .map(group => ({
      ...group,
      paths: group.paths.sort((a, b) => a.label.localeCompare(b.label)),
      callerLabels: group.callerLabels.sort((a, b) => a.localeCompare(b)),
    }))
    .sort((a, b) => a.routeKey.localeCompare(b.routeKey))
}

export function roleForCallFlowNode(node: GraphNode, context?: { viaEndpointHandler?: boolean }): CallFlowStep['role'] {
  const file = normalizePath(node.file).toLowerCase()
  const label = node.label.toLowerCase()
  if (node.type === 'Endpoint') return 'endpoint'
  if (context?.viaEndpointHandler || node.type === 'Handler') return 'handler'
  if (node.type === 'Hook' || /^use[A-Z_-]/.test(node.label) || label.startsWith('use_') || label.startsWith('use-')) return 'state'
  if (file.includes('/api/') || /(^|[_-])(request|fetch|client)|(?:request|fetch|client)$/i.test(node.label)) return 'api-client'
  if (file.includes('service') || file.includes('repo') || /service|repo/i.test(node.label)) return 'service'
  if (MODEL_TYPES.has(node.type)) return 'model'
  if (
    node.type === 'Component'
    || ((node.type === 'Object' || node.type === 'Function' || node.type === 'Method') && (node.language === 'typescript' || node.language === 'qml'))
  ) return 'frontend'
  return 'unknown'
}

function buildPathForEndpoint({
  endpoint,
  caller,
  route,
  apiEdge,
  handlerEdge,
  nodesById,
  edges,
  includeTypeOnly,
}: {
  endpoint: GraphNode
  caller?: GraphNode
  route: RouteInfo
  apiEdge?: GraphEdge
  handlerEdge?: GraphEdge
  nodesById: Map<string, GraphNode>
  edges: GraphEdge[]
  includeTypeOnly: boolean
}): CallFlowPath {
  const selectedEdges: GraphEdge[] = []
  const stepsByRole = new Map<CallFlowStep['role'], CallFlowStep[]>()

  if (apiEdge) selectedEdges.push(apiEdge)

  const callerRole = caller ? roleForCallFlowNode(caller) : undefined
  if (caller && callerRole && callerRole !== 'unknown') {
    addStep(stepsByRole, actualStep(`${apiEdge?.id ?? endpoint.id}:caller`, caller, callerRole))
  }

  const frontend = callerRole === 'frontend' ? undefined : findRelatedRole(caller ? [caller] : [endpoint], 'frontend', edges, nodesById, new Set([endpoint.id]))
  const state = callerRole === 'state' ? undefined : findRelatedRole([caller, endpoint].filter(Boolean) as GraphNode[], 'state', edges, nodesById, new Set([endpoint.id]))
  const apiClient = callerRole === 'api-client' ? undefined : findRelatedRole([caller, endpoint].filter(Boolean) as GraphNode[], 'api-client', edges, nodesById, new Set([endpoint.id]))
  for (const related of [frontend, state, apiClient]) {
    if (!related) continue
    addStep(stepsByRole, actualStep(`${related.edge?.id ?? related.node.id}:${roleForCallFlowNode(related.node)}`, related.node, roleForCallFlowNode(related.node)))
    if (related.edge) selectedEdges.push(related.edge)
  }

  addStep(stepsByRole, actualStep(`${endpoint.id}:endpoint`, endpoint, 'endpoint'))

  const handler = handlerEdge ? handlerForEdge(endpoint, handlerEdge, nodesById) : undefined
  if (handlerEdge) selectedEdges.push(handlerEdge)
  if (handler) addStep(stepsByRole, actualStep(`${handlerEdge?.id ?? handler.id}:handler`, handler, 'handler'))

  const service = handler ? findService(handler, edges, nodesById) : undefined
  if (service?.edge) selectedEdges.push(service.edge)
  if (service) addStep(stepsByRole, actualStep(`${service.edge?.id ?? service.node.id}:service`, service.node, 'service'))

  const model = findModel([endpoint, handler, service?.node].filter(Boolean) as GraphNode[], edges, nodesById, includeTypeOnly)
  if (model?.edge) selectedEdges.push(model.edge)
  if (model) addStep(stepsByRole, actualStep(`${model.edge?.id ?? model.node.id}:model`, model.node, 'model'))

  const steps = withPlaceholders(stepsByRole, route.routeKey)
  const edgeIds = unique(selectedEdges.map(edge => edge.id))
  return {
    id: apiEdge?.id ?? handlerEdge?.id ?? `route:${route.routeKey}`,
    label: pathLabel(steps, endpoint),
    routeKey: route.routeKey,
    method: route.method,
    path: route.path,
    source: 'heuristic',
    steps,
    edgeIds,
    edgeTypes: edgeTypeCounts(selectedEdges),
    files: filesForSteps(steps),
  }
}

function withPlaceholders(stepsByRole: Map<CallFlowStep['role'], CallFlowStep[]>, routeKey: string) {
  const steps: CallFlowStep[] = []
  for (const role of COLUMNS) {
    const actual = stepsByRole.get(role)
    if (actual?.length) {
      steps.push(...dedupeSteps(actual))
      continue
    }
    const placeholder = PLACEHOLDER_LABELS[role]
    if (placeholder) {
      steps.push({
        id: `placeholder:${routeKey}:${role}`,
        label: placeholder,
        role,
        isPlaceholder: true,
      })
    }
  }
  return steps
}

function findHandlerEdge(endpoint: GraphNode, edges: GraphEdge[], nodesById: Map<string, GraphNode>) {
  return edges.find(edge => {
    if (edge.type !== 'EndpointHandler') return false
    const source = nodesById.get(edge.source)
    const target = nodesById.get(edge.target)
    return source?.id === endpoint.id || target?.id === endpoint.id
  })
}

function handlerForEdge(endpoint: GraphNode, edge: GraphEdge, nodesById: Map<string, GraphNode>) {
  const source = nodesById.get(edge.source)
  const target = nodesById.get(edge.target)
  return source?.id === endpoint.id ? target : target?.id === endpoint.id ? source : undefined
}

function findRelatedRole(
  starts: GraphNode[],
  role: CallFlowStep['role'],
  edges: GraphEdge[],
  nodesById: Map<string, GraphNode>,
  excluded: Set<string>,
): RelatedNode | undefined {
  for (const start of starts) {
    for (const edge of edges) {
      if (!CHAIN_EDGE_TYPES.has(edge.type) && edge.type !== 'ApiCall') continue
      if (edge.source !== start.id && edge.target !== start.id) continue
      const otherId = edge.source === start.id ? edge.target : edge.source
      if (excluded.has(otherId)) continue
      const other = nodesById.get(otherId)
      if (other && roleForCallFlowNode(other) === role) return { node: other, edge }
    }
  }
  return undefined
}

function findService(handler: GraphNode, edges: GraphEdge[], nodesById: Map<string, GraphNode>): RelatedNode | undefined {
  const outgoing = edges.filter(edge => edge.type === 'Calls' && edge.source === handler.id)
  const explicit = outgoing
    .map(edge => ({ edge, node: nodesById.get(edge.target) }))
    .find(candidate => candidate.node && roleForCallFlowNode(candidate.node) === 'service')
  if (explicit?.node) return { node: explicit.node, edge: explicit.edge }
  const fallback = outgoing.map(edge => ({ edge, node: nodesById.get(edge.target) })).find(candidate => candidate.node && roleForCallFlowNode(candidate.node) !== 'model')
  return fallback?.node ? { node: fallback.node, edge: fallback.edge } : undefined
}

function findModel(starts: GraphNode[], edges: GraphEdge[], nodesById: Map<string, GraphNode>, includeTypeOnly: boolean): RelatedNode | undefined {
  const allowedTypes = includeTypeOnly ? new Set<EdgeType>(['DataFlow', 'TypeReference']) : new Set<EdgeType>(['DataFlow'])
  for (const start of starts) {
    for (const edge of edges) {
      if (!allowedTypes.has(edge.type)) continue
      if (edge.source !== start.id && edge.target !== start.id) continue
      const otherId = edge.source === start.id ? edge.target : edge.source
      const other = nodesById.get(otherId)
      if (other && roleForCallFlowNode(other) === 'model') return { node: other, edge }
    }
  }
  return undefined
}

function actualStep(id: string, node: GraphNode, role: CallFlowStep['role']): CallFlowStep {
  return {
    id,
    label: node.label,
    node,
    file: normalizePath(node.file),
    role,
  }
}

function addStep(map: Map<CallFlowStep['role'], CallFlowStep[]>, step: CallFlowStep) {
  const existing = map.get(step.role) ?? []
  if (!existing.some(candidate => candidate.node?.id === step.node?.id && candidate.label === step.label)) {
    map.set(step.role, [...existing, step])
  }
}

function dedupeSteps(steps: CallFlowStep[]) {
  const seen = new Set<string>()
  return steps.filter(step => {
    const key = step.node?.id ?? step.id
    if (seen.has(key)) return false
    seen.add(key)
    return true
  })
}

function pathLabel(steps: CallFlowStep[], endpoint: GraphNode) {
  const first = steps.find(step => !step.isPlaceholder && step.role !== 'endpoint')
  const tail = steps.filter(step => !step.isPlaceholder).slice(-1)[0]
  if (!first) return `${endpoint.label} -> ${tail?.label ?? 'handler'}`
  return `${first.label} -> ${endpoint.label}`
}

function firstCallerLabel(path: CallFlowPath) {
  return path.steps.find(step => !step.isPlaceholder && (step.role === 'frontend' || step.role === 'state' || step.role === 'api-client'))?.label ?? 'Backend route'
}

function edgeTypeCounts(edges: GraphEdge[]) {
  const counts: Partial<Record<EdgeType, number>> = {}
  for (const edge of edges) {
    if (edge.bundledTypes?.length) {
      for (const type of edge.bundledTypes) counts[type] = (counts[type] ?? 0) + 1
      continue
    }
    counts[edge.type] = (counts[edge.type] ?? 0) + Math.max(1, edge.bundledCount ?? 1)
  }
  return counts
}

function filesForSteps(steps: CallFlowStep[]) {
  return unique(steps.flatMap(step => normalizePath(step.file ?? step.node?.file) ? [normalizePath(step.file ?? step.node?.file)] : []))
}

function routeInfoFor(endpoint: GraphNode, edge?: GraphEdge): RouteInfo {
  const method = extractMethod(edge?.label, edge?.description, endpoint.label, endpoint.description)
  const path = extractPath(edge?.label, edge?.description, endpoint.label, endpoint.description) ?? endpoint.label
  const routeKey = normalizeRouteKey(method, path)
  return {
    routeKey,
    method: routeKey.split(' ')[0],
    path: routeKey.replace(/^[A-Z]+ /, ''),
  }
}

function extractMethod(...values: Array<string | undefined>) {
  for (const value of values) {
    const match = value?.match(METHOD_RE)
    if (match) return match[1].toUpperCase()
  }
  return undefined
}

function extractPath(...values: Array<string | undefined>) {
  for (const value of values) {
    const match = value?.match(PATH_RE)
    if (match) return match[1]
  }
  return undefined
}

function normalizePath(path?: string | null) {
  return (path ?? '').replaceAll('\\', '/').replace(/^\/+/, '')
}

function unique<T>(values: T[]) {
  return [...new Set(values)]
}
