import { describe, expect, it } from 'vitest'
import { shouldRefreshSnapshotForPatch } from './backendMessages'
import type { GraphPatch } from '../types'

function patch(overrides: Partial<GraphPatch> = {}): GraphPatch {
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

describe('backend message helpers', () => {
  it('does not refresh for ordinary graph patches', () => {
    expect(shouldRefreshSnapshotForPatch(patch({
      addedNodes: [{ id: 'a', type: 'Function', label: 'a', x: 0, y: 0, vx: 0, vy: 0 }],
    }), 1)).toBe(false)
  })

  it('refreshes only for explicit or unsafe patch cases', () => {
    expect(shouldRefreshSnapshotForPatch(patch({ fullRebuild: true }), 1)).toBe(true)
    expect(shouldRefreshSnapshotForPatch(patch({ updatedNodes: [{ id: 'a', type: 'Function', label: 'a', x: 0, y: 0, vx: 0, vy: 0 }] }), 0)).toBe(true)
  })
})
