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
  children?: ProjectGroup[]
}

export interface AggregatedProjectEdge {
  id: string
  sourceGroupId: string
  targetGroupId: string
  count: number
  edgeTypes: Partial<Record<EdgeType, number>>
  underlyingEdgeIds: string[]
}

export interface ProjectMapModel {
  groups: ProjectGroup[]
  edges: AggregatedProjectEdge[]
  nodeToGroup: Map<string, string>
}

export interface DependencyMatrixCell {
  sourceGroupId: string
  targetGroupId: string
  count: number
  edgeTypes: Partial<Record<EdgeType, number>>
  underlyingEdgeIds: string[]
}

export interface DependencyMatrixModel {
  groups: ProjectGroup[]
  cells: DependencyMatrixCell[]
}

export type HotspotSeverity = 'critical' | 'warning' | 'info' | 'noise'

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
  connections: number
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

export interface LocalNeighborhoodModel {
  centerNode: GraphNode | null
  incomingNodes: GraphNode[]
  outgoingNodes: GraphNode[]
  relatedApiNodes: GraphNode[]
  relatedTypeNodes: GraphNode[]
  relatedTestNodes: GraphNode[]
  visibleNodes: GraphNode[]
  visibleEdges: GraphEdge[]
}

export interface CallFlowStep {
  id: string
  label: string
  node?: GraphNode
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
  steps: CallFlowStep[]
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
