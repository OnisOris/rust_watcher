import type { DiagnosticRecord, GraphEdge, GraphNode, GraphPatch, ProjectFile } from '../types'

export function applyGraphPatchToNodes(nodes: GraphNode[], patch: GraphPatch): GraphNode[] {
  const removed = new Set(patch.removedNodeIds)
  const updated = new Map(patch.updatedNodes.map(node => [node.id, node]))
  const next = nodes
    .filter(node => !removed.has(node.id))
    .map(node => updated.get(node.id) ?? node)
  const existing = new Set(next.map(node => node.id))
  return [...next, ...patch.addedNodes.filter(node => !existing.has(node.id))]
}

export function applyGraphPatchToEdges(edges: GraphEdge[], patch: GraphPatch): GraphEdge[] {
  const removed = new Set(patch.removedEdgeIds)
  const updated = new Map(patch.updatedEdges.map(edge => [edge.id, edge]))
  const next = edges
    .filter(edge => !removed.has(edge.id))
    .map(edge => updated.get(edge.id) ?? edge)
  const existing = new Set(next.map(edge => edge.id))
  return [...next, ...patch.addedEdges.filter(edge => !existing.has(edge.id))]
}

export function diagnosticsByNodeFromFileMap(diagnosticsByFile: Map<string, DiagnosticRecord[]>) {
  const diagnosticsByNode = new Map<string, DiagnosticRecord[]>()
  diagnosticsByFile.forEach(diagnostics => {
    diagnostics.forEach(diagnostic => {
      diagnostic.relatedNodeIds.forEach(nodeId => {
        const list = diagnosticsByNode.get(nodeId) ?? []
        list.push(diagnostic)
        diagnosticsByNode.set(nodeId, list)
      })
    })
  })
  return diagnosticsByNode
}

export function applyDiagnosticsPatch(
  diagnosticsByFile: Map<string, DiagnosticRecord[]>,
  patch: Pick<GraphPatch, 'diagnostics' | 'changedFiles'>,
) {
  const nextByFile = new Map(diagnosticsByFile)
  for (const file of patch.changedFiles ?? []) nextByFile.set(file, [])
  patch.diagnostics.forEach(diagnostic => {
    const list = nextByFile.get(diagnostic.file) ?? []
    list.push(diagnostic)
    nextByFile.set(diagnostic.file, list)
  })
  return {
    diagnosticsByFile: nextByFile,
    diagnosticsByNode: diagnosticsByNodeFromFileMap(nextByFile),
  }
}

export function applyDiagnosticsCountsToFiles(
  files: ProjectFile[],
  diagnosticsByFile: Map<string, DiagnosticRecord[]>,
  changedFiles: string[],
) {
  const changed = new Set(changedFiles)
  return files.map(file => {
    if (!changed.has(file.path) && !diagnosticsByFile.has(file.path)) return file
    return { ...file, diagnosticsCount: diagnosticsByFile.get(file.path)?.length ?? 0 }
  })
}
