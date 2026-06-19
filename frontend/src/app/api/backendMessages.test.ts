import { describe, expect, it } from 'vitest'
import { nextLocalNodeCountAfterPatch, shouldRefreshSnapshotForPatch } from './backendMessages'
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
  it('does not refresh for empty local graph plus added nodes', () => {
    expect(shouldRefreshSnapshotForPatch(patch({
      addedNodes: [{ id: 'a', type: 'Function', label: 'a', x: 0, y: 0, vx: 0, vy: 0 }],
    }), 0)).toBe(false)
  })

  it('does not refresh for non-empty local graph plus ordinary patches', () => {
    expect(shouldRefreshSnapshotForPatch(patch({
      addedNodes: [{ id: 'a', type: 'Function', label: 'a', x: 0, y: 0, vx: 0, vy: 0 }],
    }), 1)).toBe(false)
  })

  it('refreshes for empty local graph plus updated nodes', () => {
    expect(shouldRefreshSnapshotForPatch(patch({ updatedNodes: [{ id: 'a', type: 'Function', label: 'a', x: 0, y: 0, vx: 0, vy: 0 }] }), 0)).toBe(true)
  })

  it('refreshes for full rebuild patches', () => {
    expect(shouldRefreshSnapshotForPatch(patch({ fullRebuild: true }), 1)).toBe(true)
  })

  it('tracks ordinary patch sequences without forcing refresh', () => {
    const first = patch({
      addedNodes: [{ id: 'a', type: 'Function', label: 'a', x: 0, y: 0, vx: 0, vy: 0 }],
    })
    let localNodeCount = 0
    expect(shouldRefreshSnapshotForPatch(first, localNodeCount)).toBe(false)
    localNodeCount = nextLocalNodeCountAfterPatch(localNodeCount, first)

    const second = patch({
      updatedNodes: [{ id: 'a', type: 'Function', label: 'renamed', x: 0, y: 0, vx: 0, vy: 0 }],
    })
    expect(localNodeCount).toBe(1)
    expect(shouldRefreshSnapshotForPatch(second, localNodeCount)).toBe(false)
  })
})
