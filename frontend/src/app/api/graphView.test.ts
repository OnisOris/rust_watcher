import { describe, expect, it } from 'vitest'
import {
  applyCollapsedGroups,
  applyDepthFilter,
  applyEdgeVisibilityLevel,
  applyGraphFilters,
  buildCollapsedGroupStats,
  buildRouteFlowGraph,
  bundleEdges,
  depthControlKind,
  depthOptionsForMode,
  visibleNodeIdsForDepth,
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

function ids(ids: Set<string>) {
  return [...ids].sort()
}

describe('graph view helpers', () => {
  it('uses Scope labels and hierarchy tooltips for Architecture/Modules depth', () => {
    expect(depthControlKind('Macro')).toBe('Scope')
    expect(depthControlKind('Meso')).toBe('Scope')
    expect(depthOptionsForMode('Macro').map(option => option.title)).toEqual([
      'Top-level project scopes',
      'Scopes and files/modules',
      'Scopes, files and important symbols',
      'Full current graph',
    ])
  })

  it('uses Radius labels and neighborhood tooltips for local/call-flow depth', () => {
    expect(depthControlKind('Micro')).toBe('Radius')
    expect(depthControlKind('CallFlow')).toBe('Radius')
    expect(depthOptionsForMode('CallFlow').map(option => option.title)).toEqual([
      'One-hop neighborhood',
      'Two-hop neighborhood',
      'Three-hop neighborhood',
      'Full current mode',
    ])
  })

  it('Architecture depth 1 does not choose main as a center', () => {
    const nodes = [
      node('workspace', undefined, 'Module'),
      node('rust', 'rust', 'Module'),
      node('file', 'rust', 'File'),
      { ...node('main', 'rust', 'Function'), label: 'main' },
      node('endpoint', undefined, 'Endpoint'),
    ]
    const edges: GraphEdge[] = [
      { id: 'workspace-rust', source: 'workspace', target: 'rust', type: 'Contains' },
      { id: 'rust-file', source: 'rust', target: 'file', type: 'Contains' },
      { id: 'file-main', source: 'file', target: 'main', type: 'Contains' },
      { id: 'file-endpoint', source: 'file', target: 'endpoint', type: 'Contains' },
      { id: 'main-endpoint', source: 'main', target: 'endpoint', type: 'Calls' },
    ]

    expect(ids(visibleNodeIdsForDepth(nodes, edges, 'Macro', 1, null))).toEqual(['rust', 'workspace'])
  })

  it('Architecture depth expands by hierarchy levels', () => {
    const publicSymbol = {
      ...node('public-service', 'rust', 'Function'),
      visibility: 'pub' as const,
      signature: 'pub fn service()',
    }
    const nodes = [
      node('workspace', undefined, 'Module'),
      node('rust', 'rust', 'Module'),
      node('file', 'rust', 'File'),
      node('endpoint', undefined, 'Endpoint'),
      publicSymbol,
      { ...node('main', 'rust', 'Function'), label: 'main' },
    ]
    const edges: GraphEdge[] = [
      { id: 'workspace-rust', source: 'workspace', target: 'rust', type: 'Contains' },
      { id: 'rust-file', source: 'rust', target: 'file', type: 'Contains' },
      { id: 'file-endpoint', source: 'file', target: 'endpoint', type: 'Contains' },
      { id: 'file-public', source: 'file', target: 'public-service', type: 'Contains' },
      { id: 'file-main', source: 'file', target: 'main', type: 'Contains' },
    ]

    expect(ids(visibleNodeIdsForDepth(nodes, edges, 'Macro', 1, null))).toEqual(['rust', 'workspace'])
    expect(ids(visibleNodeIdsForDepth(nodes, edges, 'Macro', 2, null))).toEqual(['file', 'rust', 'workspace'])
    expect(ids(visibleNodeIdsForDepth(nodes, edges, 'Macro', 3, null))).toEqual(['endpoint', 'file', 'public-service', 'rust', 'workspace'])
  })

  it('Local Symbol depth expands around the selected node', () => {
    const nodes = [
      node('selected', 'typescript', 'Function'),
      node('callee', 'typescript', 'Function'),
      node('model', 'typescript', 'Interface'),
      node('outside', 'typescript', 'Function'),
    ]
    const edges: GraphEdge[] = [
      { id: 'call', source: 'selected', target: 'callee', type: 'Calls' },
      { id: 'type', source: 'callee', target: 'model', type: 'TypeReference' },
      { id: 'outside', source: 'outside', target: 'model', type: 'Calls' },
    ]

    expect(ids(visibleNodeIdsForDepth(nodes, edges, 'Micro', 1, 'selected'))).toEqual(['callee', 'selected'])
    expect(ids(visibleNodeIdsForDepth(nodes, edges, 'Micro', 2, 'selected'))).toEqual(['callee', 'model', 'selected'])
  })

  it('Call Flow depth with a selected node expands around that node', () => {
    const nodes = [
      node('caller', 'typescript', 'Component'),
      node('handler', 'rust', 'Function'),
      node('service', 'rust', 'Function'),
      node('endpoint', undefined, 'Endpoint'),
      node('outside', 'rust', 'Function'),
    ]
    const edges: GraphEdge[] = [
      { id: 'api', source: 'caller', target: 'endpoint', type: 'ApiCall' },
      { id: 'handler', source: 'endpoint', target: 'handler', type: 'EndpointHandler' },
      { id: 'call', source: 'handler', target: 'service', type: 'Calls' },
      { id: 'outside', source: 'outside', target: 'service', type: 'Calls' },
    ]

    expect(ids(visibleNodeIdsForDepth(nodes, edges, 'CallFlow', 1, 'handler'))).toEqual(['endpoint', 'handler', 'service'])
    expect(ids(visibleNodeIdsForDepth(nodes, edges, 'CallFlow', 2, 'handler'))).toEqual(['caller', 'endpoint', 'handler', 'outside', 'service'])
  })

  it('Local Symbol without a selected node does not randomly center on main', () => {
    const nodes = [
      { ...node('main', 'rust', 'Function'), label: 'main' },
      node('service', 'rust', 'Function'),
    ]
    const edges = [edge('main', 'service')]

    expect(ids(visibleNodeIdsForDepth(nodes, edges, 'Micro', 1, null))).toEqual([])
  })

  it('applies depth filtering to App-level nodes and edges together', () => {
    const nodes = [
      node('workspace', undefined, 'Module'),
      node('rust', 'rust', 'Module'),
      node('file', 'rust', 'File'),
      node('endpoint', undefined, 'Endpoint'),
      { ...node('main', 'rust', 'Function'), label: 'main' },
    ]
    const edges: GraphEdge[] = [
      { id: 'workspace-rust', source: 'workspace', target: 'rust', type: 'Contains' },
      { id: 'rust-file', source: 'rust', target: 'file', type: 'Contains' },
      { id: 'file-endpoint', source: 'file', target: 'endpoint', type: 'Contains' },
      { id: 'main-endpoint', source: 'main', target: 'endpoint', type: 'Calls' },
    ]

    const depthGraph = applyDepthFilter({ nodes, edges }, 'Macro', 2, null)

    expect(depthGraph.nodes.map(node => node.id)).toEqual(['workspace', 'rust', 'file'])
    expect(depthGraph.edges.map(edge => edge.id)).toEqual(['workspace-rust', 'rust-file'])
  })

  it('applies selected Local Symbol radius before graph consumers read counts', () => {
    const nodes = [
      node('selected', 'typescript', 'Function'),
      node('callee', 'typescript', 'Function'),
      node('model', 'typescript', 'Interface'),
    ]
    const edges: GraphEdge[] = [
      { id: 'call', source: 'selected', target: 'callee', type: 'Calls' },
      { id: 'type', source: 'callee', target: 'model', type: 'TypeReference' },
    ]

    const depthGraph = applyDepthFilter({ nodes, edges }, 'Micro', 1, 'selected')

    expect(depthGraph.nodes.map(node => node.id)).toEqual(['selected', 'callee'])
    expect(depthGraph.edges.map(edge => edge.id)).toEqual(['call'])
  })

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
