import { describe, expect, it } from 'vitest'
import {
  applyCollapsedGroups,
  applyEdgeVisibilityLevel,
  applyGraphFilters,
  buildCollapsedGroupStats,
  buildRouteFlowGraph,
  bundleEdges,
} from './graphView'
import type { DiagnosticRecord, EdgeType, GraphEdge, GraphFilters, GraphNode, LanguageFilter, NodeType } from '../types'

const allNodeTypes = new Set<NodeType>(['File', 'Module', 'Struct', 'Class', 'Object', 'Enum', 'Trait', 'Impl', 'Function', 'Method', 'Component', 'Hook', 'Interface', 'TypeAlias', 'Property', 'Signal', 'Handler', 'Endpoint', 'Macro', 'ExternalCrate'])
const allEdgeTypes = new Set<EdgeType>(['Contains', 'Imports', 'Uses', 'Calls', 'Renders', 'ApiCall', 'EndpointHandler', 'Implements', 'TypeReference', 'DataFlow', 'ModDeclaration', 'ExternalDependency'])

function filters(languages: LanguageFilter[]): GraphFilters {
  return {
    nodeTypes: allNodeTypes,
    edgeTypes: allEdgeTypes,
    languages: new Set(languages),
    edgeVisibility: 'All',
    showTests: true,
    showExternal: true,
    showDetached: false,
    onlyPublicAPI: false,
    depth: 'full',
    onlyCurrentFile: false,
  }
}

function node(id: string, language: string | undefined, type: NodeType = 'Function'): GraphNode {
  return {
    id,
    language,
    type,
    label: id,
    x: 0,
    y: 0,
    vx: 0,
    vy: 0,
  }
}

function edge(source: string, target: string): GraphEdge {
  return { id: `${source}->${target}`, source, target, type: 'Calls' }
}

describe('graph view helpers', () => {
  it('filters visible graph by language without deleting source graph data', () => {
    const nodes = [
      node('rust', 'rust'),
      node('ts', 'typescript'),
      node('py', 'python'),
      node('qml', 'qml'),
      node('endpoint', undefined, 'Endpoint'),
    ]
    const edges = [edge('rust', 'ts'), edge('ts', 'endpoint'), edge('py', 'endpoint')]

    const visible = applyGraphFilters({ nodes, edges }, filters(['typescript', 'endpoints']))

    expect(visible.nodes.map(n => n.id)).toEqual(['ts', 'endpoint'])
    expect(visible.edges.map(e => e.id)).toEqual(['ts->endpoint'])
    expect(nodes).toHaveLength(5)
  })

  it('treats JavaScript as the TypeScript/JavaScript language filter', () => {
    const visible = applyGraphFilters(
      { nodes: [node('js', 'javascript'), node('rust', 'rust')], edges: [] },
      filters(['typescript']),
    )
    expect(visible.nodes.map(n => n.id)).toEqual(['js'])
  })

  it('hides detached nodes unless the detached filter is enabled', () => {
    const detached = { ...node('scratch', 'rust', 'File'), reachability: 'Detached' as const }
    const active = { ...node('main', 'rust', 'File'), reachability: 'Active' as const }

    expect(applyGraphFilters({ nodes: [active, detached], edges: [] }, filters(['rust'])).nodes.map(n => n.id)).toEqual(['main'])

    const withDetached = filters(['rust'])
    withDetached.showDetached = true
    expect(applyGraphFilters({ nodes: [active, detached], edges: [] }, withDetached).nodes.map(n => n.id)).toEqual(['main', 'scratch'])
  })

  it('collapses a file group while keeping external edges readable', () => {
    const nodes = [
      node('file:a', 'typescript', 'File'),
      node('child:a', 'typescript', 'Component'),
      node('endpoint', undefined, 'Endpoint'),
    ]
    const edges: GraphEdge[] = [
      { id: 'contains', source: 'file:a', target: 'child:a', type: 'Contains' },
      { id: 'api', source: 'child:a', target: 'endpoint', type: 'ApiCall' },
    ]

    const visible = applyCollapsedGroups({ nodes, edges }, new Set(['file:a']))

    expect(visible.nodes.map(n => n.id)).toEqual(['file:a', 'endpoint'])
    expect(visible.edges).toContainEqual({
      id: 'ApiCall:file:a->endpoint:api',
      source: 'file:a',
      target: 'endpoint',
      type: 'ApiCall',
    })
  })

  it('bundles duplicate external edges after collapse', () => {
    const nodes = [
      node('file:a', 'typescript', 'File'),
      node('child:a', 'typescript', 'Component'),
      node('child:b', 'typescript', 'Hook'),
      node('endpoint', undefined, 'Endpoint'),
    ]
    const collapsed = applyCollapsedGroups({ nodes, edges: [
      { id: 'contains-a', source: 'file:a', target: 'child:a', type: 'Contains' },
      { id: 'contains-b', source: 'file:a', target: 'child:b', type: 'Contains' },
      { id: 'api-a', source: 'child:a', target: 'endpoint', type: 'ApiCall' },
      { id: 'api-b', source: 'child:b', target: 'endpoint', type: 'ApiCall' },
    ] }, new Set(['file:a']))
    const bundled = bundleEdges(collapsed.edges)

    expect(bundled.find(edge => edge.type === 'ApiCall')?.bundledCount).toBe(2)
  })

  it('bundles visually duplicate edges and preserves count', () => {
    const bundled = bundleEdges([
      { id: 'a', source: 's', target: 't', type: 'Calls' },
      { id: 'b', source: 's', target: 't', type: 'Calls' },
      { id: 'c', source: 's', target: 't', type: 'Uses' },
    ])

    expect(bundled).toHaveLength(2)
    expect(bundled.find(edge => edge.type === 'Calls')?.bundledCount).toBe(2)
    expect(bundled.find(edge => edge.type === 'Calls')?.bundledEdgeIds).toEqual(['a', 'b'])
  })

  it('does not bundle different DataFlow kinds between the same nodes', () => {
    const bundled = bundleEdges([
      { id: 'request', source: 's', target: 't', type: 'DataFlow', dataFlowKind: 'ApiRequest', label: 'fetch' },
      { id: 'response', source: 's', target: 't', type: 'DataFlow', dataFlowKind: 'ApiResponse', label: 'json' },
      { id: 'request-2', source: 's', target: 't', type: 'DataFlow', dataFlowKind: 'ApiRequest', label: 'fetch' },
    ])

    expect(bundled).toHaveLength(2)
    expect(bundled.find(edge => edge.dataFlowKind === 'ApiRequest')?.bundledCount).toBe(2)
    expect(bundled.find(edge => edge.dataFlowKind === 'ApiResponse')?.bundledCount).toBeUndefined()
  })

  it('Essential mode hides noisy fallback edges', () => {
    const essential = applyEdgeVisibilityLevel([
      { id: 'contains', source: 'a', target: 'b', type: 'Contains' },
      { id: 'semantic-call', source: 'a', target: 'c', type: 'Calls', confidence: 'Semantic' },
      { id: 'fallback-call', source: 'a', target: 'd', type: 'Calls', confidence: 'SyntaxFallback' },
      { id: 'imports', source: 'a', target: 'e', type: 'Imports' },
    ], {
      edgeTypes: allEdgeTypes,
      edgeVisibility: 'Essential',
    })

    expect(essential.map(edge => edge.id)).toEqual(['contains', 'semantic-call'])
  })

  it('Route Flow keeps ApiCall -> Endpoint -> Handler chain', () => {
    const nodes = [
      node('component', 'typescript', 'Component'),
      node('endpoint', undefined, 'Endpoint'),
      { ...node('detached-endpoint', undefined, 'Endpoint'), reachability: 'Detached' as const },
      node('handler', 'rust', 'Function'),
      node('service', 'rust', 'Function'),
      node('model', 'rust', 'Struct'),
      node('noise', 'rust', 'Function'),
    ]
    const edges: GraphEdge[] = [
      { id: 'api', source: 'component', target: 'endpoint', type: 'ApiCall' },
      { id: 'detached-api', source: 'component', target: 'detached-endpoint', type: 'ApiCall' },
      { id: 'request', source: 'component', target: 'endpoint', type: 'DataFlow', dataFlowKind: 'ApiRequest' },
      { id: 'handler', source: 'endpoint', target: 'handler', type: 'EndpointHandler' },
      { id: 'handler-response', source: 'handler', target: 'endpoint', type: 'DataFlow', dataFlowKind: 'ApiResponse' },
      { id: 'call', source: 'handler', target: 'service', type: 'Calls' },
      { id: 'response', source: 'service', target: 'model', type: 'DataFlow', dataFlowKind: 'ReturnValue' },
      { id: 'noise', source: 'noise', target: 'service', type: 'Calls' },
    ]

    const route = buildRouteFlowGraph({ nodes, edges })

    expect(route.nodes.map(n => n.id)).toEqual(['component', 'endpoint', 'handler', 'service', 'model'])
    expect(route.edges.map(e => e.id)).toEqual(['api', 'request', 'handler', 'handler-response', 'call', 'response'])
  })

  it('collapsed group stats count hidden diagnostics', () => {
    const nodes = [node('file', 'qml', 'File'), node('child', 'qml', 'Object'), node('external', 'rust')]
    const edges: GraphEdge[] = [
      { id: 'contains', source: 'file', target: 'child', type: 'Contains' },
      { id: 'api', source: 'child', target: 'external', type: 'ApiCall' },
    ]
    const diagnostics: DiagnosticRecord = {
      id: 'd',
      language: 'qml',
      file: 'Main.qml',
      severity: 'Error',
      message: 'broken',
      relatedNodeIds: ['child'],
    }

    const stats = buildCollapsedGroupStats(
      { nodes, edges },
      new Set(['file']),
      new Map([['child', [diagnostics]]]),
    )

    expect(stats.get('file')?.hiddenNodeCount).toBe(1)
    expect(stats.get('file')?.hiddenDiagnosticCount).toBe(1)
    expect(stats.get('file')?.outgoingEdgeTypes).toEqual(['ApiCall'])
  })

})
