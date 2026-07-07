import { describe, expect, it } from 'vitest'
import { buildLocalNeighborhoodModel } from './localNeighborhood'
import type { EdgeType, GraphEdge, GraphNode, NodeType } from '../types'

function node(id: string, type: NodeType = 'Function', file?: string): GraphNode {
  return { id, type, file, label: id, x: 0, y: 0, vx: 0, vy: 0 }
}

function edge(source: string, target: string, type: EdgeType = 'Calls'): GraphEdge {
  return { id: `${source}->${target}`, source, target, type }
}

describe('local neighborhood builder', () => {
  it('returns incoming and outgoing nodes for radius 1', () => {
    const model = buildLocalNeighborhoodModel(
      [node('center'), node('caller'), node('callee'), node('outside')],
      [edge('caller', 'center'), edge('center', 'callee'), edge('outside', 'caller')],
      'center',
      1,
    )

    expect(model.centerNode?.id).toBe('center')
    expect(model.incomingNodes.map(item => item.id)).toEqual(['caller'])
    expect(model.outgoingNodes.map(item => item.id)).toEqual(['callee'])
    expect(model.visibleNodes.map(item => item.id).sort()).toEqual(['callee', 'caller', 'center'])
  })

  it('expands radius 2 and separates API/type/test related nodes', () => {
    const model = buildLocalNeighborhoodModel(
      [
        node('center'),
        node('endpoint', 'Endpoint'),
        node('dto', 'Struct'),
        node('test', 'Function', 'tests/center_test.rs'),
      ],
      [edge('center', 'endpoint', 'ApiCall'), edge('endpoint', 'dto', 'TypeReference'), edge('test', 'center')],
      'center',
      2,
    )

    expect(model.visibleNodes.map(item => item.id).sort()).toEqual(['center', 'dto', 'endpoint', 'test'])
    expect(model.relatedApiNodes.map(item => item.id)).toEqual(['endpoint'])
    expect(model.relatedTypeNodes.map(item => item.id)).toEqual(['dto'])
    expect(model.relatedTestNodes.map(item => item.id)).toEqual(['test'])
  })

  it('handles missing selected nodes', () => {
    const model = buildLocalNeighborhoodModel([node('a')], [], 'missing', 1)

    expect(model.centerNode).toBeNull()
    expect(model.visibleNodes).toEqual([])
  })
})
