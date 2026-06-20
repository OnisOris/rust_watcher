import { describe, expect, it } from 'vitest'
import { deriveTraceHighlights, traceToMarkdown } from './trace'
import type { TraceExplanation } from '../types'

function trace(): TraceExplanation {
  return {
    id: 'trace',
    kind: 'Route',
    title: 'Route trace: GET /api/person',
    summary: 'Route GET /api/person is called by 1 frontend/QML node and handled by 1 backend function.',
    warnings: ['Multiple active endpoint implementations match this route.'],
    rootNodeId: 'endpoint',
    routeKey: 'GET /api/person',
    createdAt: '1',
    steps: [
      { id: 's1', kind: 'ApiRequest', nodeId: 'caller', edgeId: 'api', title: 'API request', description: 'ApiCall', confidence: 'Semantic', reachability: 'Active' },
      { id: 's2', kind: 'Endpoint', nodeId: 'endpoint', title: 'Endpoint', description: 'route', reachability: 'Active' },
      { id: 's3', kind: 'DetachedSource', nodeId: 'scratch', title: 'Detached source', description: 'detached', reachability: 'Detached', evidence: 'fn scratch() {}' },
    ],
  }
}

describe('trace helpers', () => {
  it('derives highlighted node and edge ids from trace steps', () => {
    const highlights = deriveTraceHighlights(trace())

    expect([...highlights!.nodeIds]).toEqual(['caller', 'endpoint', 'scratch'])
    expect([...highlights!.edgeIds]).toEqual(['api'])
  })

  it('serializes trace markdown with warnings, steps, confidence and reachability', () => {
    const markdown = traceToMarkdown(trace())

    expect(markdown).toContain('# Route trace: GET /api/person')
    expect(markdown).toContain('Multiple active endpoint implementations')
    expect(markdown).toContain('**ApiRequest** API request')
    expect(markdown).toContain('Confidence: Semantic')
    expect(markdown).toContain('Reachability: Detached')
  })
})
