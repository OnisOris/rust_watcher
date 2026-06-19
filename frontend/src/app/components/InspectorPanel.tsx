import { useEffect, useMemo, useState, type ReactNode } from 'react'
import { ExternalLink, BookMarked, Pin, ChevronRight, ArrowUpRight, ArrowDownRight, Users, GitBranch, Zap, Layers, AlertTriangle } from 'lucide-react'
import type { AnalyzerStatus, AppState, DiagnosticRecord, EdgeConfidence, GraphNode, GraphEdge, NodeDetailsResponse, ReferenceRecord } from '../types'

interface InspectorPanelProps {
  selectedNode: GraphNode | null
  nodes: GraphNode[]
  edges: GraphEdge[]
  projectName?: string | null
  analyzerStatus?: AnalyzerStatus
  appState?: AppState
  filesCount?: number
  message?: string | null
  onTogglePin: (id: string) => void
  onSelectNode: (id: string) => void
  onOpenInEditor: (node: GraphNode) => void
}

const NODE_TYPE_COLORS: Record<string, string> = {
  File: '#3B82F6', Module: '#8B5CF6', Struct: '#06B6D4', Enum: '#F59E0B',
  Trait: '#10B981', Impl: '#6366F1', Function: '#EC4899', Method: '#F97316',
  Component: '#14B8A6', Hook: '#A855F7', Interface: '#22C55E', TypeAlias: '#84CC16',
  Endpoint: '#E11D48', Macro: '#EF4444', ExternalCrate: '#7D8795',
}

export function InspectorPanel({
  selectedNode,
  nodes,
  edges,
  projectName,
  analyzerStatus = 'Starting',
  appState = 'empty',
  filesCount = 0,
  message,
  onTogglePin,
  onSelectNode,
  onOpenInEditor,
}: InspectorPanelProps) {
  if (!selectedNode) {
    return (
      <ProjectOverview
        nodes={nodes}
        edges={edges}
        projectName={projectName}
        analyzerStatus={analyzerStatus}
        appState={appState}
        filesCount={filesCount}
        message={message}
        onSelectNode={onSelectNode}
      />
    )
  }
  return <NodeInspector node={selectedNode} nodes={nodes} edges={edges} onTogglePin={onTogglePin} onSelectNode={onSelectNode} onOpenInEditor={onOpenInEditor} />
}

// ── Project overview (nothing selected) ────────────────────────────────────
function ProjectOverview({
  nodes,
  edges,
  projectName,
  analyzerStatus,
  appState,
  filesCount,
  message,
  onSelectNode,
}: {
  nodes: GraphNode[]
  edges: GraphEdge[]
  projectName?: string | null
  analyzerStatus: AnalyzerStatus
  appState: AppState
  filesCount: number
  message?: string | null
  onSelectNode: (id: string) => void
}) {
  const visibleCrates = new Set(nodes.map(n => n.crate).filter((crateName): crateName is string => !!crateName && crateName !== 'external'))
  const crateCount = visibleCrates.size || nodes.filter(n => n.id.startsWith('crate:')).length
  const analyzerColor = analyzerStatus === 'Error' ? '#F87171' : analyzerStatus === 'Indexing' || analyzerStatus === 'Starting' || analyzerStatus === 'Fallback' ? '#F59E0B' : '#34D399'

  const topConnected = [...nodes]
    .map(n => ({
      ...n,
      inCount: edges.filter(e => e.target === n.id).length,
      outCount: edges.filter(e => e.source === n.id).length,
    }))
    .sort((a, b) => (b.inCount + b.outCount) - (a.inCount + a.outCount))
    .slice(0, 5)

  return (
    <div
      className="flex flex-col overflow-hidden"
      style={{ width: 340, background: 'var(--cc-panel)', borderLeft: '1px solid var(--cc-border)', fontFamily: 'Inter, sans-serif' }}
    >
      <PanelHeader title="Project Overview" subtitle={projectName ?? 'workspace'} />

      <div className="overflow-y-auto flex-1 p-3 space-y-4" style={{ scrollbarWidth: 'thin', scrollbarColor: 'var(--cc-border) transparent' }}>
        {/* stats grid */}
        <div className="grid grid-cols-3 gap-2">
          <StatCard label="Nodes" value={nodes.length} color="#06B6D4" />
          <StatCard label="Edges" value={edges.length} color="#8B5CF6" />
          <StatCard label="Crates" value={crateCount} color="#10B981" />
        </div>

        {/* analyzer status */}
        <Card>
          <div className="flex items-center gap-2 mb-2">
            <div className="w-2 h-2 rounded-full" style={{ background: analyzerColor }} />
            <span style={{ fontSize: 11, color: 'var(--cc-text)', fontWeight: 500 }}>rust-analyzer</span>
            <span style={{ marginLeft: 'auto', fontSize: 10, color: analyzerColor }}>{analyzerStatus}</span>
          </div>
          <div style={{ fontSize: 10, color: 'var(--cc-text-subtle)' }}>
            {message ?? appState} · {filesCount} files indexed
          </div>
        </Card>

        {/* hotspots */}
        <Section label="Hotspots">
          {topConnected.slice(0, 4).map(node => (
            <div key={node.id} className="flex items-start gap-2 py-1.5">
              <div className="w-1.5 h-1.5 rounded-full mt-1.5 shrink-0" style={{ background: NODE_TYPE_COLORS[node.type] ?? '#7D8795' }} />
              <div>
                <div style={{ fontSize: 11, color: 'var(--cc-text)', fontFamily: 'JetBrains Mono, monospace' }}>{node.label}</div>
                <div style={{ fontSize: 10, color: 'var(--cc-text-subtle)' }}>{node.inCount + node.outCount} links · {node.type}</div>
              </div>
            </div>
          ))}
        </Section>

        {/* top connected */}
        <Section label="Top Connected Nodes">
          {topConnected.map(n => (
            <button
              key={n.id}
              onClick={() => onSelectNode(n.id)}
              className="flex items-center gap-2 w-full rounded py-1.5 px-2 transition-colors"
              style={{ background: 'none', cursor: 'pointer' }}
            >
              <TypeDot type={n.type} />
              <span style={{ fontSize: 11, color: 'var(--cc-text-muted)', flex: 1, textAlign: 'left', fontFamily: 'JetBrains Mono, monospace' }}>{n.label}</span>
              <span style={{ fontSize: 10, color: 'var(--cc-text-subtle)' }}>{n.inCount + n.outCount} links</span>
            </button>
          ))}
        </Section>
      </div>
    </div>
  )
}

// ── Node inspector (something selected) ────────────────────────────────────
function NodeInspector({ node, nodes, edges, onTogglePin, onSelectNode, onOpenInEditor }: {
  node: GraphNode; nodes: GraphNode[]; edges: GraphEdge[]; onTogglePin: (id: string) => void; onSelectNode: (id: string) => void; onOpenInEditor: (node: GraphNode) => void
}) {
  const nodeMap = useMemo(() => new Map(nodes.map(n => [n.id, n])), [nodes])
  const [details, setDetails] = useState<NodeDetailsResponse | null>(null)

  useEffect(() => {
    let cancelled = false
    setDetails(null)
    fetch(`/api/node/${encodeURIComponent(node.id)}/details`)
      .then(response => response.ok ? response.json() : null)
      .then((payload: NodeDetailsResponse | null) => {
        if (!cancelled) setDetails(payload)
      })
      .catch(() => {
        if (!cancelled) setDetails(null)
      })
    return () => { cancelled = true }
  }, [node.id])

  const outgoing = details?.outgoingEdges ?? edges.filter(e => e.source === node.id)
  const incoming = details?.incomingEdges ?? edges.filter(e => e.target === node.id)

  const callers = details?.callers ?? incoming.filter(e => e.type === 'Calls' || e.type === 'EndpointHandler').map(e => nodeMap.get(e.source)).filter(Boolean) as GraphNode[]
  const callees = details?.callees ?? outgoing.filter(e => e.type === 'Calls' || e.type === 'EndpointHandler').map(e => nodeMap.get(e.target)).filter(Boolean) as GraphNode[]
  const apiCallers = incoming.filter(e => e.type === 'ApiCall').map(e => nodeMap.get(e.source)).filter(Boolean) as GraphNode[]
  const apiTargets = outgoing.filter(e => e.type === 'ApiCall').map(e => nodeMap.get(e.target)).filter(Boolean) as GraphNode[]
  const renders = outgoing.filter(e => e.type === 'Renders').map(e => nodeMap.get(e.target)).filter(Boolean) as GraphNode[]
  const renderedBy = incoming.filter(e => e.type === 'Renders').map(e => nodeMap.get(e.source)).filter(Boolean) as GraphNode[]
  const typeRefs = details?.relatedTypes.length ? details.relatedTypes : outgoing.filter(e => e.type === 'TypeReference').map(e => nodeMap.get(e.target)).filter(Boolean) as GraphNode[]
  const dataFlowTargets = outgoing.filter(e => e.type === 'DataFlow').map(e => nodeMap.get(e.target)).filter(Boolean) as GraphNode[]
  const implementors = incoming.filter(e => e.type === 'Implements').map(e => nodeMap.get(e.source)).filter(Boolean) as GraphNode[]
  const usedBy = incoming.filter(e => e.type === 'Uses' || e.type === 'TypeReference').map(e => nodeMap.get(e.source)).filter(Boolean) as GraphNode[]
  const references = details?.references ?? referenceRecordsFromNodes(usedBy)
  const diagnostics = details?.diagnostics ?? []
  const callerConfidence = confidenceByNode(incoming, 'source')
  const calleeConfidence = confidenceByNode(outgoing, 'target')

  const typeColor = NODE_TYPE_COLORS[node.type] ?? '#7D8795'

  return (
    <div
      className="flex flex-col overflow-hidden"
      style={{ width: 340, background: 'var(--cc-panel)', borderLeft: '1px solid var(--cc-border)', fontFamily: 'Inter, sans-serif' }}
    >
      <PanelHeader title="Inspector" subtitle={node.type} subtitleColor={typeColor} />

      <div className="overflow-y-auto flex-1 p-3 space-y-3" style={{ scrollbarWidth: 'thin', scrollbarColor: 'var(--cc-border) transparent' }}>
        {/* symbol card */}
        <Card>
          <div className="flex items-start gap-3 mb-3">
            <div className="rounded flex items-center justify-center shrink-0" style={{ width: 36, height: 36, background: `${typeColor}18`, border: `1px solid ${typeColor}30` }}>
              <TypeIcon type={node.type} color={typeColor} />
            </div>
            <div className="flex-1 min-w-0">
              <div style={{ fontSize: 15, color: 'var(--cc-text)', fontWeight: 600, fontFamily: 'JetBrains Mono, monospace', lineHeight: 1.3 }}>{node.label}</div>
              <div style={{ fontSize: 11, color: 'var(--cc-text-subtle)', marginTop: 2 }}>{node.type} · {node.crate ?? 'unknown'}</div>
            </div>
          </div>

          {/* badges row */}
          <div className="flex flex-wrap gap-1.5 mb-3">
            {node.visibility && <VisBadge vis={node.visibility} />}
            {node.isAsync && <Badge color="#06B6D4">async</Badge>}
            {node.isUnsafe && <Badge color="#F87171">unsafe</Badge>}
            {node.isGeneric && <Badge color="#8B5CF6">generic</Badge>}
            {diagnostics.length > 0 && <Badge color={diagnostics.some(d => d.severity === 'Error') ? '#F87171' : '#F59E0B'}>{diagnostics.length} diagnostics</Badge>}
          </div>

          {/* signature */}
          {node.signature && (
            <div className="rounded p-2" style={{ background: 'var(--cc-surface)', border: '1px solid var(--cc-border)' }}>
              <pre style={{ fontSize: 10, color: 'var(--cc-text-muted)', fontFamily: 'JetBrains Mono, monospace', whiteSpace: 'pre-wrap', lineHeight: 1.5, margin: 0 }}>
                {node.signature}
              </pre>
            </div>
          )}
        </Card>

        {/* location */}
        {(node.file || node.module) && (
          <Card>
            <SectionLabel label="Location" />
            {node.file && <InfoRow label="File" value={node.file} mono />}
            {node.module && <InfoRow label="Module" value={node.module} mono />}
            {node.line && <InfoRow label="Line" value={`L${node.line}`} mono />}
          </Card>
        )}

        {/* call graph */}
        {(callers.length > 0 || callees.length > 0 || apiCallers.length > 0 || apiTargets.length > 0 || renders.length > 0 || renderedBy.length > 0) && (
          <Card>
            <SectionLabel label="Call Graph" />
            {callers.length > 0 && (
              <NodeList label="Called by" icon={<ArrowDownRight size={11} color="#06B6D4" />} nodes={callers} onSelect={onSelectNode} confidenceByNodeId={callerConfidence} />
            )}
            {callees.length > 0 && (
              <NodeList label="Calls" icon={<ArrowUpRight size={11} color="#EC4899" />} nodes={callees} onSelect={onSelectNode} confidenceByNodeId={calleeConfidence} />
            )}
            {apiCallers.length > 0 && (
              <NodeList label="API called by" icon={<ArrowDownRight size={11} color="#E11D48" />} nodes={apiCallers} onSelect={onSelectNode} />
            )}
            {apiTargets.length > 0 && (
              <NodeList label="API calls" icon={<ArrowUpRight size={11} color="#E11D48" />} nodes={apiTargets} onSelect={onSelectNode} />
            )}
            {renders.length > 0 && (
              <NodeList label="Renders" icon={<ArrowUpRight size={11} color="#14B8A6" />} nodes={renders} onSelect={onSelectNode} />
            )}
            {renderedBy.length > 0 && (
              <NodeList label="Rendered by" icon={<ArrowDownRight size={11} color="#14B8A6" />} nodes={renderedBy} onSelect={onSelectNode} />
            )}
          </Card>
        )}

        {/* type references */}
        {typeRefs.length > 0 && (
          <Card>
            <NodeList label="Type References" icon={<Layers size={11} color="#3B82F6" />} nodes={typeRefs} onSelect={onSelectNode} />
          </Card>
        )}

        {/* data flow */}
        {dataFlowTargets.length > 0 && (
          <Card>
            <NodeList label="Data Flow →" icon={<GitBranch size={11} color="#8B5CF6" />} nodes={dataFlowTargets} onSelect={onSelectNode} />
          </Card>
        )}

        {/* implementors (for traits) */}
        {implementors.length > 0 && (
          <Card>
            <NodeList label="Implementors" icon={<Users size={11} color="#10B981" />} nodes={implementors} onSelect={onSelectNode} />
          </Card>
        )}

        {/* used by */}
        {usedBy.length > 0 && (
          <Card>
            <NodeList label="Used by" icon={<ChevronRight size={11} color="#F59E0B" />} nodes={usedBy.slice(0, 6)} onSelect={onSelectNode} />
          </Card>
        )}

        {references.length > 0 && (
          <Card>
            <ReferenceList references={references.slice(0, 8)} onSelect={onSelectNode} />
          </Card>
        )}

        {diagnostics.length > 0 && (
          <Card>
            <DiagnosticsList diagnostics={diagnostics} />
          </Card>
        )}

        {/* stats */}
        <Card>
          <div className="grid grid-cols-2 gap-2">
            <SmallStat label="Incoming" value={incoming.length} />
            <SmallStat label="Outgoing" value={outgoing.length} />
          </div>
        </Card>

        {/* actions */}
        <div className="grid grid-cols-2 gap-2">
          <ActionBtn icon={<BookMarked size={13} />} label="Bookmark" onClick={() => {}} />
          <ActionBtn icon={<Pin size={13} />} label={node.pinned ? 'Unpin Node' : 'Pin Node'} onClick={() => onTogglePin(node.id)} active={!!node.pinned} />
          <ActionBtn icon={<ExternalLink size={13} />} label="Open in Editor" onClick={() => onOpenInEditor(node)} disabled={!node.file} />
        </div>
      </div>
    </div>
  )
}

// ── Mini components ─────────────────────────────────────────────────────────
function PanelHeader({ title, subtitle, subtitleColor }: { title: string; subtitle?: string; subtitleColor?: string }) {
  const subtitleBg = subtitleColor ? `${subtitleColor}18` : 'var(--cc-elevated)'
  return (
    <div className="flex items-center gap-2 px-3 shrink-0" style={{ height: 40, borderBottom: '1px solid var(--cc-border)' }}>
      <span style={{ color: 'var(--cc-text-muted)', fontSize: 11, fontWeight: 600, letterSpacing: '0.08em', textTransform: 'uppercase' }}>{title}</span>
      {subtitle && (
        <span style={{ marginLeft: 'auto', fontSize: 11, color: subtitleColor ?? 'var(--cc-text-subtle)', background: subtitleBg, padding: '2px 7px', borderRadius: 4 }}>
          {subtitle}
        </span>
      )}
    </div>
  )
}

function Card({ children }: { children: ReactNode }) {
  return <div className="rounded-lg p-3" style={{ background: 'var(--cc-card)', border: '1px solid var(--cc-border)' }}>{children}</div>
}

function Section({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div>
      <p style={{ fontSize: 10, fontWeight: 600, color: 'var(--cc-text-faint)', letterSpacing: '0.07em', textTransform: 'uppercase', marginBottom: 6 }}>{label}</p>
      {children}
    </div>
  )
}

function SectionLabel({ label }: { label: string }) {
  return <p style={{ fontSize: 10, fontWeight: 600, color: 'var(--cc-text-faint)', letterSpacing: '0.07em', textTransform: 'uppercase', marginBottom: 6 }}>{label}</p>
}

function InfoRow({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="flex items-start gap-2 py-0.5">
      <span style={{ fontSize: 11, color: 'var(--cc-text-subtle)', width: 48, shrink: 0 }}>{label}</span>
      <span style={{ fontSize: 11, color: 'var(--cc-text-muted)', fontFamily: mono ? 'JetBrains Mono, monospace' : 'inherit', wordBreak: 'break-all' }}>{value}</span>
    </div>
  )
}

function StatCard({ label, value, color }: { label: string; value: number; color: string }) {
  return (
    <div className="rounded-lg p-2 text-center" style={{ background: 'var(--cc-card)', border: '1px solid var(--cc-border)' }}>
      <div style={{ fontSize: 20, fontWeight: 700, color, lineHeight: 1 }}>{value}</div>
      <div style={{ fontSize: 10, color: 'var(--cc-text-subtle)', marginTop: 2 }}>{label}</div>
    </div>
  )
}

function SmallStat({ label, value }: { label: string; value: number }) {
  return (
    <div className="text-center py-1">
      <div style={{ fontSize: 16, fontWeight: 600, color: 'var(--cc-text-muted)' }}>{value}</div>
      <div style={{ fontSize: 10, color: 'var(--cc-text-faint)' }}>{label}</div>
    </div>
  )
}

function NodeList({ label, icon, nodes, onSelect, confidenceByNodeId }: {
  label: string
  icon: ReactNode
  nodes: GraphNode[]
  onSelect: (id: string) => void
  confidenceByNodeId?: Map<string, EdgeConfidence>
}) {
  return (
    <div className="mb-1">
      <div className="flex items-center gap-1.5 mb-1.5">
        {icon}
        <span style={{ fontSize: 10, color: 'var(--cc-text-subtle)', fontWeight: 500 }}>{label} ({nodes.length})</span>
      </div>
      {nodes.map(n => (
        <button
          key={n.id}
          onClick={() => onSelect(n.id)}
          className="flex items-center gap-2 w-full rounded py-1 px-1.5 transition-colors"
          style={{ background: 'none', cursor: 'pointer' }}
        >
          <TypeDot type={n.type} />
          <span style={{ fontSize: 11, color: 'var(--cc-text-muted)', fontFamily: 'JetBrains Mono, monospace', textAlign: 'left', flex: 1 }}>{n.label}</span>
          {confidenceByNodeId?.get(n.id) && <ConfidenceBadge confidence={confidenceByNodeId.get(n.id)!} />}
          <ChevronRight size={10} color="var(--cc-text-faint)" />
        </button>
      ))}
    </div>
  )
}

function ReferenceList({ references, onSelect }: { references: ReferenceRecord[]; onSelect: (id: string) => void }) {
  return (
    <div>
      <div className="flex items-center gap-1.5 mb-1.5">
        <ChevronRight size={11} color="#F59E0B" />
        <span style={{ fontSize: 10, color: 'var(--cc-text-subtle)', fontWeight: 500 }}>References ({references.length})</span>
      </div>
      {references.map((reference, index) => (
        <button
          key={`${reference.location.file}:${reference.location.line}:${reference.location.character}:${reference.node?.id ?? index}`}
          onClick={() => reference.node && onSelect(reference.node.id)}
          disabled={!reference.node}
          className="flex items-center gap-2 w-full rounded py-1 px-1.5 transition-colors"
          style={{ background: 'none', cursor: reference.node ? 'pointer' : 'default', opacity: reference.node ? 1 : 0.82 }}
        >
          <TypeDot type={reference.node?.type ?? 'File'} />
          <span style={{ fontSize: 11, color: 'var(--cc-text-muted)', fontFamily: 'JetBrains Mono, monospace', textAlign: 'left', flex: 1 }}>
            {reference.node?.label ?? reference.location.file}
          </span>
          <span style={{ fontSize: 10, color: 'var(--cc-text-subtle)', fontFamily: 'JetBrains Mono, monospace' }}>
            L{reference.location.line}
          </span>
        </button>
      ))}
    </div>
  )
}

function DiagnosticsList({ diagnostics }: { diagnostics: DiagnosticRecord[] }) {
  return (
    <div>
      <div className="flex items-center gap-1.5 mb-1.5">
        <AlertTriangle size={11} color="#F87171" />
        <span style={{ fontSize: 10, color: 'var(--cc-text-subtle)', fontWeight: 500 }}>Diagnostics ({diagnostics.length})</span>
      </div>
      {diagnostics.map(diagnostic => (
        <div key={diagnostic.id} className="rounded p-2 mb-1.5" style={{ background: 'var(--cc-surface)', border: '1px solid var(--cc-border)' }}>
          <div className="flex items-center gap-2 mb-1">
            <DiagnosticBadge diagnostic={diagnostic} />
            <span style={{ fontSize: 10, color: 'var(--cc-text-subtle)', fontFamily: 'JetBrains Mono, monospace' }}>L{diagnostic.range ? diagnostic.range.start.line + 1 : 0}</span>
          </div>
          <div style={{ fontSize: 11, color: 'var(--cc-text-muted)', lineHeight: 1.35 }}>{diagnostic.message}</div>
        </div>
      ))}
    </div>
  )
}

function DiagnosticBadge({ diagnostic }: { diagnostic: DiagnosticRecord }) {
  const color = diagnostic.severity === 'Error' ? '#F87171' : diagnostic.severity === 'Warning' ? '#F59E0B' : '#7D8795'
  return <Badge color={color}>{diagnostic.severity}</Badge>
}

function ConfidenceBadge({ confidence }: { confidence: EdgeConfidence }) {
  const colors: Record<EdgeConfidence, string> = {
    Exact: '#34D399',
    Semantic: '#06B6D4',
    SyntaxFallback: '#F59E0B',
    Heuristic: '#7D8795',
  }
  return (
    <span style={{ fontSize: 9, color: colors[confidence], border: `1px solid ${colors[confidence]}30`, background: `${colors[confidence]}18`, borderRadius: 4, padding: '1px 4px' }}>
      {confidence}
    </span>
  )
}

function confidenceByNode(edges: GraphEdge[], side: 'source' | 'target') {
  const map = new Map<string, EdgeConfidence>()
  edges.forEach(edge => {
    if (edge.confidence) map.set(edge[side], edge.confidence)
  })
  return map
}

function referenceRecordsFromNodes(nodes: GraphNode[]): ReferenceRecord[] {
  return nodes
    .filter(node => node.file)
    .map(node => ({
      node,
      location: {
        file: node.file!,
        line: node.line ?? 0,
        character: node.selectionRange?.start.character ?? 0,
        range: node.range,
      },
    }))
}

function TypeDot({ type }: { type: string }) {
  const color = NODE_TYPE_COLORS[type] ?? '#7D8795'
  return <div className="w-2 h-2 rounded-full shrink-0" style={{ background: color }} />
}

function TypeIcon({ type, color }: { type: string; color: string }) {
  const icons: Record<string, ReactNode> = {
    Function: <Zap size={16} color={color} />,
    Method: <Zap size={16} color={color} />,
    Component: <Layers size={16} color={color} />,
    Hook: <GitBranch size={16} color={color} />,
    Interface: <Layers size={16} color={color} />,
    TypeAlias: <Layers size={16} color={color} />,
    Endpoint: <ExternalLink size={16} color={color} />,
    Trait: <GitBranch size={16} color={color} />,
    Struct: <Layers size={16} color={color} />,
    default: <ChevronRight size={16} color={color} />,
  }
  return <>{icons[type] ?? icons.default}</>
}

function Badge({ color, children }: { color: string; children: ReactNode }) {
  return (
    <span style={{ fontSize: 10, padding: '2px 6px', borderRadius: 4, background: `${color}18`, color, border: `1px solid ${color}30` }}>
      {children}
    </span>
  )
}

function VisBadge({ vis }: { vis: string }) {
  const cfg: Record<string, { color: string; label: string }> = {
    'pub': { color: '#34D399', label: 'pub' },
    'pub(crate)': { color: '#F59E0B', label: 'pub(crate)' },
    'private': { color: '#7D8795', label: 'private' },
  }
  const c = cfg[vis] ?? cfg['private']
  return <Badge color={c.color}>{c.label}</Badge>
}

function ActionBtn({ icon, label, onClick, primary, active, disabled }: { icon: ReactNode; label: string; onClick: () => void; primary?: boolean; active?: boolean; disabled?: boolean }) {
  const highlighted = !disabled && (primary || active)
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className="flex items-center gap-2 rounded-lg px-3 py-2 transition-all"
      style={{
        background: highlighted ? 'rgba(6,182,212,0.12)' : 'var(--cc-card)',
        border: highlighted ? '1px solid rgba(6,182,212,0.3)' : '1px solid var(--cc-border)',
        color: disabled ? 'var(--cc-text-faint)' : highlighted ? '#06B6D4' : 'var(--cc-text-muted)',
        cursor: disabled ? 'not-allowed' : 'pointer',
        fontSize: 11,
        fontWeight: 500,
        opacity: disabled ? 0.55 : 1,
        width: '100%',
        justifyContent: 'center',
      }}
    >
      {icon}
      {label}
    </button>
  )
}
