import { describe, expect, it } from 'vitest'
import { buildHotspotIssues } from './hotspots'
import type { EdgeType, GraphEdge, GraphNode, NodeType } from '../types'

function node(id: string, type: NodeType = 'Function', file?: string): GraphNode {
  return { id, type, file, label: id, x: 0, y: 0, vx: 0, vy: 0 }
}

function edge(id: string, source: string, target: string, type: EdgeType = 'Calls', bundledCount?: number): GraphEdge {
  return { id, source, target, type, bundledCount }
}

describe('hotspot builder', () => {
  it('detects god modules and high-degree utilities', () => {
    const issues = buildHotspotIssues(
      [node('app.rs', 'File', 'crates/app/src/app.rs'), node('utils', 'Function', 'frontend/src/lib/utils.ts'), node('caller')],
      [
        edge('god', 'caller', 'app.rs', 'Calls', 90),
        edge('noise', 'utils', 'caller', 'Calls', 30),
      ],
    )

    expect(issues.some(issue => issue.kind === 'god-module' && issue.nodeId === 'app.rs')).toBe(true)
    expect(issues.some(issue => issue.kind === 'noise-utility' && issue.nodeId === 'utils')).toBe(true)
  })

  it('detects central API clients, unused endpoints and missing handlers', () => {
    const nodes = [
      node('requestJson', 'Function', 'frontend/src/api/client.ts'),
      node('page', 'Component', 'frontend/src/Page.tsx'),
      node('endpoint', 'Endpoint'),
      node('unused', 'Endpoint'),
    ]
    const edges = [
      edge('api', 'page', 'endpoint', 'ApiCall'),
      edge('api-client', 'requestJson', 'endpoint', 'ApiCall', 8),
    ]

    const issues = buildHotspotIssues(nodes, edges)

    expect(issues.some(issue => issue.kind === 'central-api-client' && issue.nodeId === 'requestJson')).toBe(true)
    expect(issues.some(issue => issue.kind === 'unused-endpoint' && issue.nodeId === 'unused')).toBe(true)
    expect(issues.some(issue => issue.kind === 'frontend-call-without-handler')).toBe(true)
  })

  it('detects simple mutual dependency cycle candidates', () => {
    const issues = buildHotspotIssues(
      [node('a'), node('b')],
      [edge('a-b', 'a', 'b', 'Imports'), edge('b-a', 'b', 'a', 'Imports')],
    )

    expect(issues.some(issue => issue.kind === 'cycle-candidate')).toBe(true)
  })
})
