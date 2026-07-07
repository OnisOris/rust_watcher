import { describe, expect, it } from 'vitest'
import { buildHotspotIssues } from './hotspots'
import type { EdgeType, GraphEdge, GraphNode, NodeType } from '../types'
import { makeEdge, makeNode } from './testGraphBuilders'

function node(id: string, type: NodeType = 'Function', file?: string, label = id): GraphNode {
  return makeNode({ id, type, file, label })
}

function edge(id: string, source: string, target: string, type: EdgeType = 'Calls', bundledCount?: number): GraphEdge {
  return makeEdge({ id, source, target, type, bundledCount })
}

describe('hotspot builder', () => {
  it('detects god modules with normalized degree thresholds', () => {
    const nodes: GraphNode[] = [node('hub', 'File', 'src/hub.rs', 'hub.rs')]
    const edges: GraphEdge[] = []
    for (let index = 0; index < 40; index += 1) {
      const leaf = node(`leaf-${index}`, 'Function', `src/leaf_${index}.rs`)
      nodes.push(leaf)
      edges.push(edge(`hub-${index}`, 'hub', leaf.id, 'Calls'))
    }

    const issues = buildHotspotIssues(nodes, edges)

    expect(issues.find(issue => issue.kind === 'god-module' && issue.nodeId === 'hub')).toMatchObject({
      severity: 'warning',
      confidence: 'medium',
      connections: 40,
      files: ['src/hub.rs'],
    })
  })

  it('does not create a visible cycle warning for TypeReference-only symbol mutual references', () => {
    const issues = buildHotspotIssues(
      [
        node('get_person', 'Function', 'backend/main.py'),
        node('person', 'Struct', 'src/main.rs', 'Person'),
      ],
      [
        edge('a-b', 'get_person', 'person', 'TypeReference'),
        edge('b-a', 'person', 'get_person', 'TypeReference'),
      ],
    )

    expect(issues.some(issue => issue.kind === 'cycle-candidate')).toBe(false)
  })

  it('can include low-confidence symbol mutual references when explicitly requested', () => {
    const issues = buildHotspotIssues(
      [
        node('get_person', 'Function', 'backend/main.py'),
        node('person', 'Struct', 'src/main.rs', 'Person'),
      ],
      [
        edge('a-b', 'get_person', 'person', 'TypeReference'),
        edge('b-a', 'person', 'get_person', 'TypeReference'),
      ],
      { includeLowConfidence: true },
    )

    expect(issues.some(issue => issue.kind === 'cycle-candidate' && issue.confidence === 'low')).toBe(true)
  })

  it('groups file-level import cycles into one warning', () => {
    const issues = buildHotspotIssues(
      [
        node('a-fn', 'Function', 'backend/main.py'),
        node('b-fn', 'Function', 'src/main.rs'),
        node('b-type', 'Struct', 'src/main.rs', 'Person'),
      ],
      [
        edge('a-b', 'a-fn', 'b-fn', 'Imports'),
        edge('b-a', 'b-fn', 'a-fn', 'Imports'),
        edge('a-b-2', 'a-fn', 'b-type', 'Calls'),
      ],
    )

    const cycle = issues.find(issue => issue.kind === 'cycle-candidate')
    expect(cycle).toMatchObject({
      severity: 'warning',
      confidence: 'high',
      connections: 3,
    })
    expect(cycle?.title).toContain('backend/main.py <-> src/main.rs')
    expect(cycle?.files?.sort()).toEqual(['backend/main.py', 'src/main.rs'])
  })

  it('detects backend to frontend boundary violations as critical', () => {
    const issues = buildHotspotIssues(
      [
        node('backend', 'Function', 'backend/src/lib.rs'),
        node('frontend', 'Component', 'frontend/src/App.tsx'),
      ],
      [edge('bad-import', 'backend', 'frontend', 'Imports')],
    )

    expect(issues.find(issue => issue.kind === 'boundary-violation')).toMatchObject({
      severity: 'critical',
      confidence: 'high',
    })
  })

  it('detects frontend API calls without a backend handler', () => {
    const issues = buildHotspotIssues(
      [
        node('caller', 'Component', 'frontend/src/App.tsx', 'App'),
        node('endpoint', 'Endpoint', 'backend/src/routes.rs', 'GET /api/missing'),
      ],
      [edge('api', 'caller', 'endpoint', 'ApiCall')],
    )

    expect(issues.find(issue => issue.kind === 'frontend-call-without-handler')).toMatchObject({
      severity: 'warning',
      confidence: 'high',
      files: ['frontend/src/App.tsx', 'backend/src/routes.rs'],
    })
  })

  it('classifies high-degree utility nodes as noise', () => {
    const issues = buildHotspotIssues(
      [node('utils', 'Function', 'frontend/src/lib/utils.ts'), node('caller')],
      [edge('noise', 'utils', 'caller', 'Calls', 30)],
    )

    expect(issues.find(issue => issue.kind === 'noise-utility')).toMatchObject({
      severity: 'noise',
    })
    expect(issues.some(issue => issue.kind === 'god-module' && issue.nodeId === 'utils')).toBe(false)
  })

  it('detects central API clients and unused endpoints', () => {
    const nodes = [
      node('requestJson', 'Function', 'frontend/src/api/client.ts'),
      node('page', 'Component', 'frontend/src/Page.tsx'),
      node('endpoint', 'Endpoint', 'backend/src/routes.rs', 'GET /api/person'),
      node('unused', 'Endpoint', 'backend/src/routes.rs', 'GET /api/unused'),
    ]
    const edges = [
      edge('api', 'page', 'endpoint', 'ApiCall'),
      edge('api-client', 'requestJson', 'endpoint', 'ApiCall', 8),
      edge('handler', 'endpoint', 'requestJson', 'EndpointHandler'),
    ]

    const issues = buildHotspotIssues(nodes, edges)

    expect(issues.some(issue => issue.kind === 'central-api-client' && issue.nodeId === 'requestJson')).toBe(true)
    expect(issues.some(issue => issue.kind === 'unused-endpoint' && issue.nodeId === 'unused')).toBe(true)
  })

  it('hides low-confidence issues by default', () => {
    const visible = buildHotspotIssues(
      [node('a'), node('b')],
      [edge('a-b', 'a', 'b', 'DataFlow'), edge('b-a', 'b', 'a', 'DataFlow')],
    )
    const all = buildHotspotIssues(
      [node('a'), node('b')],
      [edge('a-b', 'a', 'b', 'DataFlow'), edge('b-a', 'b', 'a', 'DataFlow')],
      { includeLowConfidence: true },
    )

    expect(visible.some(issue => issue.confidence === 'low')).toBe(false)
    expect(all.some(issue => issue.confidence === 'low')).toBe(true)
  })
})
