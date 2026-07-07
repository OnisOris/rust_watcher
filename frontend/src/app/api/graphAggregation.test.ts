import { describe, expect, it } from 'vitest'
import { buildProjectMapModel } from './graphAggregation'
import type { EdgeType, GraphEdge, GraphNode, NodeType } from '../types'
import { makeEdge, makeFileNode, makeNode } from './testGraphBuilders'

function node(id: string, file: string | undefined, type: NodeType = 'Function', language?: string): GraphNode {
  return makeNode({
    id,
    file,
    type,
    label: id,
    language,
  })
}

function edge(id: string, source: string, target: string, type: EdgeType = 'Calls'): GraphEdge {
  return makeEdge({ id, source, target, type })
}

describe('project graph aggregation', () => {
  it('classifies language and path heuristics into architecture groups', () => {
    const model = buildProjectMapModel([
      makeFileNode('frontend/src/App.tsx', 'typescript'),
      makeNode({ id: 'qml-window', type: 'Object', label: 'ApplicationWindow', file: 'qml/Main.qml', language: 'qml' }),
      makeFileNode('src/main.rs', 'rust'),
      makeFileNode('backend/main.py', 'python'),
      makeFileNode('shared/protocol/types.rs', 'rust'),
      makeFileNode('src/domain/types/person.ts', 'typescript'),
      makeFileNode('tests/api_test.rs', 'rust'),
      { ...makeNode({ id: 'serde', type: 'ExternalCrate', label: 'serde' }), reachability: 'External' as const },
    ], [])

    expect(model.nodeToGroup.get('file:frontend/src/App.tsx')).toBe('frontend')
    expect(model.nodeToGroup.get('qml-window')).toBe('frontend')
    expect(model.nodeToGroup.get('file:src/main.rs')).toBe('backend')
    expect(model.nodeToGroup.get('file:backend/main.py')).toBe('backend')
    expect(model.nodeToGroup.get('file:shared/protocol/types.rs')).toBe('shared')
    expect(model.nodeToGroup.get('file:src/domain/types/person.ts')).toBe('shared')
    expect(model.nodeToGroup.get('file:tests/api_test.rs')).toBe('tests')
    expect(model.nodeToGroup.get('serde')).toBe('external')
  })

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
    expect(model.groups.map(group => group.label)).toEqual([
      'Frontend / UI',
      'Backend / Services',
      'Shared / Protocol',
      'Tests',
      'External Dependencies',
      'Generated / Mocks',
    ])
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
    expect(aggregated?.examples?.[0]).toMatchObject({
      sourceLabel: 'fe1',
      targetLabel: 'be',
      type: 'ApiCall',
    })
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

  it('groups by language', () => {
    const model = buildProjectMapModel([
      node('app', 'frontend/src/App.tsx', 'Component', 'typescript'),
      node('view', 'qml/Main.qml', 'Object', 'qml'),
      node('backend', 'src/main.rs', 'Function', 'rust'),
    ], [], { grouping: 'language' })

    expect(model.grouping).toBe('language')
    expect(model.groups.map(group => group.label).sort()).toEqual(['QML', 'Rust', 'TypeScript'])
    expect(model.nodeToGroup.get('app')).toBe('language:typescript')
  })

  it('groups by directory', () => {
    const model = buildProjectMapModel([
      node('app', 'frontend/src/App.tsx', 'Component'),
      node('button', 'frontend/src/components/Button.tsx', 'Component'),
      node('backend', 'backend/main.py', 'Function'),
    ], [], { grouping: 'directory' })

    expect(model.grouping).toBe('directory')
    expect(model.groups.map(group => group.label).sort()).toEqual(['backend', 'frontend/src', 'frontend/src/components'])
  })

  it('groups by module and runtime', () => {
    const model = buildProjectMapModel([
      { ...node('app', 'frontend/src/App.tsx', 'Component'), module: 'frontend::App' },
      { ...node('backend', 'src/main.rs', 'Function', 'rust'), module: 'example::src' },
    ], [], { grouping: 'module' })
    const runtime = buildProjectMapModel([
      node('qml', 'qml/Main.qml', 'Object', 'qml'),
      node('rust', 'src/main.rs', 'Function', 'rust'),
    ], [], { grouping: 'runtime' })

    expect(model.groups.map(group => group.label).sort()).toEqual(['example/src', 'frontend/App'])
    expect(runtime.groups.map(group => group.label).sort()).toEqual(['QML Runtime', 'Rust Service Runtime'])
  })

  it('adds language breakdown, top files, and key symbols to groups', () => {
    const model = buildProjectMapModel([
      node('file', 'frontend/src/App.tsx', 'File', 'typescript'),
      { ...node('window', 'qml/Main.qml', 'Object', 'qml'), connections: 10 },
      { ...node('hook', 'frontend/src/App.tsx', 'Hook', 'typescript'), connections: 4 },
    ], [])
    const group = model.groups.find(item => item.id === 'frontend')

    expect(group?.languageBreakdown).toEqual({ TypeScript: 2, QML: 1 })
    expect(group?.topFiles).toEqual(['frontend/src/App.tsx', 'qml/Main.qml'])
    expect(group?.keySymbols).toEqual(['window', 'hook'])
  })

  it('marks small architecture maps as auto-expanded and keeps child groups', () => {
    const model = buildProjectMapModel([
      node('app', 'frontend/src/App.tsx', 'Component'),
      node('backend', 'src/main.rs', 'Function'),
    ], [edge('api', 'app', 'backend', 'ApiCall')])

    expect(model.autoExpanded).toBe(true)
    expect(model.groups).toHaveLength(2)
    expect(model.groups.find(group => group.id === 'frontend')?.children?.length).toBeGreaterThan(0)
    expect(model.groups.find(group => group.id === 'backend')?.children?.length).toBeGreaterThan(0)
  })
})
