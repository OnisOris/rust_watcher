import type { GraphPatch } from '../types'

export function shouldRefreshSnapshotForPatch(patch: GraphPatch, localNodeCount: number) {
  if (patch.fullRebuild) return true
  if (localNodeCount > 0) return false
  return !!(
    patch.updatedNodes.length
    || patch.removedNodeIds.length
    || patch.updatedEdges.length
    || patch.removedEdgeIds.length
  )
}

export function nextLocalNodeCountAfterPatch(localNodeCount: number, patch: GraphPatch) {
  if (shouldRefreshSnapshotForPatch(patch, localNodeCount)) return localNodeCount
  return Math.max(0, localNodeCount + patch.addedNodes.length - patch.removedNodeIds.length)
}
