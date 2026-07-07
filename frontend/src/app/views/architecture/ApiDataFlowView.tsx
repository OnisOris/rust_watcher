import { AlertCircle, ArrowRight, CheckCircle, Code2, Filter } from 'lucide-react'
import { useMemo, useState } from 'react'
import { buildApiEndpointGroups } from '../../api/apiDataFlow'
import type { GraphEdge, GraphNode } from '../../types'
import type { ApiEndpointGroup, ApiEndpointParticipant } from './architectureTypes'

interface ApiDataFlowViewProps {
  nodes: GraphNode[]
  edges: GraphEdge[]
  onSelectNode: (id: string | null) => void
  onOpenNode: (node: GraphNode) => void
}

type ApiFilter = 'all' | 'resolved' | 'unresolved' | 'no-handler' | 'no-caller' | 'unused' | 'writes'

const STATUS: Record<ApiEndpointGroup['status'], { label: string; color: string; icon: typeof AlertCircle }> = {
  ok: { label: 'OK', color: '#22C55E', icon: CheckCircle },
  'no-handler': { label: 'No handler', color: '#F59E0B', icon: AlertCircle },
  'no-caller': { label: 'No caller', color: '#A78BFA', icon: AlertCircle },
  unused: { label: 'Unused', color: '#94A3B8', icon: AlertCircle },
  unresolved: { label: 'Unresolved', color: '#F97316', icon: AlertCircle },
}

export function ApiDataFlowView({ nodes, edges, onSelectNode, onOpenNode }: ApiDataFlowViewProps) {
  const groups = useMemo(() => buildApiEndpointGroups(nodes, edges), [nodes, edges])
  const [filter, setFilter] = useState<ApiFilter>('all')
  const [groupBy, setGroupBy] = useState<'endpoint' | 'caller' | 'handler'>('endpoint')
  const [selectedId, setSelectedId] = useState<string | null>(groups[0]?.id ?? null)
  const nodeById = useMemo(() => new Map(nodes.map(node => [node.id, node])), [nodes])
  const filtered = groups.filter(group => {
    if (filter === 'resolved') return group.status === 'ok'
    if (filter === 'unresolved') return group.status === 'unresolved'
    if (filter === 'no-handler') return group.status === 'no-handler'
    if (filter === 'no-caller') return group.status === 'no-caller'
    if (filter === 'unused') return group.status === 'unused'
    if (filter === 'writes') return ['POST', 'PUT', 'PATCH', 'DELETE'].includes(group.method ?? '')
    return true
  })
  const selected = groups.find(group => group.id === selectedId) ?? filtered[0]

  return (
    <div className="w-full h-full flex flex-col" style={{ background: 'var(--cc-bg)' }}>
      <div className="h-10 flex items-center gap-2 px-4 shrink-0" style={{ borderBottom: '1px solid var(--cc-border)', background: 'var(--cc-panel)' }}>
        <Code2 size={15} style={{ color: 'var(--cc-text-subtle)' }} />
        <span style={{ fontSize: 12, color: 'var(--cc-text)', fontWeight: 700 }}>API / Data Flow</span>
        <div className="h-4 w-px mx-1" style={{ background: 'var(--cc-border)' }} />
        {[
          ['all', 'All'],
          ['resolved', 'Resolved'],
          ['unresolved', 'Unresolved'],
          ['no-handler', 'No handler'],
          ['no-caller', 'No caller'],
          ['unused', 'Unused endpoints'],
          ['writes', 'POST / PUT / DEL'],
        ].map(([id, label]) => (
          <button key={id} className={`arch-segment ${filter === id ? 'arch-segment-active' : ''}`} onClick={() => setFilter(id as ApiFilter)}>{label}</button>
        ))}
        <div className="ml-auto flex items-center gap-2">
          <select
            value={groupBy}
            onChange={event => setGroupBy(event.target.value as typeof groupBy)}
            style={{ height: 28, borderRadius: 7, border: '1px solid var(--cc-border)', background: 'var(--cc-surface)', color: 'var(--cc-text-subtle)', fontSize: 11, padding: '0 8px' }}
          >
            <option value="endpoint">Group by endpoint</option>
            <option value="caller" disabled>Group by caller (TODO)</option>
            <option value="handler" disabled>Group by handler (TODO)</option>
          </select>
          <Filter size={14} style={{ color: 'var(--cc-text-subtle)' }} />
        </div>
      </div>
      <div className="h-9 flex items-center gap-5 px-4 shrink-0" style={{ borderBottom: '1px solid var(--cc-border)', background: 'var(--cc-bg)' }}>
        <Stat label="Endpoint groups" value={groups.length} />
        <Stat label="Resolved" value={groups.filter(group => group.status === 'ok').length} color="#22C55E" />
        <Stat label="Unresolved" value={groups.filter(group => group.status === 'unresolved').length} color="#F97316" />
        <Stat label="No handler" value={groups.filter(group => group.status === 'no-handler').length} color="#F59E0B" />
        <Stat label="No caller" value={groups.filter(group => group.status === 'no-caller').length} color="#A78BFA" />
        <Stat label="Unused" value={groups.filter(group => group.status === 'unused').length} color="#94A3B8" />
      </div>
      <div className="flex-1 flex min-h-0">
        <div className="flex-1 overflow-auto">
          <table className="w-full text-xs border-collapse">
            <thead className="sticky top-0 z-10" style={{ background: 'var(--cc-panel)' }}>
              <tr style={{ borderBottom: '1px solid var(--cc-border)' }}>
                {['Status', 'Method', 'Endpoint', 'Callers', 'Handler', 'Data types'].map(header => (
                  <th key={header} className="px-3 py-2 text-left" style={{ color: 'var(--cc-text-subtle)', fontSize: 10, textTransform: 'uppercase' }}>{header}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {filtered.map(group => {
                const status = STATUS[group.status]
                const Icon = status.icon
                const selectedRow = selected?.id === group.id
                return (
                  <tr
                    key={group.id}
                    onClick={() => {
                      setSelectedId(group.id)
                      onSelectNode(group.endpointNodeIds[0] ?? group.callers[0]?.nodeId ?? null)
                    }}
                    style={{
                      borderBottom: '1px solid var(--cc-border)',
                      background: selectedRow ? 'var(--cc-card)' : 'transparent',
                      cursor: 'pointer',
                    }}
                  >
                    <td className="px-3 py-2"><span className="flex items-center gap-1" style={{ color: status.color }}><Icon size={13} />{status.label}</span></td>
                    <td className="px-3 py-2"><span className="arch-badge">{group.method ?? 'ANY'}</span></td>
                    <td className="px-3 py-2" style={{ color: 'var(--cc-text)', fontFamily: 'monospace' }}>{group.path}</td>
                    <td className="px-3 py-2" style={{ color: 'var(--cc-text-muted)' }}>{group.callers.length ? `${group.callers.length} caller${group.callers.length === 1 ? '' : 's'}` : '-'}</td>
                    <td className="px-3 py-2" style={{ color: 'var(--cc-text-muted)', fontFamily: 'monospace' }}>{summarizeParticipants(group.handlers)}</td>
                    <td className="px-3 py-2" style={{ color: 'var(--cc-text-muted)', fontFamily: 'monospace' }}>{summarizeParticipants(group.dataTypes)}</td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        </div>
        <aside className="w-[360px] shrink-0 overflow-auto" style={{ borderLeft: '1px solid var(--cc-border)', background: 'var(--cc-panel)' }}>
          {selected ? (
            <EndpointDetail
              group={selected}
              nodeById={nodeById}
              onOpenNode={onOpenNode}
            />
          ) : (
            <div className="p-4"><div className="arch-empty">No API endpoints found in the current graph.</div></div>
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

function EndpointDetail({ group, nodeById, onOpenNode }: { group: ApiEndpointGroup; nodeById: Map<string, GraphNode>; onOpenNode: (node: GraphNode) => void }) {
  const status = STATUS[group.status]
  return (
    <div className="p-4 space-y-4">
      <div>
        <div className="arch-eyebrow">{group.method ?? 'ANY'}</div>
        <h3 style={{ color: 'var(--cc-text)', fontSize: 18, fontWeight: 820, marginTop: 6, overflowWrap: 'anywhere' }}>{group.path}</h3>
        <div className="flex items-center gap-2 mt-2">
          <span className="arch-badge" style={{ borderColor: `${status.color}66`, color: status.color }}>{status.label}</span>
          <span className="arch-badge">{group.routeKey}</span>
        </div>
      </div>
      <MiniFlow group={group} nodeById={nodeById} onOpenNode={onOpenNode} />
      <ParticipantSection title="Frontend callers" participants={group.callers} nodeById={nodeById} onOpenNode={onOpenNode} />
      <ParticipantSection title="Backend handlers" participants={group.handlers} nodeById={nodeById} onOpenNode={onOpenNode} />
      <ParticipantSection title="Data types" participants={group.dataTypes} nodeById={nodeById} onOpenNode={onOpenNode} />
      <section className="arch-panel" style={{ boxShadow: 'none' }}>
        <div className="arch-eyebrow">Relationship types</div>
        <div className="space-y-2 mt-2">
          {Object.entries(group.edgeTypeCounts).map(([type, count]) => (
            <div key={type} className="flex justify-between" style={{ fontSize: 12, color: 'var(--cc-text-muted)' }}>
              <span>{type}</span>
              <span style={{ color: 'var(--cc-text)' }}>{count}</span>
            </div>
          ))}
          {!Object.keys(group.edgeTypeCounts).length && <div style={{ fontSize: 12, color: 'var(--cc-text-subtle)' }}>No relationship edges captured.</div>}
        </div>
      </section>
    </div>
  )
}

function MiniFlow({ group, nodeById, onOpenNode }: { group: ApiEndpointGroup; nodeById: Map<string, GraphNode>; onOpenNode: (node: GraphNode) => void }) {
  const handlerLabels = uniqueLabels(group.handlers)
  const dataTypeLabels = uniqueLabels(group.dataTypes)
  const callerLabels = uniqueLabels(group.callers)
  const callerLabel = callerLabels.length > 1 ? `${callerLabels.length} callers` : callerLabels[0] ?? '(none)'
  const handlerLabel = handlerLabels.length > 1 ? `${handlerLabels.length} handlers` : handlerLabels[0] ?? '(unresolved)'
  const dataTypeLabel = dataTypeLabels.length > 1 ? `${dataTypeLabels.length} data types` : dataTypeLabels[0] ?? '(unknown)'
  const steps = [
    { label: callerLabel, sub: callerLabels.slice(0, 4).join(', ') || 'frontend callers', participant: group.callers[0], color: '#3B82F6' },
    { label: group.method ?? 'ANY', sub: group.path, participant: group.endpointNodeIds[0] ? { nodeId: group.endpointNodeIds[0], label: group.path } : undefined, color: '#E11D48' },
    { label: handlerLabel, sub: handlerLabels.slice(0, 4).join(', ') || 'backend handler', participant: group.handlers[0], color: '#F97316' },
    { label: dataTypeLabel, sub: dataTypeLabels.slice(0, 4).join(', ') || 'data types', participant: group.dataTypes[0], color: '#14B8A6' },
  ]
  return (
    <section className="space-y-3">
      <div className="arch-eyebrow">Mini Flow</div>
      {steps.map((step, index) => {
        const node = step.participant?.nodeId ? nodeById.get(step.participant.nodeId) : undefined
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
              <div style={{ color: step.color, fontSize: 12, fontWeight: 760, fontFamily: 'monospace', overflowWrap: 'anywhere' }}>{step.label}</div>
              <div style={{ color: 'var(--cc-text-subtle)', fontSize: 10, marginTop: 3, overflowWrap: 'anywhere' }}>{step.sub}</div>
            </button>
            {index < steps.length - 1 && <ArrowRight size={14} style={{ color: 'var(--cc-text-faint)', margin: '7px auto', display: 'block' }} />}
          </div>
        )
      })}
    </section>
  )
}

function ParticipantSection({ title, participants, nodeById, onOpenNode }: { title: string; participants: ApiEndpointParticipant[]; nodeById: Map<string, GraphNode>; onOpenNode: (node: GraphNode) => void }) {
  return (
    <section className="arch-panel" style={{ boxShadow: 'none' }}>
      <div className="arch-eyebrow">{title}</div>
      <div className="space-y-2 mt-3">
        {participants.map(participant => {
          const node = nodeById.get(participant.nodeId)
          return (
            <button key={participant.nodeId} className="arch-node-row" disabled={!node} onClick={() => node && onOpenNode(node)}>
              <span className="arch-node-type">{participant.type ?? 'Node'}</span>
              <span className="truncate" style={{ color: 'var(--cc-text)', fontFamily: 'monospace' }}>{participant.label}</span>
              <span className="truncate" style={{ color: 'var(--cc-text-faint)', fontSize: 10 }}>{participant.file ?? participant.language ?? ''}</span>
            </button>
          )
        })}
        {!participants.length && <div style={{ fontSize: 12, color: 'var(--cc-text-subtle)' }}>None detected.</div>}
      </div>
    </section>
  )
}

function summarizeParticipants(participants: ApiEndpointParticipant[]) {
  const labels = uniqueLabels(participants)
  if (!labels.length) return '-'
  if (labels.length <= 2) return labels.join(', ')
  return `${labels.slice(0, 2).join(', ')} +${labels.length - 2}`
}

function uniqueLabels(participants: ApiEndpointParticipant[]) {
  return [...new Set(participants.map(participant => participant.label))]
}
