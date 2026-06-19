import type { EdgeType, GraphFilters, LanguageFilter, NodeType, SavedView } from '../types'

export interface SavedViewApplication {
  filters: GraphFilters
  collapsedGroups: Set<string>
  focusedNodeId: string | null
}

export function normalizeSavedView(view: SavedView): SavedView {
  const rawFilters = view.filters as Partial<GraphFilters> & {
    nodeTypes?: NodeType[] | Set<NodeType>
    edgeTypes?: EdgeType[] | Set<EdgeType>
    languages?: LanguageFilter[] | Set<LanguageFilter>
  }
  return {
    ...view,
    collapsedGroups: Array.isArray(view.collapsedGroups) ? view.collapsedGroups : [],
    focusedNodeId: view.focusedNodeId ?? null,
    filters: {
      ...view.filters,
      nodeTypes: rawFilters.nodeTypes
        ? rawFilters.nodeTypes instanceof Set ? rawFilters.nodeTypes : new Set(rawFilters.nodeTypes)
        : undefined,
      edgeTypes: rawFilters.edgeTypes
        ? rawFilters.edgeTypes instanceof Set ? rawFilters.edgeTypes : new Set(rawFilters.edgeTypes)
        : undefined,
      languages: rawFilters.languages
        ? rawFilters.languages instanceof Set ? rawFilters.languages : new Set(rawFilters.languages)
        : undefined,
    },
  }
}

export function serializableFilters(filters: GraphFilters) {
  return {
    ...filters,
    nodeTypes: [...filters.nodeTypes],
    edgeTypes: [...filters.edgeTypes],
    languages: [...filters.languages],
  }
}

export function applySavedViewState(currentFilters: GraphFilters, view: SavedView): SavedViewApplication {
  const normalized = normalizeSavedView(view)
  return {
    filters: {
      ...currentFilters,
      ...normalized.filters,
      nodeTypes: normalized.filters.nodeTypes instanceof Set ? normalized.filters.nodeTypes : currentFilters.nodeTypes,
      edgeTypes: normalized.filters.edgeTypes instanceof Set ? normalized.filters.edgeTypes : currentFilters.edgeTypes,
      languages: normalized.filters.languages instanceof Set ? normalized.filters.languages : currentFilters.languages,
    },
    collapsedGroups: new Set(normalized.collapsedGroups),
    focusedNodeId: normalized.focusedNodeId ?? null,
  }
}
