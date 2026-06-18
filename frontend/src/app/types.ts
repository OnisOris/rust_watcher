export type NodeType =
  | 'File'
  | 'Module'
  | 'Struct'
  | 'Enum'
  | 'Trait'
  | 'Impl'
  | 'Function'
  | 'Method'
  | 'Macro'
  | 'ExternalCrate'

export type EdgeType =
  | 'Contains'
  | 'Uses'
  | 'Calls'
  | 'Implements'
  | 'TypeReference'
  | 'DataFlow'
  | 'ModDeclaration'
  | 'ExternalDependency'

export type GraphMode = 'Macro' | 'Meso' | 'Micro' | 'CallFlow' | 'DataFlow' | 'Traits'
export type AppState = 'empty' | 'indexing' | 'normal' | 'error'
export type AnalyzerStatus = 'Starting' | 'Indexing' | 'Ready' | 'Fallback' | 'Stale' | 'Error'
export type ThemeMode = 'light' | 'dark'

export interface GraphNode {
  id: string
  type: NodeType
  label: string
  file?: string
  module?: string
  crate?: string
  line?: number
  visibility?: 'pub' | 'pub(crate)' | 'private'
  isAsync?: boolean
  isUnsafe?: boolean
  isGeneric?: boolean
  signature?: string
  description?: string
  pinned?: boolean
  bookmarked?: boolean
  connections?: number
  x: number
  y: number
  vx: number
  vy: number
}

export interface GraphEdge {
  id: string
  source: string
  target: string
  type: EdgeType
}

export interface ProjectFile {
  id: string
  name: string
  path: string
  module: string
  crate: string
  functionsCount: number
  linksCount: number
  diagnosticsCount: number
  complexity: 'low' | 'medium' | 'high'
}

export interface AnalysisEvent {
  id: string
  type: 'info' | 'warning' | 'error' | 'analyzer' | 'graph'
  message: string
  timestamp: string
  file?: string
}

export interface AppStatus {
  appState: AppState
  analyzerStatus: AnalyzerStatus
  projectName: string | null
  projectPath: string | null
  lastUpdated: string | null
  message: string | null
  progress: number | null
}

export interface GraphSnapshot {
  nodes: GraphNode[]
  edges: GraphEdge[]
  files: ProjectFile[]
  events: AnalysisEvent[]
  status: AppStatus
}

export interface GraphPatch {
  addedNodes: GraphNode[]
  updatedNodes: GraphNode[]
  removedNodeIds: string[]
  addedEdges: GraphEdge[]
  updatedEdges: GraphEdge[]
  removedEdgeIds: string[]
}

export interface SearchResult {
  id: string
  label: string
  type: NodeType
  file: string | null
  module: string | null
  crate: string | null
  line: number | null
}

export interface FocusResponse {
  center: string
  nodes: GraphNode[]
  edges: GraphEdge[]
}

export interface GraphFilters {
  nodeTypes: Set<NodeType>
  edgeTypes: Set<EdgeType>
  showTests: boolean
  showExternal: boolean
  onlyPublicAPI: boolean
  depth: 1 | 2 | 3 | 'full'
  onlyCurrentFile: boolean
}
