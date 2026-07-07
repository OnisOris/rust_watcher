import { useMemo, useState } from 'react'
import { buildProjectMapModel } from '../../api/graphAggregation'
import { buildHotspotIssues } from '../../api/hotspots'
import { ApiDataFlowView } from './ApiDataFlowView'
import { CallFlowView } from './CallFlowView'
import { DependencyMatrixView } from './DependencyMatrixView'
import { HotspotsView } from './HotspotsView'
import { LocalNeighborhoodView } from './LocalNeighborhoodView'
import { ModuleDrilldownView } from './ModuleDrilldownView'
import { ProjectMapView } from './ProjectMapView'
import { RawGraphView } from './RawGraphView'
import type { ArchitectureWorkspaceProps, ProjectMapGrouping } from './architectureTypes'

const DENSE_NODE_THRESHOLD = 150
const DENSE_EDGE_THRESHOLD = 400

export function ArchitectureWorkspace({
  nodes,
  edges,
  rawNodes,
  rawEdges,
  files,
  filters,
  viewMode,
  selectedNodeId,
  diagnosticsByNode,
  onViewModeChange,
  onSelectNode,
  onOpenNode,
  rawGraphControls,
  rawGraphProps,
}: ArchitectureWorkspaceProps) {
  const [selectedGroupId, setSelectedGroupId] = useState<string | null>(null)
  const [projectMapGrouping, setProjectMapGrouping] = useState<ProjectMapGrouping>('architecture')

  const projectMap = useMemo(() => buildProjectMapModel(nodes, edges, {
    grouping: projectMapGrouping,
    includeTests: filters.showTests,
    includeExternal: filters.showExternal,
    includeGenerated: true,
  }), [nodes, edges, filters.showTests, filters.showExternal, projectMapGrouping])
  const hotspots = useMemo(() => buildHotspotIssues(nodes, edges, { includeLowConfidence: true }), [nodes, edges])

  const renderView = () => {
    switch (viewMode) {
      case 'project-map':
        return (
          <ProjectMapView
            model={projectMap}
            nodes={nodes}
            selectedNodeId={selectedNodeId}
            grouping={projectMapGrouping}
            onGroupingChange={setProjectMapGrouping}
            onSelectGroup={(groupId) => {
              setSelectedGroupId(groupId)
              onViewModeChange('module-drilldown')
            }}
            onSelectNode={onSelectNode}
            onOpenRawGraph={() => onViewModeChange('raw-graph')}
            onOpenMatrix={() => onViewModeChange('dependency-matrix')}
            onOpenHotspots={() => onViewModeChange('hotspots')}
          />
        )
      case 'dependency-matrix':
        return <DependencyMatrixView nodes={nodes} edges={edges} filters={filters} />
      case 'hotspots':
        return (
          <HotspotsView
            issues={hotspots}
            nodes={nodes}
            onSelectNode={onSelectNode}
            onOpenNode={onOpenNode}
            onOpenNeighborhood={() => onViewModeChange('local-neighborhood')}
            onOpenMatrix={() => onViewModeChange('dependency-matrix')}
            onOpenApiDataFlow={() => onViewModeChange('api-data-flow')}
            onOpenRawGraph={() => onViewModeChange('raw-graph')}
          />
        )
      case 'module-drilldown':
        return <ModuleDrilldownView model={projectMap} nodes={nodes} selectedGroupId={selectedGroupId} onBack={() => onViewModeChange('project-map')} onSelectNode={onSelectNode} onOpenNode={onOpenNode} />
      case 'local-neighborhood':
        return (
          <LocalNeighborhoodView
            nodes={nodes}
            edges={edges}
            files={files}
            selectedNodeId={selectedNodeId}
            depth={filters.depth}
            onSelectNode={onSelectNode}
            onOpenNode={onOpenNode}
            onOpenRawGraph={() => onViewModeChange('raw-graph')}
            onOpenCallFlow={() => onViewModeChange('call-flow')}
            onOpenApiDataFlow={() => onViewModeChange('api-data-flow')}
          />
        )
      case 'call-flow':
        return <CallFlowView nodes={nodes} edges={edges} selectedNodeId={selectedNodeId} onSelectNode={onSelectNode} onOpenNode={onOpenNode} />
      case 'api-data-flow':
        return <ApiDataFlowView nodes={nodes} edges={edges} onSelectNode={onSelectNode} onOpenNode={onOpenNode} />
      case 'raw-graph':
        return (
          <RawGraphView
            {...rawGraphProps}
            nodes={rawNodes}
            edges={rawEdges}
            filters={filters}
            selectedNodeId={selectedNodeId}
            diagnosticsByNode={diagnosticsByNode}
            onSelectNode={onSelectNode}
            onOpenNode={onOpenNode}
            controls={rawGraphControls}
          />
        )
    }
  }

  return (
    <div className="absolute inset-0 flex flex-col overflow-hidden architecture-workspace">
      <div className="relative flex-1 min-h-0">
        {renderView()}
      </div>
    </div>
  )
}

export function shouldUseReadableDefault(nodes: unknown[], edges: unknown[]) {
  return nodes.length > DENSE_NODE_THRESHOLD || edges.length > DENSE_EDGE_THRESHOLD
}
