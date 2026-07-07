import { Activity } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import type { GraphEdge, GraphNode } from '../../types'
import type { CallFlowPath, CallFlowStep } from './architectureTypes'
import { DiagramCanvas } from './DiagramCanvas'

interface CallFlowViewProps {
  nodes: GraphNode[]
  edges: GraphEdge[]
  selectedNodeId: string | null
  onSelectNode: (id: string | null) => void
  onOpenNode: (node: GraphNode) => void
}

const COLUMNS: Array<{ role: CallFlowStep['role']; label: string }> = [
  { role: 'frontend', label: 'Frontend Page' },
  { role: 'state', label: 'Hook / State' },
  { role: 'api-client', label: 'API Client' },
  { role: 'endpoint', label: 'HTTP Endpoint' },
  { role: 'handler', label: 'Backend Handler' },
  { role: 'service', label: 'Service / Repo' },
  { role: 'model', label: 'Model / DB' },
]

const FLOW_CANVAS = { width: 1520, height: 620 }

export function CallFlowView({ nodes, edges, selectedNodeId, onSelectNode, onOpenNode }: CallFlowViewProps) {
  const paths = useMemo(() => buildCallFlowPaths(nodes, edges), [nodes, edges])
  const [selectedPathId, setSelectedPathId] = useState<string | null>(paths[0]?.id ?? null)
  const selectedNodePath = selectedNodeId ? paths.find(path => path.steps.some(step => step.node?.id === selectedNodeId)) : undefined
  const selectedPath = selectedNodePath ?? paths.find(path => path.id === selectedPathId) ?? paths[0]

  useEffect(() => {
    if (selectedNodePath && selectedNodePath.id !== selectedPathId) {
      setSelectedPathId(selectedNodePath.id)
      return
    }
    if (selectedPathId && paths.some(path => path.id === selectedPathId)) return
    setSelectedPathId(paths[0]?.id ?? null)
  }, [paths, selectedNodePath, selectedPathId])

  return (
    <div className="w-full h-full flex flex-col" style={{ background: 'var(--cc-bg)' }}>
      <div className="h-10 flex items-center gap-3 px-4 shrink-0" style={{ borderBottom: '1px solid var(--cc-border)', background: 'var(--cc-panel)' }}>
        <Activity size={15} style={{ color: 'var(--cc-text-subtle)' }} />
        <span style={{ fontSize: 12, color: 'var(--cc-text)', fontWeight: 700 }}>Call Flow</span>
        <span style={{ fontSize: 11, color: 'var(--cc-text-subtle)' }}>{paths.length} readable paths from graph edges</span>
      </div>
      {!paths.length ? (
        <div className="flex-1 flex items-center justify-center"><div className="arch-empty">No API/call-flow paths found in the current filtered graph.</div></div>
      ) : (
        <div className="flex-1 flex min-h-0">
          <aside className="w-64 shrink-0 overflow-auto p-3" style={{ borderRight: '1px solid var(--cc-border)', background: 'var(--cc-panel)' }}>
            <div className="arch-eyebrow mb-2">Paths</div>
            <div className="space-y-2">
              {paths.slice(0, 60).map(path => (
                <button key={path.id} className={`arch-list-row ${selectedPath?.id === path.id ? 'arch-list-row-active' : ''}`} onClick={() => setSelectedPathId(path.id)}>
                  <span className="arch-row-label">{path.label}</span>
                  <span>{path.steps.length}</span>
                </button>
              ))}
            </div>
          </aside>
          <div className="flex-1 min-w-0">
            <DiagramCanvas width={FLOW_CANVAS.width} height={FLOW_CANVAS.height} initialZoom={1} autoFit={false}>
            <div className="grid gap-4" style={{ width: FLOW_CANVAS.width, minHeight: FLOW_CANVAS.height, gridTemplateColumns: `repeat(${COLUMNS.length}, minmax(190px, 1fr))` }}>
              {COLUMNS.map(column => (
                <div key={column.role} className="arch-flow-column">
                  <div className="arch-eyebrow">{column.label}</div>
                  <div className="mt-3 space-y-3">
                    {selectedPath?.steps.filter(step => step.role === column.role).map(step => (
                      <button
                        key={step.id}
                        className="arch-flow-step"
                        onClick={() => step.node && onSelectNode(step.node.id)}
                        onDoubleClick={() => step.node && onOpenNode(step.node)}
                      >
                        <span className="arch-flow-step-label">{step.label}</span>
                        <span className="arch-flow-step-path">{step.node?.file ?? step.role}</span>
                      </button>
                    ))}
                  </div>
                </div>
              ))}
            </div>
            </DiagramCanvas>
          </div>
        </div>
      )}
    </div>
  )
}

export function buildCallFlowPaths(nodes: GraphNode[], edges: GraphEdge[]): CallFlowPath[] {
  const byId = new Map(nodes.map(node => [node.id, node]))
  const apiEdges = edges.filter(edge => edge.type === 'ApiCall')
  const usedHandlerEdgeIds = new Set<string>()
  const paths: CallFlowPath[] = []
  for (const apiEdge of apiEdges) {
    const caller = byId.get(apiEdge.source)
    const endpoint = byId.get(apiEdge.target)
    if (!caller || !endpoint) continue
    const handlerEdge = edges.find(edge =>
      edge.type === 'EndpointHandler' && (edge.source === endpoint.id || edge.target === endpoint.id),
    )
    if (handlerEdge) usedHandlerEdgeIds.add(handlerEdge.id)
    const handlerId = handlerEdge ? handlerEdge.source === endpoint.id ? handlerEdge.target : handlerEdge.source : undefined
    const handler = handlerId ? byId.get(handlerId) : undefined
    const serviceEdge = handler ? edges.find(edge => edge.type === 'Calls' && edge.source === handler.id) : undefined
    const service = serviceEdge ? byId.get(serviceEdge.target) : undefined
    const modelEdge = [endpoint, handler, service].filter(Boolean).flatMap(node =>
      edges.filter(edge =>
        (edge.type === 'TypeReference' || edge.type === 'DataFlow') && (edge.source === node!.id || edge.target === node!.id),
      ),
    )[0]
    const model = modelEdge ? byId.get(modelEdge.source === endpoint.id || modelEdge.source === handler?.id || modelEdge.source === service?.id ? modelEdge.target : modelEdge.source) : undefined
    const steps: CallFlowStep[] = [
      { id: `${apiEdge.id}:caller`, label: caller.label, node: caller, role: roleFor(caller) },
      { id: `${apiEdge.id}:endpoint`, label: endpoint.label, node: endpoint, role: 'endpoint' },
      ...(handler ? [{ id: `${apiEdge.id}:handler`, label: handler.label, node: handler, role: 'handler' as const }] : []),
      ...(service ? [{ id: `${apiEdge.id}:service`, label: service.label, node: service, role: 'service' as const }] : []),
      ...(model ? [{ id: `${apiEdge.id}:model`, label: model.label, node: model, role: 'model' as const }] : []),
    ]
    paths.push({
      id: apiEdge.id,
      label: `${caller.label} -> ${endpoint.label}`,
      steps,
      edgeIds: [apiEdge.id, handlerEdge?.id, serviceEdge?.id, modelEdge?.id].filter(Boolean) as string[],
    })
  }
  for (const handlerEdge of edges.filter(edge => edge.type === 'EndpointHandler' && !usedHandlerEdgeIds.has(edge.id))) {
    const source = byId.get(handlerEdge.source)
    const target = byId.get(handlerEdge.target)
    const endpoint = source?.type === 'Endpoint' ? source : target?.type === 'Endpoint' ? target : undefined
    const handler = endpoint?.id === source?.id ? target : source
    if (!endpoint || !handler) continue
    const serviceEdge = edges.find(edge => edge.type === 'Calls' && edge.source === handler.id)
    const service = serviceEdge ? byId.get(serviceEdge.target) : undefined
    const modelEdge = [endpoint, handler, service].filter(Boolean).flatMap(node =>
      edges.filter(edge =>
        (edge.type === 'TypeReference' || edge.type === 'DataFlow') && (edge.source === node!.id || edge.target === node!.id),
      ),
    )[0]
    const model = modelEdge ? byId.get(modelEdge.source === endpoint.id || modelEdge.source === handler.id || modelEdge.source === service?.id ? modelEdge.target : modelEdge.source) : undefined
    paths.push({
      id: handlerEdge.id,
      label: `${endpoint.label} -> ${handler.label}`,
      steps: [
        { id: `${handlerEdge.id}:endpoint`, label: endpoint.label, node: endpoint, role: 'endpoint' },
        { id: `${handlerEdge.id}:handler`, label: handler.label, node: handler, role: 'handler' },
        ...(service ? [{ id: `${handlerEdge.id}:service`, label: service.label, node: service, role: 'service' as const }] : []),
        ...(model ? [{ id: `${handlerEdge.id}:model`, label: model.label, node: model, role: 'model' as const }] : []),
      ],
      edgeIds: [handlerEdge.id, serviceEdge?.id, modelEdge?.id].filter(Boolean) as string[],
    })
  }
  return paths
}

function roleFor(node: GraphNode): CallFlowStep['role'] {
  if (node.type === 'Component' || node.language === 'typescript' || node.language === 'qml') return 'frontend'
  if (node.type === 'Hook' || node.label.toLowerCase().includes('state')) return 'state'
  if (node.label.toLowerCase().includes('client') || node.label.toLowerCase().includes('fetch')) return 'api-client'
  return 'unknown'
}
