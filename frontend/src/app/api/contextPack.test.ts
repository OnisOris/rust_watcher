import { describe, expect, it } from 'vitest'
import { contextPackToMarkdown, contextSnippetLabel, summarizeContextPack } from './contextPack'
import type { ContextPack } from '../types'

function pack(): ContextPack {
  return {
    id: 'pack',
    kind: 'Trace',
    title: 'Trace context',
    summary: '1 snippet, 1 node, 1 edge.',
    rootNodeId: 'n1',
    routeKey: 'GET /api/person',
    traceId: 'trace',
    warnings: ['Detached source included.'],
    createdAt: '1',
    nodes: [{ id: 'n1', type: 'Function', label: 'handler', language: 'rust', file: 'src/main.rs', line: 10, x: 0, y: 0, vx: 0, vy: 0 }],
    edges: [{ id: 'e1', source: 'caller', target: 'n1', type: 'Calls', confidence: 'Semantic' }],
    diagnostics: [{ id: 'd1', language: 'rust', file: 'src/main.rs', severity: 'Warning', message: 'careful', relatedNodeIds: ['n1'] }],
    snippets: [{
      id: 's1',
      file: 'src/main.rs',
      language: 'rust',
      startLine: 8,
      endLine: 11,
      code: 'fn handler() {\n    ok();\n}',
      relatedNodeIds: ['n1'],
      relatedEdgeIds: ['e1'],
      reason: 'selected node',
    }],
  }
}

describe('context pack helpers', () => {
  it('labels snippets by file and line range', () => {
    expect(contextSnippetLabel(pack().snippets[0])).toBe('src/main.rs:L8-L11')
  })

  it('summarizes context pack counts', () => {
    expect(summarizeContextPack(pack())).toBe('1 snippet · 1 node · 1 edge')
  })

  it('serializes markdown with title, warnings, snippets, files and diagnostics', () => {
    const markdown = contextPackToMarkdown(pack())
    expect(markdown).toContain('# Trace context')
    expect(markdown).toContain('Detached source included')
    expect(markdown).toContain('src/main.rs:L8-L11')
    expect(markdown).toContain('Warning: src/main.rs')
    expect(markdown).toContain('Calls: caller -> n1 [Semantic]')
  })
})
