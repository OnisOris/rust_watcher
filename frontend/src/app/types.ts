export type NodeType =
  | 'File'
  | 'Module'
  | 'Struct'
  | 'Class'
  | 'Enum'
  | 'Trait'
  | 'Impl'
  | 'Function'
  | 'Method'
  | 'Component'
  | 'Hook'
  | 'Interface'
  | 'TypeAlias'
  | 'Endpoint'
  | 'Macro'
  | 'ExternalCrate'

export type EdgeType =
  | 'Contains'
  | 'Imports'
  | 'Uses'
  | 'Calls'
  | 'Renders'
  | 'ApiCall'
  | 'EndpointHandler'
  | 'Implements'
  | 'TypeReference'
  | 'DataFlow'
  | 'ModDeclaration'
  | 'ExternalDependency'

export type EdgeConfidence = 'Exact' | 'Semantic' | 'SyntaxFallback' | 'Heuristic'
export type DiagnosticSeverity = 'Error' | 'Warning' | 'Information' | 'Hint'
export type GraphMode = 'Macro' | 'Meso' | 'Micro' | 'CallFlow' | 'DataFlow' | 'Traits'
export type GraphLabelMode = 'auto' | 'key' | 'all'
export type AppState = 'empty' | 'indexing' | 'normal' | 'error'
export type AnalyzerStatus = 'Starting' | 'Indexing' | 'Ready' | 'Fallback' | 'Stale' | 'Error'
export type ThemeMode = 'light' | 'dark'

export interface GraphLayoutSettings {
  spacing: number
  repulsion: number
  linkLength: number
  damping: number
}

export const DEFAULT_GRAPH_LAYOUT_SETTINGS: GraphLayoutSettings = {
  spacing: 1,
  repulsion: 1,
  linkLength: 1,
  damping: 1,
}

export interface GraphNode {
  id: string
  language?: string
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
  range?: LspRange
  selectionRange?: LspRange
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
  confidence?: EdgeConfidence
}

export interface LspPosition {
  line: number
  character: number
}

export interface LspRange {
  start: LspPosition
  end: LspPosition
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
  diagnostics: DiagnosticRecord[]
  changedFiles: string[]
}

export interface DiagnosticRecord {
  id: string
  language: string
  file: string
  range?: LspRange
  severity: DiagnosticSeverity
  source?: string
  message: string
  code?: string
  relatedNodeIds: string[]
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

export interface NodeDetailsResponse {
  node: GraphNode
  incomingEdges: GraphEdge[]
  outgoingEdges: GraphEdge[]
  callers: GraphNode[]
  callees: GraphNode[]
  references: ReferenceRecord[]
  relatedTypes: GraphNode[]
  diagnostics: DiagnosticRecord[]
}

export interface ReferenceRecord {
  node?: GraphNode
  location: SourceLocation
}

export interface SourceLocation {
  file: string
  line: number
  character: number
  range?: LspRange
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
