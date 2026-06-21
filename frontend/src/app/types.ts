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
export type TraceKind = 'Route' | 'DataFlow' | 'NodeNeighborhood'
export type TraceStepKind =
  | 'Caller'
  | 'ApiRequest'
  | 'Endpoint'
  | 'EndpointHandler'
  | 'BackendHandler'
  | 'ServiceCall'
  | 'ModelUse'
  | 'ReturnValue'
  | 'ApiResponse'
  | 'StateUpdate'
  | 'PropertyBinding'
  | 'DetachedSource'
  | 'ExternalDependency'
  | 'Unknown'
export type ContextPackKind = 'Node' | 'Trace' | 'Route' | 'DataFlow'
export type DiagnosticSeverity = 'Error' | 'Warning' | 'Information' | 'Hint'
export type GraphMode = 'Macro' | 'Meso' | 'Micro' | 'CallFlow' | 'DataFlow' | 'Traits'
export type GraphLayoutMode = 'Force' | 'SemanticZones' | 'PackageMap' | 'Neighborhood'
export type GraphLabelMode = 'auto' | 'key' | 'all'
export type EdgeVisibilityLevel = 'Essential' | 'Semantic' | 'All'
export type AppState = 'empty' | 'indexing' | 'normal' | 'error'
export type AnalyzerStatus = 'Starting' | 'Indexing' | 'Ready' | 'Fallback' | 'Stale' | 'Error'
export type AnalyzerKind = 'Rust' | 'TypeScript' | 'Python' | 'Qml' | 'Other'
export type AnalyzerEngine =
  | 'RustAnalyzer'
  | 'Ty'
  | 'TypeScriptParser'
  | 'TypeScriptLanguageServer'
  | 'QmlParser'
  | 'QmlLanguageServer'
  | 'TreeSitter'
  | 'Parser'
  | 'Other'
export type AnalyzerCapability =
  | 'Symbols'
  | 'Diagnostics'
  | 'References'
  | 'Definitions'
  | 'TypeDefinitions'
  | 'CallHierarchy'
  | 'SemanticCalls'
  | 'SemanticTokens'
  | 'Formatting'
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
  underlyingNodeIds?: string[]
  underlyingEdgeIds?: string[]
  packagePath?: string
  regionId?: string
  layoutGuide?: string
  packageStats?: {
    fileCount: number
    symbolCount: number
    endpointCount: number
    diagnosticCount: number
    exportedSymbolCount: number
    incomingEdgeCount: number
    outgoingEdgeCount: number
  }
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
  routedPath?: Array<{ x: number; y: number }>
}

export type GraphRegionKind =
  | 'Language'
  | 'Package'
  | 'Module'
  | 'Layer'
  | 'Boundary'
  | 'External'
  | 'Detached'
  | 'Generated'

export interface GraphBounds {
  x: number
  y: number
  width: number
  height: number
}

export interface RegionStats {
  fileCount: number
  symbolCount: number
  endpointCount: number
  diagnosticCount: number
  incomingEdgeCount: number
  outgoingEdgeCount: number
}

export interface GraphRegion {
  id: string
  label: string
  kind: GraphRegionKind
  language?: string
  bounds: GraphBounds
  colorToken: string
  nodeIds: string[]
  childRegionIds: string[]
  stats: RegionStats
}

export interface LayoutRegionAssignment {
  nodeId: string
  regionId: string
  reason: string
}

export interface SemanticLayoutResult {
  nodes: GraphNode[]
  edges: GraphEdge[]
  regions: GraphRegion[]
  assignments: LayoutRegionAssignment[]
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
  analyzers?: AnalyzerServiceStatus[]
  pythonAnalyzer?: {
    mode: string
    status: string
    message?: string | null
  } | null
  projectName: string | null
  projectPath: string | null
  lastUpdated: string | null
  message: string | null
  progress: number | null
}

export interface AnalyzerServiceStatus {
  id: string
  kind: AnalyzerKind
  engine: AnalyzerEngine
  label: string
  status: AnalyzerStatus
  mode?: string | null
  message?: string | null
  capabilities: AnalyzerCapability[]
  filesIndexed: number
  lastUpdated?: string | null
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

export interface TraceStep {
  id: string
  kind: TraceStepKind
  nodeId?: string
  edgeId?: string
  title: string
  description: string
  language?: string
  file?: string
  line?: number
  confidence?: EdgeConfidence
  evidence?: string
  reachability?: SourceReachability
}

export interface TraceExplanation {
  id: string
  kind: TraceKind
  title: string
  summary: string
  steps: TraceStep[]
  warnings: string[]
  rootNodeId?: string
  routeKey?: string
  createdAt: string
}

export interface ContextSnippet {
  id: string
  file: string
  language?: string
  startLine: number
  endLine: number
  code: string
  relatedNodeIds: string[]
  relatedEdgeIds: string[]
  reason: string
}

export interface ContextPack {
  id: string
  kind: ContextPackKind
  title: string
  summary: string
  rootNodeId?: string
  routeKey?: string
  traceId?: string
  snippets: ContextSnippet[]
  nodes: GraphNode[]
  edges: GraphEdge[]
  diagnostics: DiagnosticRecord[]
  warnings: string[]
  createdAt: string
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
