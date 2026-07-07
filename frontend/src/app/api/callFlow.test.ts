import { describe, expect, it } from 'vitest'
import { buildCallFlowGroups, buildCallFlowPaths, roleForCallFlowNode } from './callFlow'
import type { EdgeType, GraphEdge, GraphNode, NodeType } from '../types'
import { makeEdge, makeNode } from './testGraphBuilders'

function node(id: string, type: NodeType, label = id, file = `src/${id}.rs`, language?: string): GraphNode {
  return makeNode({ id, type, label, file, language })
}

function edge(id: string, source: string, target: string, type: EdgeType): GraphEdge {
  return makeEdge({ id, source, target, type })
}

describe('call flow builder', () => {
  it('builds a path from ApiCall plus EndpointHandler edges', () => {
    const paths = buildCallFlowPaths(
      [
        node('page', 'Component', 'ApplicationWindow', 'qml/Main.qml', 'qml'),
        node('endpoint', 'Endpoint', 'GET /api/person', 'src/main.rs'),
        node('handler', 'Function', 'get_person', 'src/main.rs'),
        node('model', 'Struct', 'Person', 'src/main.rs'),
      ],
      [
        edge('api', 'page', 'endpoint', 'ApiCall'),
        edge('handler-edge', 'endpoint', 'handler', 'EndpointHandler'),
        edge('model-edge', 'handler', 'model', 'DataFlow'),
      ],
    )

    expect(paths).toHaveLength(1)
    expect(paths[0]).toMatchObject({
      routeKey: 'GET /api/person',
      method: 'GET',
      path: '/api/person',
      source: 'heuristic',
    })
    expect(paths[0].edgeIds).toEqual(['api', 'handler-edge', 'model-edge'])
    expect(paths[0].steps.filter(step => !step.isPlaceholder).map(step => step.role)).toEqual(['frontend', 'endpoint', 'handler', 'model'])
  })

  it('adds placeholders for missing hook, API client, service, and model layers', () => {
    const paths = buildCallFlowPaths(
      [
        node('page', 'Component', 'ApplicationWindow', 'qml/Main.qml', 'qml'),
        node('endpoint', 'Endpoint', 'GET /api/person'),
        node('handler', 'Function', 'get_person'),
      ],
      [
        edge('api', 'page', 'endpoint', 'ApiCall'),
        edge('handler-edge', 'endpoint', 'handler', 'EndpointHandler'),
      ],
    )

    expect(paths[0].steps.filter(step => step.isPlaceholder).map(step => step.label)).toEqual([
      'No hook detected',
      'No API client detected',
      'No service detected',
      'No model detected',
    ])
  })

  it('groups multiple callers under the same endpoint', () => {
    const paths = buildCallFlowPaths(
      [
        node('app', 'Component', 'ApplicationWindow', 'qml/Main.qml', 'qml'),
        node('hook', 'Hook', 'usePerson', 'frontend/src/App.tsx', 'typescript'),
        node('endpoint', 'Endpoint', 'GET /api/person'),
        node('handler', 'Function', 'get_person'),
      ],
      [
        edge('api-app', 'app', 'endpoint', 'ApiCall'),
        edge('api-hook', 'hook', 'endpoint', 'ApiCall'),
        edge('handler-edge', 'endpoint', 'handler', 'EndpointHandler'),
      ],
    )

    const groups = buildCallFlowGroups(paths)
    expect(groups).toHaveLength(1)
    expect(groups[0].routeKey).toBe('GET /api/person')
    expect(groups[0].callerLabels).toEqual(['ApplicationWindow', 'usePerson'])
    expect(groups[0].paths).toHaveLength(2)
  })

  it('detects roles for hooks, API clients, endpoints, handlers, services, and models', () => {
    expect(roleForCallFlowNode(node('hook', 'Function', 'usePerson', 'frontend/src/App.tsx', 'typescript'))).toBe('state')
    expect(roleForCallFlowNode(node('client', 'Function', 'requestJson', 'frontend/src/api/client.ts', 'typescript'))).toBe('api-client')
    expect(roleForCallFlowNode(node('endpoint', 'Endpoint', 'GET /api/person'))).toBe('endpoint')
    expect(roleForCallFlowNode(node('handler', 'Function', 'get_person'), { viaEndpointHandler: true })).toBe('handler')
    expect(roleForCallFlowNode(node('service', 'Class', 'PersonService', 'src/person_service.rs'))).toBe('service')
    expect(roleForCallFlowNode(node('model', 'Struct', 'Person'))).toBe('model')
  })

  it('can hide type-only model references', () => {
    const nodes = [
      node('page', 'Component', 'ApplicationWindow', 'qml/Main.qml', 'qml'),
      node('endpoint', 'Endpoint', 'GET /api/person'),
      node('handler', 'Function', 'get_person'),
      node('model', 'Struct', 'Person'),
    ]
    const edges = [
      edge('api', 'page', 'endpoint', 'ApiCall'),
      edge('handler-edge', 'endpoint', 'handler', 'EndpointHandler'),
      edge('type-only', 'handler', 'model', 'TypeReference'),
    ]

    const withTypes = buildCallFlowPaths(nodes, edges)
    const withoutTypes = buildCallFlowPaths(nodes, edges, { includeTypeOnly: false })

    expect(withTypes[0].steps.some(step => step.role === 'model' && step.label === 'Person')).toBe(true)
    expect(withoutTypes[0].steps.find(step => step.role === 'model')).toMatchObject({
      isPlaceholder: true,
      label: 'No model detected',
    })
  })

  it('works without trace API data', () => {
    const paths = buildCallFlowPaths(
      [
        node('endpoint', 'Endpoint', 'GET /api/io/modules/{id}'),
        node('handler', 'Function', 'io_module'),
      ],
      [edge('handler-edge', 'endpoint', 'handler', 'EndpointHandler')],
    )

    expect(paths).toHaveLength(1)
    expect(paths[0].source).toBe('heuristic')
    expect(paths[0].steps.map(step => step.role)).toEqual(['state', 'api-client', 'endpoint', 'handler', 'service', 'model'])
  })
})
