import type { DiagnosticRecord, GraphEdge, GraphNode, GraphPatch, ProjectFile } from '../types'

export function applyGraphPatchToNodes(nodes: GraphNode[], patch: GraphPatch): GraphNode[] {
  const removed = new Set(patch.removedNodeIds)
  const existingBeforePatch = new Map(nodes.map(node => [node.id, node]))
  const updated = new Map(patch.updatedNodes.map(node => {
    const existing = existingBeforePatch.get(node.id)
    return [node.id, existing ? mergeNodePreservingLayout(existing, node) : node]
  }))
  const next = nodes
    .filter(node => !removed.has(node.id))
    .map(node => updated.get(node.id) ?? node)
  const existing = new Set(next.map(node => node.id))
  const additions = patch.addedNodes
    .filter(node => !existing.has(node.id))
    .map(node => seedAddedNodePosition(node, next, patch))
  return [...next, ...additions]
}

export function applyGraphPatchToEdges(edges: GraphEdge[], patch: GraphPatch): GraphEdge[] {
  const removed = new Set(patch.removedEdgeIds)
  const removedNodes = new Set(patch.removedNodeIds)
  const updated = new Map(patch.updatedEdges.map(edge => [edge.id, edge]))
  const next = edges
    .filter(edge => !removed.has(edge.id) && !removedNodes.has(edge.source) && !removedNodes.has(edge.target))
    .map(edge => updated.get(edge.id) ?? edge)
  const existing = new Set(next.map(edge => edge.id))
  return [...next, ...patch.addedEdges.filter(edge => !existing.has(edge.id))]
}

function mergeNodePreservingLayout(existing: GraphNode, updated: GraphNode): GraphNode {
  return {
    ...updated,
    x: existing.x,
    y: existing.y,
    vx: existing.vx,
    vy: existing.vy,
    pinned: existing.pinned,
  }
}

function seedAddedNodePosition(node: GraphNode, existingNodes: GraphNode[], patch: GraphPatch): GraphNode {
  const existingById = new Map(existingNodes.map(existing => [existing.id, existing]))
  const relatedEdge = patch.addedEdges.find(edge =>
    edge.source === node.id && existingById.has(edge.target)
    || edge.target === node.id && existingById.has(edge.source)
  )
  if (!relatedEdge) return node
  const related = existingById.get(relatedEdge.source === node.id ? relatedEdge.target : relatedEdge.source)
  if (!related) return node
  const offset = stableOffset(node.id)
  return {
    ...node,
    x: related.x + offset.x,
    y: related.y + offset.y,
    vx: 0,
    vy: 0,
  }
}

function stableOffset(id: string) {
  let hash = 0
  for (let index = 0; index < id.length; index++) {
    hash = (hash * 31 + id.charCodeAt(index)) | 0
  }
  const angle = (Math.abs(hash) % 360) * Math.PI / 180
  const radius = 72 + (Math.abs(hash) % 37)
  return {
    x: Math.cos(angle) * radius,
    y: Math.sin(angle) * radius,
  }
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

export function applyDiagnosticCountsToFiles(
  files: ProjectFile[],
  diagnosticsByFile: Map<string, DiagnosticRecord[]>,
  changedFiles: string[],
) {
  const changed = new Set(changedFiles)
  return files.map(file => {
    if (!changed.has(file.path)) return file
    return { ...file, diagnosticsCount: diagnosticsByFile.get(file.path)?.length ?? 0 }
  })
}
