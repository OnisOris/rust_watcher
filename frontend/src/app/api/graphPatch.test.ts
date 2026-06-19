import { describe, expect, it } from 'vitest'
import {
  applyDiagnosticCountsToFiles,
  applyDiagnosticsPatch,
  applyGraphPatchToEdges,
  applyGraphPatchToNodes,
  diagnosticsByNodeFromFileMap,
} from './graphPatch'
import type { DiagnosticRecord, GraphEdge, GraphNode, GraphPatch, ProjectFile } from '../types'

function diagnostic(id: string, file: string, nodeId: string): DiagnosticRecord {
  return {
    id,
    language: 'rust',
    file,
    severity: 'Error',
    message: id,
    relatedNodeIds: [nodeId],
  }
}

function emptyPatch(overrides: Partial<GraphPatch> = {}): GraphPatch {
  return {
    addedNodes: [],
    updatedNodes: [],
    removedNodeIds: [],
    addedEdges: [],
    updatedEdges: [],
    removedEdgeIds: [],
    diagnostics: [],
    changedFiles: [],
    ...overrides,
  }
}

describe('graph patch helpers', () => {
  it('adds, updates, and removes nodes', () => {
    const nodes: GraphNode[] = [
      { id: 'a', type: 'Function', label: 'a', x: 0, y: 0, vx: 0, vy: 0 },
      { id: 'old', type: 'Function', label: 'old', x: 0, y: 0, vx: 0, vy: 0 },
    ]
    const next = applyGraphPatchToNodes(nodes, emptyPatch({
      addedNodes: [{ id: 'b', type: 'Function', label: 'b', x: 0, y: 0, vx: 0, vy: 0 }],
      updatedNodes: [{ id: 'a', type: 'Function', label: 'a2', x: 1, y: 1, vx: 0, vy: 0 }],
      removedNodeIds: ['old'],
    }))

    expect(next.map(node => node.id)).toEqual(['a', 'b'])
    expect(next.find(node => node.id === 'a')?.label).toBe('a2')
  })

  it('adds, updates, and removes edges', () => {
    const edges: GraphEdge[] = [
      { id: 'Calls:a->old', source: 'a', target: 'old', type: 'Calls' },
      { id: 'Calls:a->b', source: 'a', target: 'b', type: 'Calls' },
    ]
    const next = applyGraphPatchToEdges(edges, emptyPatch({
      addedEdges: [{ id: 'Calls:b->c', source: 'b', target: 'c', type: 'Calls' }],
      updatedEdges: [{ id: 'Calls:a->b', source: 'a', target: 'b', type: 'Calls', confidence: 'Semantic' }],
      removedEdgeIds: ['Calls:a->old'],
    }))

    expect(next.map(edge => edge.id)).toEqual(['Calls:a->b', 'Calls:b->c'])
    expect(next.find(edge => edge.id === 'Calls:a->b')?.confidence).toBe('Semantic')
  })

  it('preserves unrelated diagnostics across patches', () => {
    const first = applyDiagnosticsPatch(new Map(), {
      changedFiles: ['src/a.rs'],
      diagnostics: [diagnostic('a1', 'src/a.rs', 'node-a')],
    })
    const second = applyDiagnosticsPatch(first.diagnosticsByFile, {
      changedFiles: ['src/b.rs'],
      diagnostics: [diagnostic('b1', 'src/b.rs', 'node-b')],
    })

    expect(second.diagnosticsByFile.get('src/a.rs')?.map(item => item.id)).toEqual(['a1'])
    expect(second.diagnosticsByFile.get('src/b.rs')?.map(item => item.id)).toEqual(['b1'])
  })

  it('clears only changed files with empty diagnostics', () => {
    const previous = new Map<string, DiagnosticRecord[]>([
      ['src/a.rs', [diagnostic('a1', 'src/a.rs', 'node-a')]],
      ['src/b.rs', [diagnostic('b1', 'src/b.rs', 'node-b')]],
    ])
    const next = applyDiagnosticsPatch(previous, {
      changedFiles: ['src/a.rs'],
      diagnostics: [],
    })

    expect(next.diagnosticsByFile.get('src/a.rs')).toEqual([])
    expect(next.diagnosticsByFile.get('src/b.rs')?.map(item => item.id)).toEqual(['b1'])
  })

  it('builds diagnostics by node from file diagnostics', () => {
    const byNode = diagnosticsByNodeFromFileMap(new Map([
      ['src/a.rs', [
        diagnostic('a1', 'src/a.rs', 'node-a'),
        diagnostic('shared', 'src/a.rs', 'node-b'),
      ]],
      ['src/b.rs', [diagnostic('b1', 'src/b.rs', 'node-b')]],
    ]))

    expect(byNode.get('node-a')?.map(item => item.id)).toEqual(['a1'])
    expect(byNode.get('node-b')?.map(item => item.id)).toEqual(['shared', 'b1'])
  })

  it('updates diagnostic counts only for affected files', () => {
    const files: ProjectFile[] = [
      {
        id: 'file-a',
        name: 'a.rs',
        path: 'src/a.rs',
        module: 'a',
        crate: 'demo',
        functionsCount: 1,
        linksCount: 0,
        diagnosticsCount: 2,
        complexity: 'low',
      },
      {
        id: 'file-b',
        name: 'b.rs',
        path: 'src/b.rs',
        module: 'b',
        crate: 'demo',
        functionsCount: 1,
        linksCount: 0,
        diagnosticsCount: 7,
        complexity: 'low',
      },
    ]
    const diagnosticsByFile = new Map<string, DiagnosticRecord[]>([
      ['src/a.rs', []],
      ['src/b.rs', [diagnostic('b1', 'src/b.rs', 'node-b')]],
    ])

    const next = applyDiagnosticCountsToFiles(files, diagnosticsByFile, ['src/a.rs'])

    expect(next.find(file => file.path === 'src/a.rs')?.diagnosticsCount).toBe(0)
    expect(next.find(file => file.path === 'src/b.rs')?.diagnosticsCount).toBe(7)
  })
})
