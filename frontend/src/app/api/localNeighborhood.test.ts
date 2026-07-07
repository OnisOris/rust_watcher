import { describe, expect, it } from 'vitest'
import { buildLocalNeighborhoodModel } from './localNeighborhood'
import type { EdgeType, GraphEdge, GraphNode, NodeType } from '../types'
import { makeEdge, makeNode } from './testGraphBuilders'

function node(id: string, type: NodeType = 'Function', file?: string): GraphNode {
  return makeNode({ id, type, file, label: id })
}

function edge(source: string, target: string, type: EdgeType = 'Calls', bundledCount?: number): GraphEdge {
  return makeEdge({ id: `${source}->${target}:${type}`, source, target, type, bundledCount })
}

describe('local neighborhood builder', () => {
  it('groups incoming relations by edge type', () => {
    const model = buildLocalNeighborhoodModel(
      [node('center'), node('caller'), node('importer'), node('api')],
      [
        edge('caller', 'center', 'Calls'),
        edge('importer', 'center', 'Imports'),
        edge('api', 'center', 'ApiCall'),
      ],
      'center',
      1,
    )

    expect(model.incomingGroups.map(group => [group.edgeType, group.nodes.map(item => item.id)])).toEqual([
      ['Calls', ['caller']],
      ['Imports', ['importer']],
      ['ApiCall', ['api']],
    ])
    expect(model.incomingCountByType).toEqual({ Calls: 1, Imports: 1, ApiCall: 1 })
  })

  it('groups outgoing relations by edge type', () => {
    const model = buildLocalNeighborhoodModel(
      [node('center'), node('callee'), node('used'), node('dto', 'Struct')],
      [
        edge('center', 'callee', 'Calls'),
        edge('center', 'used', 'Uses'),
        edge('center', 'dto', 'TypeReference', 3),
      ],
      'center',
      1,
    )

    expect(model.outgoingGroups.map(group => [group.edgeType, group.nodes.map(item => item.id)])).toEqual([
      ['Calls', ['callee']],
      ['Uses', ['used']],
      ['TypeReference', ['dto']],
    ])
    expect(model.outgoingCountByType).toEqual({ Calls: 1, Uses: 1, TypeReference: 3 })
  })

  it('detects related API, type, and test nodes inside the radius', () => {
    const model = buildLocalNeighborhoodModel(
      [
        node('center'),
        node('endpoint', 'Endpoint'),
        node('dto', 'Struct'),
        node('test', 'Function', 'tests/center_test.rs'),
      ],
      [
        edge('center', 'endpoint', 'ApiCall'),
        edge('endpoint', 'dto', 'TypeReference'),
        edge('test', 'center', 'Calls'),
      ],
      'center',
      2,
    )

    expect(model.relatedApiNodes.map(item => item.id)).toEqual(['endpoint'])
    expect(model.relatedTypeNodes.map(item => item.id)).toEqual(['dto'])
    expect(model.relatedTestNodes.map(item => item.id)).toEqual(['test'])
  })

  it('expands radius 1 vs radius 2', () => {
    const nodes = [node('center'), node('direct'), node('second'), node('outside')]
    const edges = [
      edge('center', 'direct'),
      edge('direct', 'second'),
      edge('outside', 'second'),
    ]

    const r1 = buildLocalNeighborhoodModel(nodes, edges, 'center', 1)
    const r2 = buildLocalNeighborhoodModel(nodes, edges, 'center', 2)

    expect(r1.visibleNodes.map(item => item.id).sort()).toEqual(['center', 'direct'])
    expect(r2.visibleNodes.map(item => item.id).sort()).toEqual(['center', 'direct', 'second'])
  })

  it('handles missing selected nodes', () => {
    const model = buildLocalNeighborhoodModel([node('a')], [], 'missing', 1)

    expect(model.centerNode).toBeNull()
    expect(model.incomingGroups).toEqual([])
    expect(model.visibleNodes).toEqual([])
  })

  it('limits dense radius 3 neighborhoods to top nodes by degree', () => {
    const nodes = [node('center')]
    const edges: GraphEdge[] = []
    for (let index = 0; index < 100; index += 1) {
      const direct = node(`direct-${index}`)
      const leaf = node(`leaf-${index}`)
      nodes.push(direct, leaf)
      edges.push(edge('center', direct.id, 'Calls', index === 0 ? 50 : 1))
      edges.push(edge(direct.id, leaf.id, 'Calls'))
    }

    const model = buildLocalNeighborhoodModel(nodes, edges, 'center', 3)

    expect(model.isDense).toBe(true)
    expect(model.denseNodeLimit).toBe(80)
    expect(model.visibleNodes).toHaveLength(80)
    expect(model.visibleNodes.some(item => item.id === 'center')).toBe(true)
    expect(model.visibleNodes.some(item => item.id === 'direct-0')).toBe(true)
  })
})
