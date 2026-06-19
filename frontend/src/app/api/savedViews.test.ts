import { describe, expect, it } from 'vitest'
import { applySavedViewState, normalizeSavedView, serializableFilters } from './savedViews'
import type { EdgeType, GraphFilters, LanguageFilter, NodeType, SavedView } from '../types'

const allNodeTypes = new Set<NodeType>(['File', 'Module', 'Struct', 'Class', 'Object', 'Enum', 'Trait', 'Impl', 'Function', 'Method', 'Component', 'Hook', 'Interface', 'TypeAlias', 'Property', 'Signal', 'Handler', 'Endpoint', 'Macro', 'ExternalCrate'])
const allEdgeTypes = new Set<EdgeType>(['Contains', 'Imports', 'Uses', 'Calls', 'Renders', 'ApiCall', 'EndpointHandler', 'Implements', 'TypeReference', 'DataFlow', 'ModDeclaration', 'ExternalDependency'])

function filters(languages: LanguageFilter[] = ['rust', 'typescript', 'python', 'qml', 'external', 'endpoints']): GraphFilters {
  return {
    nodeTypes: allNodeTypes,
    edgeTypes: allEdgeTypes,
    languages: new Set(languages),
    edgeVisibility: 'All',
    showTests: true,
    showExternal: true,
    onlyPublicAPI: false,
    depth: 'full',
    onlyCurrentFile: false,
  }
}

function savedView(): SavedView {
  return {
    id: 'view',
    name: 'QML',
    filters: {
      nodeTypes: ['File', 'Object'] as unknown as Set<NodeType>,
      edgeTypes: ['Contains', 'Renders'] as unknown as Set<EdgeType>,
      languages: ['qml', 'endpoints'] as unknown as Set<LanguageFilter>,
    },
    focusedNodeId: 'node:qml',
    collapsedGroups: ['file:Main.qml'],
  }
}

describe('saved view helpers', () => {
  it('normalizes backend arrays into Sets', () => {
    const normalized = normalizeSavedView(savedView())

    expect(normalized.filters.languages).toBeInstanceOf(Set)
    expect([...(normalized.filters.languages as Set<LanguageFilter>)]).toEqual(['qml', 'endpoints'])
    expect(normalized.collapsedGroups).toEqual(['file:Main.qml'])
  })

  it('restores language filters, collapsed groups, and focus', () => {
    const applied = applySavedViewState(filters(['rust']), savedView())

    expect([...applied.filters.languages]).toEqual(['qml', 'endpoints'])
    expect([...applied.collapsedGroups]).toEqual(['file:Main.qml'])
    expect(applied.focusedNodeId).toBe('node:qml')
  })

  it('serializes filters for backend save payloads', () => {
    const payload = serializableFilters(filters(['python']))

    expect(payload.languages).toEqual(['python'])
    expect(Array.isArray(payload.nodeTypes)).toBe(true)
    expect(Array.isArray(payload.edgeTypes)).toBe(true)
  })
})
