import { describe, expect, it } from 'vitest'
import { buildDependencyMatrixModel, defaultDependencyMatrixLevel } from './dependencyMatrix'
import type { EdgeType, GraphEdge, GraphNode, NodeType } from '../types'
import { makeEdge, makeNode } from './testGraphBuilders'

function node(id: string, type: NodeType = 'Function', file?: string, label = id): GraphNode {
  return makeNode({ id, type, file, label })
}

function edge(id: string, source: string, target: string, type: EdgeType = 'Calls', bundledCount?: number): GraphEdge {
  return makeEdge({ id, source, target, type, bundledCount })
}

function cell(model: ReturnType<typeof buildDependencyMatrixModel>, source: string, target: string) {
  return model.cells.find(candidate => candidate.sourceGroupId === source && candidate.targetGroupId === target)
}

describe('dependency matrix builder', () => {
  it('groups dependencies at area level', () => {
    const model = buildDependencyMatrixModel(
      [
        node('ui', 'Component', 'frontend/src/App.tsx'),
        node('handler', 'Function', 'backend/main.py'),
      ],
      [edge('api', 'ui', 'handler', 'ApiCall')],
      { level: 'area' },
    )

    expect(model.level).toBe('area')
    expect(model.groups.map(group => group.id)).toEqual(['frontend', 'backend'])
    expect(cell(model, 'frontend', 'backend')).toMatchObject({
      count: 1,
      edgeTypes: { ApiCall: 1 },
    })
  })

  it('groups dependencies at module level', () => {
    const model = buildDependencyMatrixModel(
      [
        node('app', 'Component', 'frontend/src/App.tsx'),
        node('main', 'Function', 'backend/main.py'),
      ],
      [edge('api', 'app', 'main', 'ApiCall')],
      { level: 'module' },
    )

    expect(model.groups.map(group => group.label).sort()).toEqual(['backend/main', 'frontend/App'])
    expect(cell(model, 'module:frontend/App', 'module:backend/main')?.count).toBe(1)
  })

  it('groups dependencies at directory level', () => {
    const model = buildDependencyMatrixModel(
      [
        node('app', 'Component', 'frontend/src/App.tsx'),
        node('card', 'Component', 'frontend/src/components/Card.tsx'),
        node('main', 'Function', 'backend/main.py'),
      ],
      [
        edge('internal', 'app', 'card', 'Uses'),
        edge('api', 'card', 'main', 'ApiCall'),
      ],
      { level: 'directory' },
    )

    expect(model.groups.map(group => group.label).sort()).toEqual(['backend', 'frontend/src', 'frontend/src/components'])
    expect(cell(model, 'dir:frontend/src/components', 'dir:backend')?.count).toBe(1)
  })

  it('groups dependencies at file level and keeps underlying edge ids', () => {
    const model = buildDependencyMatrixModel(
      [
        node('app', 'Component', 'frontend/src/App.tsx'),
        node('main', 'Function', 'backend/main.py'),
      ],
      [edge('api', 'app', 'main', 'ApiCall')],
      { level: 'file' },
    )

    const selected = cell(model, 'file:frontend/src/App.tsx', 'file:backend/main.py')
    expect(selected).toMatchObject({
      count: 1,
      files: ['backend/main.py', 'frontend/src/App.tsx'],
      underlyingEdgeIds: ['api'],
    })
    expect(selected?.examples[0]).toMatchObject({
      sourceLabel: 'app',
      targetLabel: 'main',
      type: 'ApiCall',
    })
  })

  it('breaks down edge types inside a cell', () => {
    const model = buildDependencyMatrixModel(
      [
        node('caller', 'Component', 'frontend/src/App.tsx'),
        node('endpoint', 'Endpoint', 'backend/main.py', 'GET /api/person'),
        node('person', 'Struct', 'backend/main.py', 'Person'),
      ],
      [
        edge('api', 'caller', 'endpoint', 'ApiCall', 10),
        edge('data', 'caller', 'person', 'DataFlow', 7),
      ],
      { level: 'area' },
    )

    expect(cell(model, 'frontend', 'backend')?.edgeTypes).toEqual({ ApiCall: 10, DataFlow: 7 })
  })

  it('marks cycle cells when reverse dependencies exist', () => {
    const model = buildDependencyMatrixModel(
      [
        node('a', 'Function', 'src/a.rs'),
        node('b', 'Function', 'src/b.rs'),
      ],
      [
        edge('a-b', 'a', 'b', 'Imports'),
        edge('b-a', 'b', 'a', 'Imports'),
      ],
      { level: 'file' },
    )

    expect(cell(model, 'file:src/a.rs', 'file:src/b.rs')?.badges).toContain('cycle')
    expect(cell(model, 'file:src/b.rs', 'file:src/a.rs')?.badges).toContain('cycle')
  })

  it('marks strong dependencies', () => {
    const model = buildDependencyMatrixModel(
      [
        node('a', 'Function', 'frontend/src/api.ts'),
        node('b', 'Function', 'backend/main.py'),
      ],
      [edge('many', 'a', 'b', 'ApiCall', 20)],
      { level: 'area' },
    )

    expect(cell(model, 'frontend', 'backend')?.badges).toContain('strong')
  })

  it('marks backend to frontend dependencies as violations', () => {
    const model = buildDependencyMatrixModel(
      [
        node('backend', 'Function', 'backend/src/lib.rs'),
        node('frontend', 'Component', 'frontend/src/App.tsx'),
      ],
      [edge('bad-import', 'backend', 'frontend', 'Imports')],
      { level: 'area' },
    )

    expect(cell(model, 'backend', 'frontend')?.badges).toEqual(expect.arrayContaining(['violation', 'unexpected']))
  })

  it('marks backend to frontend violations at file level too', () => {
    const model = buildDependencyMatrixModel(
      [
        node('backend', 'Function', 'backend/src/lib.rs'),
        node('frontend', 'Component', 'frontend/src/App.tsx'),
      ],
      [edge('bad-import', 'backend', 'frontend', 'Imports')],
      { level: 'file' },
    )

    expect(cell(model, 'file:backend/src/lib.rs', 'file:frontend/src/App.tsx')?.badges).toEqual(expect.arrayContaining(['violation', 'unexpected']))
  })

  it('marks type-only cells and can hide type references', () => {
    const nodes = [
      node('a', 'Function', 'src/a.rs'),
      node('b', 'Struct', 'src/b.rs'),
    ]
    const edges = [edge('type', 'a', 'b', 'TypeReference')]
    const visible = buildDependencyMatrixModel(nodes, edges, { level: 'file' })
    const hidden = buildDependencyMatrixModel(nodes, edges, { level: 'file', includeTypeRefs: false })

    expect(cell(visible, 'file:src/a.rs', 'file:src/b.rs')?.badges).toContain('type-only')
    expect(hidden.cells).toEqual([])
  })

  it('defaults small projects to file level and medium projects to module level', () => {
    const small = [
      node('app', 'Component', 'frontend/src/App.tsx'),
      node('main', 'Function', 'backend/main.py'),
    ]
    const medium: GraphNode[] = [
      ...small,
      node('shared', 'Struct', 'shared/types.ts'),
      node('test', 'Function', 'tests/app.test.ts'),
    ]

    expect(defaultDependencyMatrixLevel(small)).toBe('file')
    expect(defaultDependencyMatrixLevel(medium)).toBe('module')
  })
})
