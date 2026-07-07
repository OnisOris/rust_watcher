import type { ReactNode } from 'react'
import type {
  DiagnosticRecord,
  EdgeType,
  GraphEdge,
  GraphFilters,
  GraphLabelMode,
  GraphLayoutSettings,
  GraphMode,
  GraphNode,
  ProjectFile,
  NodeType,
  ThemeMode,
  TraceExplanation,
} from '../../types'

export type GraphViewMode =
  | 'project-map'
  | 'dependency-matrix'
  | 'hotspots'
  | 'module-drilldown'
  | 'local-neighborhood'
  | 'call-flow'
  | 'api-data-flow'
  | 'raw-graph'

export type ProjectGroupKind =
  | 'workspace'
  | 'frontend'
  | 'backend'
  | 'shared'
  | 'tests'
  | 'external'
  | 'generated'
  | 'module'
  | 'directory'
  | 'unknown'

export type ProjectMapGrouping = 'architecture' | 'language' | 'directory' | 'module' | 'runtime'

export interface ProjectGroup {
  id: string
  label: string
  kind: ProjectGroupKind
  pathPrefix?: string
  language?: string
  nodeIds: string[]
  fileCount: number
  symbolCount: number
  incomingCount: number
  outgoingCount: number
  languageBreakdown?: Record<string, number>
  topFiles?: string[]
  keySymbols?: string[]
  children?: ProjectGroup[]
}

export interface AggregatedProjectEdgeExample {
  id: string
  sourceLabel: string
  targetLabel: string
  type: EdgeType
  sourceFile?: string
  targetFile?: string
}

export interface AggregatedProjectEdge {
  id: string
  sourceGroupId: string
  targetGroupId: string
  count: number
  edgeTypes: Partial<Record<EdgeType, number>>
  underlyingEdgeIds: string[]
  examples?: AggregatedProjectEdgeExample[]
}

export interface ProjectMapModel {
  groups: ProjectGroup[]
  edges: AggregatedProjectEdge[]
  nodeToGroup: Map<string, string>
  grouping: ProjectMapGrouping
  autoExpanded: boolean
}

export type DependencyMatrixLevel = 'area' | 'module' | 'directory' | 'file'

export type DependencyMatrixBadge =
  | 'cycle'
  | 'strong'
  | 'unexpected'
  | 'violation'
  | 'external'
  | 'type-only'

export interface DependencyMatrixEdgeExample {
  id: string
  sourceLabel: string
  targetLabel: string
  type: EdgeType
  sourceFile?: string
  targetFile?: string
}

export interface DependencyMatrixCell {
  sourceGroupId: string
  targetGroupId: string
  count: number
  edgeTypes: Partial<Record<EdgeType, number>>
  underlyingEdgeIds: string[]
  files: string[]
  examples: DependencyMatrixEdgeExample[]
  badges: DependencyMatrixBadge[]
}

export interface DependencyMatrixModel {
  groups: ProjectGroup[]
  cells: DependencyMatrixCell[]
  level: DependencyMatrixLevel
  totalGroups: number
  truncated: boolean
  suggestedLevel?: DependencyMatrixLevel
}

export type HotspotSeverity = 'critical' | 'warning' | 'info' | 'noise'
export type HotspotConfidence = 'high' | 'medium' | 'low'

export type HotspotKind =
  | 'god-module'
  | 'noise-utility'
  | 'central-api-client'
  | 'too-many-incoming'
  | 'too-many-outgoing'
  | 'cycle-candidate'
  | 'unused-endpoint'
  | 'frontend-call-without-handler'
  | 'backend-endpoint-without-caller'
  | 'boundary-violation'

export interface HotspotIssue {
  id: string
  nodeId?: string
  edgeIds?: string[]
  title: string
  description: string
  severity: HotspotSeverity
  kind: HotspotKind
  confidence?: HotspotConfidence
  connections: number
  files?: string[]
  modules?: string[]
  suggestion?: string
}

export interface ApiDataFlowRow {
  id: string
  status: 'ok' | 'no-handler' | 'no-caller' | 'unused' | 'unresolved'
  frontendCallerNodeId?: string
  frontendCallerLabel?: string
  method?: string
  endpointPath: string
  endpointNodeId?: string
  backendHandlerNodeId?: string
  backendHandlerLabel?: string
  dataTypeNodeId?: string
  dataTypeLabel?: string
  underlyingEdgeIds: string[]
}

export interface ApiEndpointParticipant {
  nodeId: string
  label: string
  file?: string
  type?: string
  language?: string
}

export interface ApiEndpointGroup {
  id: string
  routeKey: string
  method?: string
  path: string
  status: 'ok' | 'no-handler' | 'no-caller' | 'unused' | 'unresolved'
  callers: ApiEndpointParticipant[]
  handlers: ApiEndpointParticipant[]
  dataTypes: ApiEndpointParticipant[]
  endpointNodeIds: string[]
  underlyingEdgeIds: string[]
  edgeTypeCounts: Partial<Record<EdgeType, number>>
}

export interface LocalNeighborhoodGroup {
  label: string
  edgeType: EdgeType | 'related-api' | 'related-type' | 'related-test'
  nodes: GraphNode[]
  edges: GraphEdge[]
}

export interface LocalNeighborhoodModel {
  centerNode: GraphNode | null
  incomingNodes: GraphNode[]
  outgoingNodes: GraphNode[]
  incomingGroups: LocalNeighborhoodGroup[]
  outgoingGroups: LocalNeighborhoodGroup[]
  relatedApiNodes: GraphNode[]
  relatedTypeNodes: GraphNode[]
  relatedTestNodes: GraphNode[]
  visibleNodes: GraphNode[]
  visibleEdges: GraphEdge[]
  incomingCountByType: Partial<Record<EdgeType, number>>
  outgoingCountByType: Partial<Record<EdgeType, number>>
  isDense: boolean
  denseNodeLimit?: number
}

export interface CallFlowStep {
  id: string
  label: string
  node?: GraphNode
  file?: string
  isPlaceholder?: boolean
  role:
    | 'frontend'
    | 'state'
    | 'api-client'
    | 'endpoint'
    | 'handler'
    | 'service'
    | 'model'
    | 'unknown'
}

export interface CallFlowPath {
  id: string
  label: string
  routeKey: string
  method?: string
  path?: string
  source: 'heuristic' | 'trace'
  steps: CallFlowStep[]
  edgeIds: string[]
  edgeTypes: Partial<Record<EdgeType, number>>
  files: string[]
  traceWarning?: string
}

export interface CallFlowGroup {
  id: string
  routeKey: string
  method?: string
  path?: string
  paths: CallFlowPath[]
  callerLabels: string[]
  edgeIds: string[]
}

export interface RawGraphProps {
  nodes: GraphNode[]
  edges: GraphEdge[]
  filters: GraphFilters
  selectedNodeId: string | null
  diagnosticsByNode: Map<string, DiagnosticRecord[]>
  recenterKey: number
  theme: ThemeMode
  layoutSettings: GraphLayoutSettings
  graphMode: GraphMode
  labelMode: GraphLabelMode
  highlightedTraceNodeIds?: Set<string>
  highlightedTraceEdgeIds?: Set<string>
  onSelectNode: (id: string | null) => void
  onUpdateNodes: (nodes: GraphNode[]) => void
  onOpenNode: (node: GraphNode) => void
}

export interface ArchitectureWorkspaceProps {
  nodes: GraphNode[]
  edges: GraphEdge[]
  rawNodes: GraphNode[]
  rawEdges: GraphEdge[]
  files: ProjectFile[]
  filters: GraphFilters
  viewMode: GraphViewMode
  selectedNodeId: string | null
  diagnosticsByNode: Map<string, DiagnosticRecord[]>
  onViewModeChange: (mode: GraphViewMode) => void
  onSelectNode: (id: string | null) => void
  onOpenNode: (node: GraphNode) => void
  onTraceLoaded?: (trace: TraceExplanation) => void
  rawGraphControls?: ReactNode
  rawGraphProps: Omit<RawGraphProps, 'nodes' | 'edges' | 'filters' | 'selectedNodeId' | 'diagnosticsByNode' | 'onSelectNode' | 'onOpenNode'>
}

export type ArchitectureNodeType = NodeType
