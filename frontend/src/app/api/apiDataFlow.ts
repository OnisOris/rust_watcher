import type { GraphEdge, GraphNode } from '../types'
import type { ApiDataFlowRow } from '../views/architecture/architectureTypes'

const METHOD_RE = /\b(GET|POST|PUT|PATCH|DELETE)\b/i
const PATH_RE = /(\/api\/[A-Za-z0-9_/:{}.-]+|\/[A-Za-z0-9_/:{}.-]+)/

export function buildApiDataFlowRows(nodes: GraphNode[], edges: GraphEdge[]): ApiDataFlowRow[] {
  const byId = new Map(nodes.map(node => [node.id, node]))
  const endpointNodes = nodes.filter(node => node.type === 'Endpoint')
  const endpointIds = new Set(endpointNodes.map(node => node.id))
  const rowsByEndpoint = new Map<string, ApiDataFlowRow>()

  for (const endpoint of endpointNodes) {
    const apiCalls = edges.filter(edge => edge.type === 'ApiCall' && edge.target === endpoint.id)
    const handlerEdge = edges.find(edge =>
      edge.type === 'EndpointHandler' && (edge.source === endpoint.id || edge.target === endpoint.id),
    )
    const handlerId = handlerEdge
      ? handlerEdge.source === endpoint.id ? handlerEdge.target : handlerEdge.source
      : undefined
    const handler = handlerId ? byId.get(handlerId) : undefined
    const dataType = findDataType(endpoint.id, handlerId, nodes, edges)
    const firstCaller = apiCalls.map(edge => byId.get(edge.source)).find(Boolean)
    const method = extractMethod(endpoint.label, endpoint.description, apiCalls[0]?.label)
    const row: ApiDataFlowRow = {
      id: `endpoint:${endpoint.id}`,
      status: apiCalls.length && handler ? 'ok' : apiCalls.length ? 'no-handler' : handler ? 'no-caller' : 'unused',
      frontendCallerNodeId: firstCaller?.id,
      frontendCallerLabel: firstCaller?.label,
      method,
      endpointPath: extractPath(endpoint.label, endpoint.description, apiCalls[0]?.label) ?? endpoint.label,
      endpointNodeId: endpoint.id,
      backendHandlerNodeId: handler?.id,
      backendHandlerLabel: handler?.label,
      dataTypeNodeId: dataType?.id,
      dataTypeLabel: dataType?.label,
      underlyingEdgeIds: [
        ...apiCalls.flatMap(edge => edge.bundledEdgeIds?.length ? edge.bundledEdgeIds : [edge.id]),
        ...(handlerEdge ? handlerEdge.bundledEdgeIds?.length ? handlerEdge.bundledEdgeIds : [handlerEdge.id] : []),
      ],
    }
    rowsByEndpoint.set(endpoint.id, row)
  }

  const rows = [...rowsByEndpoint.values()]

  for (const edge of edges.filter(edge => edge.type === 'ApiCall' && !endpointIds.has(edge.target))) {
    const caller = byId.get(edge.source)
    const path = extractPath(edge.label, edge.description, byId.get(edge.target)?.label)
    rows.push({
      id: `api-call:${edge.id}`,
      status: 'no-handler',
      frontendCallerNodeId: caller?.id,
      frontendCallerLabel: caller?.label,
      method: extractMethod(edge.label, edge.description),
      endpointPath: path ?? byId.get(edge.target)?.label ?? edge.label ?? edge.target,
      underlyingEdgeIds: edge.bundledEdgeIds?.length ? edge.bundledEdgeIds : [edge.id],
    })
  }

  return rows.sort((a, b) => statusRank(a.status) - statusRank(b.status) || a.endpointPath.localeCompare(b.endpointPath))
}

function findDataType(endpointId: string, handlerId: string | undefined, nodes: GraphNode[], edges: GraphEdge[]) {
  const ids = new Set([endpointId, handlerId].filter(Boolean) as string[])
  const typeEdges = edges.filter(edge =>
    (edge.type === 'DataFlow' || edge.type === 'TypeReference') && (ids.has(edge.source) || ids.has(edge.target)),
  )
  const typeIds = new Set(typeEdges.flatMap(edge => [edge.source, edge.target]).filter(id => !ids.has(id)))
  return nodes.find(node => typeIds.has(node.id) && ['Struct', 'Class', 'Interface', 'TypeAlias', 'Enum'].includes(node.type))
}

function extractMethod(...values: Array<string | undefined>) {
  const joined = values.filter(Boolean).join(' ')
  return METHOD_RE.exec(joined)?.[1]?.toUpperCase()
}

function extractPath(...values: Array<string | undefined>) {
  const joined = values.filter(Boolean).join(' ')
  return PATH_RE.exec(joined)?.[1]
}

function statusRank(status: ApiDataFlowRow['status']) {
  switch (status) {
    case 'no-handler': return 0
    case 'no-caller': return 1
    case 'unused': return 2
    case 'unresolved': return 3
    case 'ok': return 4
  }
}
