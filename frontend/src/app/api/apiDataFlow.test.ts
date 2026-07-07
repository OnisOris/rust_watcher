import { describe, expect, it } from 'vitest'
import { buildApiDataFlowRows, buildApiEndpointGroups, normalizeRouteKey } from './apiDataFlow'
import type { EdgeType, GraphEdge, GraphNode, NodeType } from '../types'
import { makeEdge, makeNode } from './testGraphBuilders'

function node(id: string, type: NodeType, label = id, file?: string): GraphNode {
  return makeNode({ id, type, label, file })
}

function edge(id: string, source: string, target: string, type: EdgeType, label?: string): GraphEdge {
  return makeEdge({ id, source, target, type, label })
}

describe('API endpoint groups', () => {
  it('normalizes route keys', () => {
    expect(normalizeRouteKey('get', ' /api/person/ ')).toBe('GET /api/person')
    expect(normalizeRouteKey('POST', '/api/person/{name}')).toBe('POST /api/person/:name')
    expect(normalizeRouteKey('DELETE', '//api/io/modules/{id}/')).toBe('DELETE /api/io/modules/:id')
  })

  it('groups multiple callers for the same endpoint', () => {
    const groups = buildApiEndpointGroups(
      [
        node('window', 'Object', 'ApplicationWindow', 'qml/Main.qml'),
        node('hook', 'Function', 'usePerson', 'frontend/src/App.tsx'),
        node('endpoint', 'Endpoint', 'GET /api/person', 'src/main.rs'),
        node('handler', 'Function', 'get_person', 'src/main.rs'),
      ],
      [
        edge('api-1', 'window', 'endpoint', 'ApiCall', 'GET /api/person'),
        edge('api-2', 'hook', 'endpoint', 'ApiCall', 'GET /api/person'),
        edge('handler', 'endpoint', 'handler', 'EndpointHandler'),
      ],
    )

    expect(groups).toHaveLength(1)
    expect(groups[0]).toMatchObject({ routeKey: 'GET /api/person', status: 'ok' })
    expect(groups[0].callers.map(caller => caller.label).sort()).toEqual(['ApplicationWindow', 'usePerson'])
  })

  it('keeps GET and POST for the same path as separate groups', () => {
    const groups = buildApiEndpointGroups(
      [
        node('get', 'Endpoint', 'GET /api/person'),
        node('post', 'Endpoint', 'POST /api/person'),
      ],
      [],
    )

    expect(groups.map(group => group.routeKey).sort()).toEqual(['GET /api/person', 'POST /api/person'])
  })

  it('marks no-handler status', () => {
    const groups = buildApiEndpointGroups(
      [node('caller', 'Component'), node('endpoint', 'Endpoint', 'GET /api/person')],
      [edge('api', 'caller', 'endpoint', 'ApiCall', 'GET /api/person')],
    )

    expect(groups[0].status).toBe('no-handler')
  })

  it('marks no-caller status', () => {
    const groups = buildApiEndpointGroups(
      [node('endpoint', 'Endpoint', 'GET /api/person'), node('handler', 'Function', 'get_person')],
      [edge('handler', 'endpoint', 'handler', 'EndpointHandler')],
    )

    expect(groups[0].status).toBe('no-caller')
  })

  it('collects backend handlers for one endpoint group', () => {
    const groups = buildApiEndpointGroups(
      [
        node('caller', 'Component', 'App', 'frontend/src/App.tsx'),
        node('endpoint', 'Endpoint', 'GET /api/person', 'src/main.rs'),
        node('handler', 'Function', 'get_person', 'src/main.rs'),
        node('router', 'Function', 'api_router', 'src/main.rs'),
      ],
      [
        edge('api', 'caller', 'endpoint', 'ApiCall'),
        edge('handler', 'endpoint', 'handler', 'EndpointHandler'),
        edge('router', 'endpoint', 'router', 'EndpointHandler'),
      ],
    )

    expect(groups).toHaveLength(1)
    expect(groups[0].handlers.map(handler => handler.label).sort()).toEqual(['api_router', 'get_person'])
    expect(groups[0].underlyingEdgeIds.sort()).toEqual(['api', 'handler', 'router'])
  })

  it('marks unused endpoint status', () => {
    const groups = buildApiEndpointGroups([node('endpoint', 'Endpoint', 'GET /api/person')], [])

    expect(groups[0].status).toBe('unused')
  })

  it('marks unresolved API calls when endpoint node cannot be matched', () => {
    const groups = buildApiEndpointGroups(
      [node('caller', 'Component', 'PersonCard')],
      [edge('api', 'caller', 'missing', 'ApiCall', 'GET /api/person')],
    )

    expect(groups[0]).toMatchObject({
      routeKey: 'GET /api/person',
      status: 'unresolved',
    })
    expect(groups[0].callers[0].label).toBe('PersonCard')
  })

  it('collects data types from DataFlow and TypeReference edges', () => {
    const groups = buildApiEndpointGroups(
      [
        node('caller', 'Component'),
        node('endpoint', 'Endpoint', 'GET /api/person'),
        node('handler', 'Function', 'get_person'),
        node('person', 'Struct', 'Person'),
        node('service', 'Class', 'PersonService'),
      ],
      [
        edge('api', 'caller', 'endpoint', 'ApiCall'),
        edge('handler', 'endpoint', 'handler', 'EndpointHandler'),
        edge('type', 'handler', 'person', 'TypeReference'),
        edge('data', 'endpoint', 'service', 'DataFlow'),
      ],
    )

    expect(groups[0].dataTypes.map(type => type.label).sort()).toEqual(['Person', 'PersonService'])
    expect(groups[0].edgeTypeCounts).toMatchObject({ ApiCall: 1, EndpointHandler: 1, TypeReference: 1, DataFlow: 1 })
  })

  it('keeps row compatibility wrapper', () => {
    const rows = buildApiDataFlowRows(
      [node('caller', 'Component'), node('endpoint', 'Endpoint', 'GET /api/settings'), node('handler', 'Function', 'get_settings')],
      [edge('api', 'caller', 'endpoint', 'ApiCall'), edge('handler', 'endpoint', 'handler', 'EndpointHandler')],
    )

    expect(rows[0]).toMatchObject({
      status: 'ok',
      frontendCallerNodeId: 'caller',
      backendHandlerNodeId: 'handler',
      method: 'GET',
      endpointPath: '/api/settings',
    })
  })
})
