import { describe, expect, it } from 'vitest'
import { buildProjectMapModel } from './graphAggregation'
import type { EdgeType, GraphEdge, GraphNode, NodeType } from '../types'

function node(id: string, file: string | undefined, type: NodeType = 'Function'): GraphNode {
  return {
    id,
    file,
    type,
    label: id,
    x: 0,
    y: 0,
    vx: 0,
    vy: 0,
  }
}

function edge(id: string, source: string, target: string, type: EdgeType = 'Calls'): GraphEdge {
  return { id, source, target, type }
}

describe('project graph aggregation', () => {
  it('groups frontend/backend/shared/tests/external and generated nodes', () => {
    const nodes = [
      node('frontend', 'frontend/src/App.tsx', 'File'),
      node('backend', 'crates/web-server/src/main.rs', 'File'),
      node('shared', 'shared/protocol/types.rs', 'File'),
      node('test', 'frontend/src/App.test.ts', 'File'),
      { ...node('external', undefined, 'ExternalCrate'), reachability: 'External' as const },
      { ...node('generated', 'frontend/generated/api.ts', 'File'), reachability: 'Generated' as const },
    ]

    const model = buildProjectMapModel(nodes, [])

    expect(model.groups.map(group => group.id)).toEqual(['frontend', 'backend', 'shared', 'tests', 'external', 'generated'])
    expect(model.nodeToGroup.get('frontend')).toBe('frontend')
    expect(model.nodeToGroup.get('backend')).toBe('backend')
    expect(model.nodeToGroup.get('shared')).toBe('shared')
  })

  it('aggregates cross-group edges and ignores internal edges', () => {
    const nodes = [
      node('fe1', 'frontend/src/api/client.ts', 'File'),
      node('fe2', 'frontend/src/components/Button.tsx', 'File'),
      node('be', 'crates/web-server/src/main.rs', 'File'),
    ]
    const edges: GraphEdge[] = [
      edge('internal', 'fe1', 'fe2', 'Imports'),
      edge('api', 'fe1', 'be', 'ApiCall'),
      { ...edge('bundled', 'fe2', 'be', 'Calls'), bundledCount: 3, bundledEdgeIds: ['a', 'b', 'c'] },
    ]

    const model = buildProjectMapModel(nodes, edges)
    const aggregated = model.edges.find(item => item.sourceGroupId === 'frontend' && item.targetGroupId === 'backend')

    expect(model.edges).toHaveLength(1)
    expect(aggregated?.count).toBe(4)
    expect(aggregated?.edgeTypes.ApiCall).toBe(1)
    expect(aggregated?.edgeTypes.Calls).toBe(3)
    expect(aggregated?.underlyingEdgeIds).toEqual(['api', 'a', 'b', 'c'])
  })

  it('does not treat containment as a cross-area dependency', () => {
    const nodes = [
      node('backend', 'crates/web-server/src/main.rs', 'File'),
      node('shared', 'shared/types.rs', 'File'),
    ]
    const model = buildProjectMapModel(nodes, [
      edge('contains', 'backend', 'shared', 'Contains'),
      edge('mod', 'backend', 'shared', 'ModDeclaration'),
    ])

    expect(model.edges).toEqual([])
  })

  it('can exclude tests and external nodes from grouping', () => {
    const model = buildProjectMapModel([
      node('test', 'tests/api_test.rs', 'File'),
      { ...node('ext', undefined, 'ExternalCrate'), reachability: 'External' as const },
      node('be', 'crates/web-server/src/main.rs', 'File'),
    ], [], { includeTests: false, includeExternal: false })

    expect(model.groups.map(group => group.id)).toEqual(['backend'])
    expect(model.nodeToGroup.has('test')).toBe(false)
    expect(model.nodeToGroup.has('ext')).toBe(false)
  })
})
