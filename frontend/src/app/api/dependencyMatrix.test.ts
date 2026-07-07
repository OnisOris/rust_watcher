import { describe, expect, it } from 'vitest'
import { buildDependencyMatrixModel } from './dependencyMatrix'
import type { ProjectMapModel } from '../views/architecture/architectureTypes'

describe('dependency matrix builder', () => {
  it('builds source-target cells while preserving edge metadata', () => {
    const projectMap: ProjectMapModel = {
      groups: [
        { id: 'frontend', label: 'Frontend', kind: 'frontend', nodeIds: ['a'], fileCount: 1, symbolCount: 0, incomingCount: 0, outgoingCount: 2 },
        { id: 'backend', label: 'Backend', kind: 'backend', nodeIds: ['b'], fileCount: 1, symbolCount: 0, incomingCount: 2, outgoingCount: 0 },
      ],
      edges: [
        {
          id: 'frontend->backend',
          sourceGroupId: 'frontend',
          targetGroupId: 'backend',
          count: 2,
          edgeTypes: { ApiCall: 1, Calls: 1 },
          underlyingEdgeIds: ['api', 'call'],
        },
      ],
      nodeToGroup: new Map([['a', 'frontend'], ['b', 'backend']]),
    }

    const matrix = buildDependencyMatrixModel(projectMap)

    expect(matrix.groups).toBe(projectMap.groups)
    expect(matrix.cells).toEqual([
      {
        sourceGroupId: 'frontend',
        targetGroupId: 'backend',
        count: 2,
        edgeTypes: { ApiCall: 1, Calls: 1 },
        underlyingEdgeIds: ['api', 'call'],
      },
    ])
  })

  it('ignores zero-count cells', () => {
    const matrix = buildDependencyMatrixModel({
      groups: [],
      edges: [{ id: 'empty', sourceGroupId: 'a', targetGroupId: 'b', count: 0, edgeTypes: {}, underlyingEdgeIds: [] }],
      nodeToGroup: new Map(),
    })

    expect(matrix.cells).toEqual([])
  })
})
