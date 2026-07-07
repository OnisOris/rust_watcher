import { AlertCircle, ArrowRight, CheckCircle, Code2, Filter } from 'lucide-react'
import { useMemo, useState } from 'react'
import { buildApiDataFlowRows } from '../../api/apiDataFlow'
import type { GraphEdge, GraphNode } from '../../types'
import type { ApiDataFlowRow } from './architectureTypes'

interface ApiDataFlowViewProps {
  nodes: GraphNode[]
  edges: GraphEdge[]
  onSelectNode: (id: string | null) => void
  onOpenNode: (node: GraphNode) => void
}

const STATUS: Record<ApiDataFlowRow['status'], { label: string; color: string; icon: typeof AlertCircle }> = {
  ok: { label: 'OK', color: '#22C55E', icon: CheckCircle },
  'no-handler': { label: 'No handler', color: '#F59E0B', icon: AlertCircle },
  'no-caller': { label: 'No caller', color: '#A78BFA', icon: AlertCircle },
  unused: { label: 'Unused', color: '#94A3B8', icon: AlertCircle },
  unresolved: { label: 'Unresolved', color: '#F97316', icon: AlertCircle },
}

export function ApiDataFlowView({ nodes, edges, onSelectNode, onOpenNode }: ApiDataFlowViewProps) {
  const rows = useMemo(() => buildApiDataFlowRows(nodes, edges), [nodes, edges])
  const [filter, setFilter] = useState<'all' | 'unresolved' | 'unused' | 'writes'>('all')
  const [selectedId, setSelectedId] = useState<string | null>(rows[0]?.id ?? null)
  const nodeById = new Map(nodes.map(node => [node.id, node]))
  const filtered = rows.filter(row => {
    if (filter === 'unresolved') return row.status === 'no-handler' || row.status === 'no-caller' || row.status === 'unresolved'
    if (filter === 'unused') return row.status === 'unused'
    if (filter === 'writes') return ['POST', 'PUT', 'PATCH', 'DELETE'].includes(row.method ?? '')
    return true
  })
  const selected = rows.find(row => row.id === selectedId) ?? filtered[0]

  return (
    <div className="w-full h-full flex flex-col" style={{ background: 'var(--cc-bg)' }}>
      <div className="h-10 flex items-center gap-2 px-4 shrink-0" style={{ borderBottom: '1px solid var(--cc-border)', background: 'var(--cc-panel)' }}>
        <Code2 size={15} style={{ color: 'var(--cc-text-subtle)' }} />
        <span style={{ fontSize: 12, color: 'var(--cc-text)', fontWeight: 700 }}>API / Data Flow</span>
        <div className="h-4 w-px mx-1" style={{ background: 'var(--cc-border)' }} />
        {[
          ['all', 'All'],
          ['unresolved', 'Unresolved'],
          ['unused', 'Unused endpoints'],
          ['writes', 'POST / PUT / DEL'],
        ].map(([id, label]) => (
          <button key={id} className={`arch-segment ${filter === id ? 'arch-segment-active' : ''}`} onClick={() => setFilter(id as typeof filter)}>{label}</button>
        ))}
        <Filter size={14} className="ml-auto" style={{ color: 'var(--cc-text-subtle)' }} />
      </div>
      <div className="h-9 flex items-center gap-5 px-4 shrink-0" style={{ borderBottom: '1px solid var(--cc-border)', background: 'var(--cc-bg)' }}>
        <Stat label="Total endpoints" value={rows.length} />
        <Stat label="Resolved" value={rows.filter(row => row.status === 'ok').length} color="#22C55E" />
        <Stat label="Unresolved" value={rows.filter(row => row.status === 'no-handler' || row.status === 'no-caller').length} color="#F59E0B" />
        <Stat label="Unused" value={rows.filter(row => row.status === 'unused').length} color="#94A3B8" />
      </div>
      <div className="flex-1 flex min-h-0">
        <div className="flex-1 overflow-auto">
          <table className="w-full text-xs border-collapse">
            <thead className="sticky top-0 z-10" style={{ background: 'var(--cc-panel)' }}>
              <tr style={{ borderBottom: '1px solid var(--cc-border)' }}>
                {['Status', 'Frontend caller', 'Method', 'Endpoint', 'Backend handler', 'Data type'].map(header => (
                  <th key={header} className="px-3 py-2 text-left" style={{ color: 'var(--cc-text-subtle)', fontSize: 10, textTransform: 'uppercase' }}>{header}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {filtered.map(row => {
                const status = STATUS[row.status]
                const Icon = status.icon
                const selectedRow = selected?.id === row.id
                return (
                  <tr
                    key={row.id}
                    onClick={() => {
                      setSelectedId(row.id)
                      onSelectNode(row.endpointNodeId ?? row.frontendCallerNodeId ?? null)
                    }}
                    style={{
                      borderBottom: '1px solid var(--cc-border)',
                      background: selectedRow ? 'var(--cc-card)' : 'transparent',
                      cursor: 'pointer',
                    }}
                  >
                    <td className="px-3 py-2"><span className="flex items-center gap-1" style={{ color: status.color }}><Icon size={13} />{status.label}</span></td>
                    <td className="px-3 py-2" style={{ color: 'var(--cc-text)', fontFamily: 'monospace' }}>{row.frontendCallerLabel ?? '-'}</td>
                    <td className="px-3 py-2"><span className="arch-badge">{row.method ?? 'ANY'}</span></td>
                    <td className="px-3 py-2" style={{ color: 'var(--cc-text)', fontFamily: 'monospace' }}>{row.endpointPath}</td>
                    <td className="px-3 py-2" style={{ color: 'var(--cc-text-muted)', fontFamily: 'monospace' }}>{row.backendHandlerLabel ?? '-'}</td>
                    <td className="px-3 py-2" style={{ color: 'var(--cc-text-muted)', fontFamily: 'monospace' }}>{row.dataTypeLabel ?? '-'}</td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        </div>
        <aside className="w-80 shrink-0 p-4 overflow-auto" style={{ borderLeft: '1px solid var(--cc-border)', background: 'var(--cc-panel)' }}>
          {selected ? (
            <MiniFlow
              row={selected}
              nodeById={nodeById}
              onOpenNode={onOpenNode}
            />
          ) : (
            <div className="arch-empty">No API endpoints found in the current graph.</div>
          )}
        </aside>
      </div>
    </div>
  )
}

function Stat({ label, value, color = 'var(--cc-text)' }: { label: string; value: number; color?: string }) {
  return (
    <div className="flex items-baseline gap-1.5">
      <span style={{ color, fontSize: 14, fontWeight: 800 }}>{value}</span>
      <span style={{ color: 'var(--cc-text-subtle)', fontSize: 11 }}>{label}</span>
    </div>
  )
}

function MiniFlow({ row, nodeById, onOpenNode }: { row: ApiDataFlowRow; nodeById: Map<string, GraphNode>; onOpenNode: (node: GraphNode) => void }) {
  const steps = [
    { label: row.frontendCallerLabel ?? '(none)', sub: 'frontend caller', nodeId: row.frontendCallerNodeId, color: '#3B82F6' },
    { label: row.method ?? 'ANY', sub: row.endpointPath, nodeId: row.endpointNodeId, color: '#E11D48' },
    { label: row.backendHandlerLabel ?? '(unresolved)', sub: 'backend handler', nodeId: row.backendHandlerNodeId, color: '#F97316' },
    { label: row.dataTypeLabel ?? '(unknown)', sub: 'data type', nodeId: row.dataTypeNodeId, color: '#14B8A6' },
  ]
  return (
    <div className="space-y-3">
      <div className="arch-eyebrow">Mini Flow</div>
      {steps.map((step, index) => {
        const node = step.nodeId ? nodeById.get(step.nodeId) : undefined
        return (
          <div key={`${step.label}:${index}`}>
            <button
              className="w-full text-left p-3"
              disabled={!node}
              onClick={() => node && onOpenNode(node)}
              style={{
                borderRadius: 8,
                border: `1px solid ${step.color}66`,
                background: `${step.color}18`,
                cursor: node ? 'pointer' : 'default',
              }}
            >
              <div style={{ color: step.color, fontSize: 12, fontWeight: 760, fontFamily: 'monospace' }}>{step.label}</div>
              <div style={{ color: 'var(--cc-text-subtle)', fontSize: 10, marginTop: 3 }}>{step.sub}</div>
            </button>
            {index < steps.length - 1 && <ArrowRight size={14} style={{ color: 'var(--cc-text-faint)', margin: '7px auto', display: 'block' }} />}
          </div>
        )
      })}
      {row.status !== 'ok' && <div style={{ color: STATUS[row.status].color, fontSize: 12 }}>{STATUS[row.status].label}: this route is not fully resolved.</div>}
    </div>
  )
}
