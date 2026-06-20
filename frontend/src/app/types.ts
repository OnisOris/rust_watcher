export type NodeType =
  | 'File'
  | 'Module'
  | 'Struct'
  | 'Class'
  | 'Object'
  | 'Enum'
  | 'Trait'
  | 'Impl'
  | 'Function'
  | 'Method'
  | 'Component'
  | 'Hook'
  | 'Interface'
  | 'TypeAlias'
  | 'Property'
  | 'Signal'
  | 'Handler'
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
export type DataFlowKind =
  | 'Argument'
  | 'ReturnValue'
  | 'Assignment'
  | 'StateUpdate'
  | 'PropertyBinding'
  | 'ApiRequest'
  | 'ApiResponse'
  | 'ModelUse'
  | 'Unknown'
export type SourceReachability = 'Active' | 'Detached' | 'Generated' | 'External'
export type DiagnosticSeverity = 'Error' | 'Warning' | 'Information' | 'Hint'
export type GraphMode = 'Macro' | 'Meso' | 'Micro' | 'CallFlow' | 'DataFlow' | 'Traits'
export type GraphLabelMode = 'auto' | 'key' | 'all'
export type EdgeVisibilityLevel = 'Essential' | 'Semantic' | 'All'
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
  reachability?: SourceReachability
  reachableFrom?: string[]
  detachedReason?: string
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
  label?: string
  description?: string
  dataFlowKind?: DataFlowKind
  evidence?: string
  bundledCount?: number
  bundledTypes?: EdgeType[]
  bundledEdgeIds?: string[]
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
  fullRebuild?: boolean
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
  endpointDetails?: EndpointDetails
}

export interface EndpointDetails {
  routeMethod: string
  routePath: string
  routeKey: string
  endpointLanguage?: string
  handlers: EndpointHandlerDetails[]
}

export interface EndpointHandlerDetails {
  nodeId: string
  label: string
  handlerLanguage?: string
  handlerFile?: string
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
  languages: Set<LanguageFilter>
  edgeVisibility: EdgeVisibilityLevel
  showTests: boolean
  showExternal: boolean
  showDetached: boolean
  onlyPublicAPI: boolean
  depth: 1 | 2 | 3 | 'full'
  onlyCurrentFile: boolean
}

export type LanguageFilter = 'rust' | 'typescript' | 'python' | 'qml' | 'external' | 'endpoints'

export interface SavedView {
  id: string
  name: string
  filters: Partial<GraphFilters>
  focusedNodeId?: string | null
  collapsedGroups: string[]
  layoutOverrides?: Record<string, unknown>
  createdAt?: string
  updatedAt?: string
}
