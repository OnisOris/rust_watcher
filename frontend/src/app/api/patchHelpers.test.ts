import {
  applyDiagnosticsCountsToFiles,
  applyDiagnosticsPatch,
  applyGraphPatchToEdges,
  applyGraphPatchToNodes,
  diagnosticsByNodeFromFileMap,
} from './patchHelpers'
import type { DiagnosticRecord, GraphPatch, ProjectFile } from '../types'

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

export function runPatchHelperTests() {
  const first = applyDiagnosticsPatch(new Map(), {
    changedFiles: ['src/a.rs'],
    diagnostics: [diagnostic('a1', 'src/a.rs', 'node-a')],
  })
  const second = applyDiagnosticsPatch(first.diagnosticsByFile, {
    changedFiles: ['src/b.rs'],
    diagnostics: [diagnostic('b1', 'src/b.rs', 'node-b')],
  })
  if (second.diagnosticsByFile.get('src/a.rs')?.length !== 1) throw new Error('file A diagnostics were dropped')
  if (second.diagnosticsByFile.get('src/b.rs')?.length !== 1) throw new Error('file B diagnostics missing')

  const cleared = applyDiagnosticsPatch(second.diagnosticsByFile, {
    changedFiles: ['src/a.rs'],
    diagnostics: [],
  })
  if ((cleared.diagnosticsByFile.get('src/a.rs')?.length ?? 0) !== 0) throw new Error('file A diagnostics not cleared')
  if (cleared.diagnosticsByFile.get('src/b.rs')?.length !== 1) throw new Error('file B diagnostics should remain')

  const byNode = diagnosticsByNodeFromFileMap(second.diagnosticsByFile)
  if (byNode.get('node-a')?.[0]?.id !== 'a1') throw new Error('node A diagnostics missing')
  if (byNode.get('node-b')?.[0]?.id !== 'b1') throw new Error('node B diagnostics missing')

  const files: ProjectFile[] = [{
    id: 'file-a',
    name: 'a.rs',
    path: 'src/a.rs',
    module: 'a',
    crate: 'demo',
    functionsCount: 1,
    linksCount: 0,
    diagnosticsCount: 1,
    complexity: 'low',
  }]
  const updatedFiles = applyDiagnosticsCountsToFiles(files, cleared.diagnosticsByFile, ['src/a.rs'])
  if (updatedFiles[0].diagnosticsCount !== 0) throw new Error('file diagnostics count not cleared')

  const patch: GraphPatch = {
    addedNodes: [{ id: 'b', type: 'Function', label: 'b', x: 0, y: 0, vx: 0, vy: 0 }],
    updatedNodes: [{ id: 'a', type: 'Function', label: 'a2', x: 1, y: 1, vx: 0, vy: 0 }],
    removedNodeIds: ['old'],
    addedEdges: [{ id: 'Calls:a->b', source: 'a', target: 'b', type: 'Calls' }],
    updatedEdges: [],
    removedEdgeIds: ['Calls:old->a'],
    diagnostics: [],
    changedFiles: [],
  }
  const nodes = applyGraphPatchToNodes([
    { id: 'a', type: 'Function', label: 'a', x: 0, y: 0, vx: 0, vy: 0 },
    { id: 'old', type: 'Function', label: 'old', x: 0, y: 0, vx: 0, vy: 0 },
  ], patch)
  if (nodes.some(node => node.id === 'old')) throw new Error('removed node remained')
  if (!nodes.some(node => node.id === 'b')) throw new Error('added node missing')
  if (nodes.find(node => node.id === 'a')?.label !== 'a2') throw new Error('updated node missing')

  const edges = applyGraphPatchToEdges([
    { id: 'Calls:old->a', source: 'old', target: 'a', type: 'Calls' },
  ], patch)
  if (edges.some(edge => edge.id === 'Calls:old->a')) throw new Error('removed edge remained')
  if (!edges.some(edge => edge.id === 'Calls:a->b')) throw new Error('added edge missing')
}
