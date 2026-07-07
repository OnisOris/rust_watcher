import { Activity, AlertTriangle, ArrowRight, GitBranch } from 'lucide-react'
import { useEffect, useMemo, useState, type ReactNode } from 'react'
import { buildCallFlowGroups, buildCallFlowPaths, roleForCallFlowNode } from '../../api/callFlow'
import type { EdgeType, GraphEdge, GraphNode, TraceExplanation, TraceStep } from '../../types'
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

const PLACEHOLDER_LABELS: Partial<Record<CallFlowStep['role'], string>> = {
  state: 'No hook detected',
  'api-client': 'No API client detected',
  service: 'No service detected',
  model: 'No model detected',
}

const FLOW_CANVAS = { width: 1740, height: 620 }

export function CallFlowView({ nodes, edges, selectedNodeId, onSelectNode, onOpenNode }: CallFlowViewProps) {
  const [includeTypeOnly, setIncludeTypeOnly] = useState(true)
  const paths = useMemo(() => buildCallFlowPaths(nodes, edges, { includeTypeOnly }), [edges, includeTypeOnly, nodes])
  const groups = useMemo(() => buildCallFlowGroups(paths), [paths])
  const [selectedGroupId, setSelectedGroupId] = useState<string | null>(groups[0]?.id ?? null)
  const [selectedPathId, setSelectedPathId] = useState<string | null>(groups[0]?.paths[0]?.id ?? null)
  const selectedNodePath = selectedNodeId ? paths.find(path => path.steps.some(step => step.node?.id === selectedNodeId)) : undefined
  const selectedGroup = groups.find(group => group.id === selectedGroupId) ?? groups[0]
  const heuristicPath = selectedNodePath ?? selectedGroup?.paths.find(path => path.id === selectedPathId) ?? selectedGroup?.paths[0] ?? paths[0]
  const nodesById = useMemo(() => new Map(nodes.map(node => [node.id, node])), [nodes])
  const edgesById = useMemo(() => new Map(edges.map(edge => [edge.id, edge])), [edges])
  const [trace, setTrace] = useState<TraceExplanation | null>(null)
  const tracePath = useMemo(() => trace && heuristicPath ? traceToCallFlowPath(trace, heuristicPath, nodesById, edgesById) : null, [edgesById, heuristicPath, nodesById, trace])
  const selectedPath = tracePath ?? heuristicPath

  useEffect(() => {
    if (selectedNodePath) {
      const group = groups.find(candidate => candidate.routeKey === selectedNodePath.routeKey)
      if (group) setSelectedGroupId(group.id)
      setSelectedPathId(selectedNodePath.id)
      return
    }
    if (selectedGroupId && groups.some(group => group.id === selectedGroupId)) {
      const group = groups.find(candidate => candidate.id === selectedGroupId)
      if (selectedPathId && group?.paths.some(path => path.id === selectedPathId)) return
      setSelectedPathId(group?.paths[0]?.id ?? null)
      return
    }
    setSelectedGroupId(groups[0]?.id ?? null)
    setSelectedPathId(groups[0]?.paths[0]?.id ?? null)
  }, [groups, selectedGroupId, selectedNodePath, selectedPathId])

  useEffect(() => {
    setTrace(null)
    if (!heuristicPath?.method || !heuristicPath.path) return
    const controller = new AbortController()
    const query = new URLSearchParams({ method: heuristicPath.method, path: heuristicPath.path })
    fetch(`/api/trace/route/by-path?${query.toString()}`, { signal: controller.signal })
      .then(response => response.ok ? response.json() : null)
      .then((nextTrace: TraceExplanation | null) => {
        if (nextTrace) setTrace(nextTrace)
      })
      .catch(() => {
        if (!controller.signal.aborted) setTrace(null)
      })
    return () => controller.abort()
  }, [heuristicPath?.method, heuristicPath?.path, heuristicPath?.routeKey])

  return (
    <div className="w-full h-full flex flex-col" style={{ background: 'var(--cc-bg)' }}>
      <div className="min-h-10 flex items-center gap-3 px-4 py-2 shrink-0" style={{ borderBottom: '1px solid var(--cc-border)', background: 'var(--cc-panel)' }}>
        <Activity size={15} style={{ color: 'var(--cc-text-subtle)' }} />
        <span style={{ fontSize: 12, color: 'var(--cc-text)', fontWeight: 700 }}>Call Flow</span>
        <span style={{ fontSize: 11, color: 'var(--cc-text-subtle)' }}>
          {groups.length} endpoint groups · {paths.length} caller paths
        </span>
        <button
          className={`arch-mode-button ml-auto ${includeTypeOnly ? 'active' : ''}`}
          onClick={() => setIncludeTypeOnly(value => !value)}
        >
          Type refs
        </button>
      </div>
      {!paths.length ? (
        <div className="flex-1 flex items-center justify-center"><div className="arch-empty">No API/call-flow paths found in the current filtered graph.</div></div>
      ) : (
        <div className="flex-1 flex min-h-0">
          <aside className="w-72 shrink-0 overflow-auto p-3" style={{ borderRight: '1px solid var(--cc-border)', background: 'var(--cc-panel)' }}>
            <div className="arch-eyebrow mb-2">Endpoint Groups</div>
            <div className="space-y-2">
              {groups.slice(0, 80).map(group => {
                const active = selectedGroup?.id === group.id
                return (
                  <div key={group.id}>
                    <button
                      className={`arch-list-row ${active ? 'arch-list-row-active' : ''}`}
                      onClick={() => {
                        setSelectedGroupId(group.id)
                        setSelectedPathId(group.paths[0]?.id ?? null)
                      }}
                    >
                      <span className="arch-row-label">{group.routeKey}</span>
                      <span>{group.callerLabels.length} callers</span>
                    </button>
                    {active ? (
                      <div className="mt-1 ml-3 space-y-1">
                        {group.paths.map(path => (
                          <button
                            key={path.id}
                            className={`arch-list-row ${heuristicPath?.id === path.id ? 'arch-list-row-active' : ''}`}
                            style={{ minHeight: 28, fontSize: 11 }}
                            onClick={() => setSelectedPathId(path.id)}
                          >
                            <span className="arch-row-label">{callerLabel(path)}</span>
                            <span>{path.steps.filter(step => !step.isPlaceholder).length}</span>
                          </button>
                        ))}
                      </div>
                    ) : null}
                  </div>
                )
              })}
            </div>
          </aside>

          <div className="flex-1 min-w-0">
            <DiagramCanvas width={FLOW_CANVAS.width} height={FLOW_CANVAS.height} initialZoom={0.9}>
              {selectedPath ? (
                <div style={{ width: FLOW_CANVAS.width, minHeight: FLOW_CANVAS.height, padding: 24 }}>
                  <div className="mb-5 flex items-center gap-3">
                    <span style={{ color: 'var(--cc-text)', fontSize: 14, fontWeight: 800 }}>{selectedPath.routeKey}</span>
                    <span className="arch-pill">{selectedPath.source === 'trace' ? 'trace' : 'heuristic'}</span>
                    {selectedPath.traceWarning ? (
                      <span className="arch-pill" style={{ color: '#B45309', borderColor: '#F59E0B', background: '#FFFBEB' }}>
                        <AlertTriangle size={12} /> graph differs
                      </span>
                    ) : null}
                  </div>
                  <div className="flex items-stretch gap-3">
                    {COLUMNS.map((column, index) => (
                      <div key={column.role} className="flex items-stretch gap-3">
                        <div className="arch-flow-column" style={{ width: 205, minHeight: 420 }}>
                          <div className="arch-eyebrow">{column.label}</div>
                          <div className="mt-3 space-y-3">
                            {stepsForRole(selectedPath, column.role).map(step => (
                              <FlowStepCard
                                key={step.id}
                                step={step}
                                onSelectNode={onSelectNode}
                                onOpenNode={onOpenNode}
                              />
                            ))}
                          </div>
                        </div>
                        {index < COLUMNS.length - 1 ? (
                          <div className="flex items-center justify-center" style={{ width: 24, color: 'var(--cc-text-subtle)' }}>
                            <ArrowRight size={22} strokeWidth={2.2} />
                          </div>
                        ) : null}
                      </div>
                    ))}
                  </div>
                </div>
              ) : null}
            </DiagramCanvas>
          </div>

          <CallFlowDetails path={selectedPath} heuristicPath={heuristicPath} trace={trace} />
        </div>
      )}
    </div>
  )
}

function FlowStepCard({
  step,
  onSelectNode,
  onOpenNode,
}: {
  step: CallFlowStep
  onSelectNode: (id: string | null) => void
  onOpenNode: (node: GraphNode) => void
}) {
  const clickable = !step.isPlaceholder && step.node
  return (
    <button
      data-no-pan="true"
      disabled={!clickable}
      className="arch-flow-step"
      onClick={() => step.node && onSelectNode(step.node.id)}
      onDoubleClick={() => step.node && onOpenNode(step.node)}
      style={{
        opacity: step.isPlaceholder ? 0.58 : 1,
        background: step.isPlaceholder ? 'var(--cc-card)' : 'var(--cc-panel)',
        cursor: clickable ? 'pointer' : 'default',
      }}
    >
      <span className="arch-flow-step-label">{step.label}</span>
      <span className="arch-flow-step-path">{step.file || step.node?.file || step.role}</span>
    </button>
  )
}

function CallFlowDetails({
  path,
  heuristicPath,
  trace,
}: {
  path?: CallFlowPath
  heuristicPath?: CallFlowPath
  trace: TraceExplanation | null
}) {
  return (
    <aside className="w-80 shrink-0 overflow-auto p-4" style={{ borderLeft: '1px solid var(--cc-border)', background: 'var(--cc-panel)' }}>
      {path ? (
        <div className="space-y-4">
          <div>
            <div className="arch-eyebrow">Selected Path</div>
            <div className="mt-2" style={{ color: 'var(--cc-text)', fontSize: 13, fontWeight: 800 }}>
              {path.steps.filter(step => !step.isPlaceholder).map(step => step.label).join(' -> ')}
            </div>
            <div className="mt-1" style={{ color: 'var(--cc-text-subtle)', fontSize: 11 }}>
              Source: {path.source}
              {trace && heuristicPath?.source === 'heuristic' ? ' · trace available' : ''}
            </div>
          </div>
          {path.traceWarning ? (
            <div className="arch-empty" style={{ textAlign: 'left', padding: 10 }}>
              {path.traceWarning}
            </div>
          ) : null}
          <DetailSection title="Edges">
            {Object.entries(path.edgeTypes).length ? Object.entries(path.edgeTypes).map(([type, count]) => (
              <div key={type} className="flex justify-between" style={{ fontSize: 12, color: 'var(--cc-text-muted)' }}>
                <span>{type}</span>
                <span style={{ color: 'var(--cc-text)' }}>{count}</span>
              </div>
            )) : <div style={{ fontSize: 12, color: 'var(--cc-text-subtle)' }}>No graph edge breakdown available.</div>}
          </DetailSection>
          <DetailSection title="Files">
            {path.files.length ? path.files.map(file => (
              <div key={file} title={file} style={{ fontSize: 11, color: 'var(--cc-text-muted)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{file}</div>
            )) : <div style={{ fontSize: 12, color: 'var(--cc-text-subtle)' }}>No file metadata.</div>}
          </DetailSection>
          <DetailSection title="Actions">
            <button
              disabled
              style={{
                minHeight: 32,
                width: '100%',
                border: '1px solid var(--cc-border)',
                borderRadius: 7,
                background: 'var(--cc-card)',
                color: 'var(--cc-text-faint)',
                fontSize: 11,
                fontWeight: 700,
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                gap: 6,
              }}
            >
              <GitBranch size={13} />
              Show as graph
            </button>
          </DetailSection>
          <div style={{ fontSize: 11, color: 'var(--cc-text-faint)' }}>{path.edgeIds.length} underlying edge ids preserved</div>
        </div>
      ) : (
        <div style={{ fontSize: 12, color: 'var(--cc-text-subtle)' }}>Select an endpoint group to inspect a readable execution path.</div>
      )}
    </aside>
  )
}

function DetailSection({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div>
      <div className="arch-eyebrow">{title}</div>
      <div className="space-y-2 mt-2">{children}</div>
    </div>
  )
}

function stepsForRole(path: CallFlowPath, role: CallFlowStep['role']) {
  return path.steps.filter(step => step.role === role)
}

function callerLabel(path: CallFlowPath) {
  return path.steps.find(step => !step.isPlaceholder && (step.role === 'frontend' || step.role === 'state' || step.role === 'api-client'))?.label ?? 'Backend route'
}

function traceToCallFlowPath(
  trace: TraceExplanation,
  fallback: CallFlowPath,
  nodesById: Map<string, GraphNode>,
  edgesById: Map<string, GraphEdge>,
): CallFlowPath | null {
  const stepsByRole = new Map<CallFlowStep['role'], CallFlowStep[]>()
  for (const [index, traceStep] of trace.steps.entries()) {
    const node = traceStep.nodeId ? nodesById.get(traceStep.nodeId) : undefined
    const role = roleForTraceStep(traceStep, node)
    if (role === 'unknown') continue
    const step: CallFlowStep = {
      id: `trace:${traceStep.id}:${index}`,
      label: node?.label ?? traceStep.title,
      node,
      file: traceStep.file ?? node?.file,
      role,
    }
    const existing = stepsByRole.get(role) ?? []
    if (!existing.some(candidate => candidate.label === step.label && candidate.file === step.file)) {
      stepsByRole.set(role, [...existing, step])
    }
  }

  if (!stepsByRole.size) return null
  const steps = withPlaceholders(stepsByRole, fallback.routeKey)
  const traceEdgeIds = trace.steps.flatMap(step => step.edgeId ? [step.edgeId] : [])
  const traceEdges = traceEdgeIds.flatMap(edgeId => edgesById.get(edgeId) ? [edgesById.get(edgeId)!] : [])
  const missingHeuristicEdges = fallback.edgeIds.filter(edgeId => !traceEdgeIds.includes(edgeId))
  return {
    ...fallback,
    id: `trace:${fallback.id}`,
    source: 'trace',
    steps,
    edgeIds: traceEdgeIds,
    edgeTypes: traceEdges.length ? edgeTypeCounts(traceEdges) : fallback.edgeTypes,
    files: unique([
      ...steps.flatMap(step => step.file ? [normalizePath(step.file)] : []),
      ...trace.steps.flatMap(step => step.file ? [normalizePath(step.file)] : []),
    ]),
    traceWarning: missingHeuristicEdges.length ? 'Trace data loaded, but some heuristic graph edges were not present in the route trace.' : undefined,
  }
}

function withPlaceholders(stepsByRole: Map<CallFlowStep['role'], CallFlowStep[]>, routeKey: string) {
  const steps: CallFlowStep[] = []
  for (const column of COLUMNS) {
    const actual = stepsByRole.get(column.role)
    if (actual?.length) {
      steps.push(...actual)
      continue
    }
    const placeholder = PLACEHOLDER_LABELS[column.role]
    if (placeholder) {
      steps.push({ id: `trace-placeholder:${routeKey}:${column.role}`, label: placeholder, role: column.role, isPlaceholder: true })
    }
  }
  return steps
}

function roleForTraceStep(step: TraceStep, node?: GraphNode): CallFlowStep['role'] {
  if (node) {
    const role = roleForCallFlowNode(node)
    if (role !== 'unknown') return role
  }
  switch (step.kind) {
    case 'Caller':
      return 'frontend'
    case 'StateUpdate':
    case 'PropertyBinding':
      return 'state'
    case 'ApiRequest':
      return 'api-client'
    case 'Endpoint':
      return 'endpoint'
    case 'EndpointHandler':
    case 'BackendHandler':
      return 'handler'
    case 'ServiceCall':
    case 'ExternalDependency':
      return 'service'
    case 'ModelUse':
    case 'ReturnValue':
    case 'ApiResponse':
      return 'model'
    default:
      return 'unknown'
  }
}

function edgeTypeCounts(edges: GraphEdge[]) {
  const counts: Partial<Record<EdgeType, number>> = {}
  for (const edge of edges) counts[edge.type] = (counts[edge.type] ?? 0) + Math.max(1, edge.bundledCount ?? 1)
  return counts
}

function normalizePath(path?: string | null) {
  return (path ?? '').replaceAll('\\', '/').replace(/^\/+/, '')
}

function unique<T>(values: T[]) {
  return [...new Set(values.filter(Boolean))]
}
