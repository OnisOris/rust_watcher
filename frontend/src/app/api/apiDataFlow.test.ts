import { describe, expect, it } from 'vitest'
import { buildApiDataFlowRows } from './apiDataFlow'
import type { EdgeType, GraphEdge, GraphNode, NodeType } from '../types'

function node(id: string, type: NodeType, label = id): GraphNode {
  return { id, type, label, x: 0, y: 0, vx: 0, vy: 0 }
}

function edge(id: string, source: string, target: string, type: EdgeType, label?: string): GraphEdge {
  return { id, source, target, type, label }
}

describe('API/data flow builder', () => {
  it('creates an ok row for caller to endpoint to handler', () => {
    const rows = buildApiDataFlowRows(
      [
        node('caller', 'Component', 'SettingsPage'),
        node('endpoint', 'Endpoint', 'GET /api/settings'),
        node('handler', 'Function', 'get_settings'),
        node('dto', 'Struct', 'SettingsDto'),
      ],
      [
        edge('api', 'caller', 'endpoint', 'ApiCall'),
        edge('handler', 'endpoint', 'handler', 'EndpointHandler'),
        edge('dto', 'handler', 'dto', 'TypeReference'),
      ],
    )

    expect(rows[0]).toMatchObject({
      status: 'ok',
      frontendCallerNodeId: 'caller',
      backendHandlerNodeId: 'handler',
      dataTypeNodeId: 'dto',
      method: 'GET',
      endpointPath: '/api/settings',
    })
    expect(rows[0].underlyingEdgeIds).toEqual(['api', 'handler'])
  })

  it('marks endpoints without caller and frontend calls without handlers', () => {
    const rows = buildApiDataFlowRows(
      [
        node('caller', 'Component'),
        node('called', 'Endpoint', 'POST /api/called'),
        node('orphan', 'Endpoint', 'GET /api/orphan'),
      ],
      [edge('api', 'caller', 'called', 'ApiCall')],
    )

    expect(rows.find(row => row.endpointNodeId === 'called')?.status).toBe('no-handler')
    expect(rows.find(row => row.endpointNodeId === 'orphan')?.status).toBe('unused')
  })

  it('creates a no-handler row for unresolved API targets', () => {
    const rows = buildApiDataFlowRows(
      [node('caller', 'Component')],
      [edge('api', 'caller', 'missing', 'ApiCall', 'DELETE /api/items/:id')],
    )

    expect(rows[0]).toMatchObject({
      status: 'no-handler',
      frontendCallerNodeId: 'caller',
      method: 'DELETE',
      endpointPath: '/api/items/:id',
    })
  })
})
