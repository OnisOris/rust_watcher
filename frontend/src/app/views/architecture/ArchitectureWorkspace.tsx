import { useEffect, useMemo, useRef, useState } from 'react'
import { buildDependencyMatrixModel } from '../../api/dependencyMatrix'
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
import type { ArchitectureWorkspaceProps } from './architectureTypes'

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
  const previousSelectedNodeId = useRef<string | null>(selectedNodeId)

  const projectMap = useMemo(() => buildProjectMapModel(nodes, edges, {
    includeTests: filters.showTests,
    includeExternal: filters.showExternal,
    includeGenerated: true,
  }), [nodes, edges, filters.showTests, filters.showExternal])
  const matrix = useMemo(() => buildDependencyMatrixModel(projectMap), [projectMap])
  const hotspots = useMemo(() => buildHotspotIssues(nodes, edges), [nodes, edges])

  useEffect(() => {
    if (selectedNodeId && selectedNodeId !== previousSelectedNodeId.current && viewMode !== 'raw-graph') {
      onViewModeChange('local-neighborhood')
    }
    previousSelectedNodeId.current = selectedNodeId
  }, [onViewModeChange, selectedNodeId, viewMode])

  const renderView = () => {
    switch (viewMode) {
      case 'project-map':
        return (
          <ProjectMapView
            model={projectMap}
            nodes={nodes}
            selectedNodeId={selectedNodeId}
            onSelectGroup={(groupId) => {
              setSelectedGroupId(groupId)
              onViewModeChange('module-drilldown')
            }}
            onSelectNode={onSelectNode}
          />
        )
      case 'dependency-matrix':
        return <DependencyMatrixView model={matrix} />
      case 'hotspots':
        return <HotspotsView issues={hotspots} nodes={nodes} onSelectNode={onSelectNode} onOpenNode={onOpenNode} onOpenNeighborhood={() => onViewModeChange('local-neighborhood')} />
      case 'module-drilldown':
        return <ModuleDrilldownView model={projectMap} nodes={nodes} selectedGroupId={selectedGroupId} onBack={() => onViewModeChange('project-map')} onSelectNode={onSelectNode} onOpenNode={onOpenNode} />
      case 'local-neighborhood':
        return <LocalNeighborhoodView nodes={nodes} edges={edges} files={files} selectedNodeId={selectedNodeId} onSelectNode={onSelectNode} onOpenNode={onOpenNode} />
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
