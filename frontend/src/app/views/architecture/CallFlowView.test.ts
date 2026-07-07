import { describe, expect, it } from 'vitest'
import type { EdgeType, GraphEdge, GraphNode, NodeType } from '../../types'
import { buildCallFlowPaths } from './CallFlowView'

function node(id: string, type: NodeType, label = id): GraphNode {
  return {
    id,
    type,
    label,
    file: `src/${id}.rs`,
    x: 0,
    y: 0,
    vx: 0,
    vy: 0,
  }
}

function edge(id: string, source: string, target: string, type: EdgeType): GraphEdge {
  return { id, source, target, type }
}

describe('buildCallFlowPaths', () => {
  it('builds backend-only paths from endpoint handler edges', () => {
    const paths = buildCallFlowPaths(
      [
        node('endpoint', 'Endpoint', 'GET /api/io/modules/{id}'),
        node('handler', 'Function', 'io_module'),
        node('service', 'Function', 'load_module'),
        node('dto', 'Struct', 'ModuleDto'),
      ],
      [
        edge('handler-edge', 'endpoint', 'handler', 'EndpointHandler'),
        edge('service-edge', 'handler', 'service', 'Calls'),
        edge('model-edge', 'service', 'dto', 'TypeReference'),
      ],
    )

    expect(paths).toHaveLength(1)
    expect(paths[0].label).toBe('GET /api/io/modules/{id} -> io_module')
    expect(paths[0].steps.map(step => step.role)).toEqual(['endpoint', 'handler', 'service', 'model'])
    expect(paths[0].edgeIds).toEqual(['handler-edge', 'service-edge', 'model-edge'])
  })
})
