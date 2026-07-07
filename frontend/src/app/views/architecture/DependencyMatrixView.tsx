import { ArrowUpRight, Table2 } from 'lucide-react'
import { useMemo, useState } from 'react'
import type { DependencyMatrixCell, DependencyMatrixModel } from './architectureTypes'
import { DiagramCanvas } from './DiagramCanvas'

interface DependencyMatrixViewProps {
  model: DependencyMatrixModel
}

export function DependencyMatrixView({ model }: DependencyMatrixViewProps) {
  const groups = model.groups.slice(0, 10)
  const groupIds = new Set(groups.map(group => group.id))
  const cells = model.cells.filter(cell => groupIds.has(cell.sourceGroupId) && groupIds.has(cell.targetGroupId))
  const [selectedKey, setSelectedKey] = useState<string | null>(cells[0] ? cellKey(cells[0]) : null)
  const cellByKey = useMemo(() => new Map(cells.map(cell => [cellKey(cell), cell])), [cells])
  const max = Math.max(1, ...cells.map(cell => cell.count))
  const selected = selectedKey ? cellByKey.get(selectedKey) : undefined
  const labelById = new Map(groups.map(group => [group.id, group.label]))

  if (groups.length < 2 || cells.length === 0) {
    return (
      <div className="w-full h-full flex flex-col" style={{ background: 'var(--cc-bg)' }}>
        <div className="h-10 flex items-center gap-3 px-4 shrink-0" style={{ borderBottom: '1px solid var(--cc-border)', background: 'var(--cc-panel)' }}>
          <Table2 size={15} style={{ color: 'var(--cc-text-subtle)' }} />
          <span style={{ fontSize: 12, color: 'var(--cc-text)', fontWeight: 700 }}>Dependency Matrix</span>
          <span style={{ fontSize: 11, color: 'var(--cc-text-subtle)' }}>{groups.length} groups · 0 cross-area cells</span>
        </div>
        <div className="flex-1 flex items-center justify-center p-6">
          <div className="arch-empty" style={{ maxWidth: 520 }}>
            Matrix needs at least two visible project areas with cross-area dependencies. This filtered graph currently has {groups.length || 'no'} visible area{groups.length === 1 ? '' : 's'}.
          </div>
        </div>
      </div>
    )
  }

  return (
    <div className="w-full h-full flex flex-col" style={{ background: 'var(--cc-bg)' }}>
      <div className="h-10 flex items-center gap-3 px-4 shrink-0" style={{ borderBottom: '1px solid var(--cc-border)', background: 'var(--cc-panel)' }}>
        <Table2 size={15} style={{ color: 'var(--cc-text-subtle)' }} />
        <span style={{ fontSize: 12, color: 'var(--cc-text)', fontWeight: 700 }}>Dependency Matrix</span>
        <span style={{ fontSize: 11, color: 'var(--cc-text-subtle)' }}>{groups.length} groups · {cells.length} non-empty cells</span>
      </div>
      <div className="flex-1 flex min-h-0">
        <div className="flex-1 min-w-0">
          <DiagramCanvas width={Math.max(720, 150 + groups.length * 64)} height={Math.max(520, 150 + groups.length * 42)}>
          <table className="border-collapse" style={{ fontFamily: 'monospace' }}>
            <thead>
              <tr>
                <th style={{ width: 132 }} />
                {groups.map(group => (
                  <th key={group.id} style={{ minWidth: 58, padding: 4, verticalAlign: 'bottom' }}>
                    <div style={{ writingMode: 'vertical-rl', transform: 'rotate(180deg)', height: 92, color: 'var(--cc-text-subtle)', fontSize: 10, textAlign: 'left' }}>{group.label}</div>
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {groups.map(row => (
                <tr key={row.id}>
                  <td style={{ paddingRight: 10, textAlign: 'right', color: 'var(--cc-text-muted)', fontSize: 11 }}>{row.label}</td>
                  {groups.map(column => {
                    const cell = cellByKey.get(`${row.id}->${column.id}`)
                    const selected = selectedKey === `${row.id}->${column.id}`
                    const intensity = cell ? Math.max(0.12, cell.count / max) : 0
                    return (
                      <td key={column.id} style={{ padding: 3 }}>
                        <button
                          onClick={() => cell && setSelectedKey(cellKey(cell))}
                          disabled={!cell || row.id === column.id}
                          title={cell ? `${row.label} -> ${column.label}: ${cell.count}` : undefined}
                          style={{
                            width: 44,
                            height: 32,
                            borderRadius: 6,
                            border: selected ? '2px solid #38BDF8' : '1px solid var(--cc-border)',
                            background: row.id === column.id ? 'var(--cc-card)' : cell ? `rgba(59,130,246,${0.18 + intensity * 0.62})` : 'transparent',
                            color: cell ? '#EFF6FF' : 'var(--cc-text-faint)',
                            fontSize: 11,
                            cursor: cell ? 'pointer' : 'default',
                          }}
                        >
                          {row.id === column.id ? '·' : cell?.count ?? ''}
                        </button>
                      </td>
                    )
                  })}
                </tr>
              ))}
            </tbody>
          </table>
          </DiagramCanvas>
        </div>
        <div className="w-72 shrink-0 overflow-auto p-4" style={{ borderLeft: '1px solid var(--cc-border)', background: 'var(--cc-panel)' }}>
          {selected ? (
            <div className="space-y-4">
              <div>
                <div className="arch-eyebrow">Cell Detail</div>
                <div className="flex items-center gap-2 mt-2" style={{ color: 'var(--cc-text)', fontSize: 13, fontWeight: 750 }}>
                  {labelById.get(selected.sourceGroupId)}
                  <ArrowUpRight size={14} style={{ color: 'var(--cc-text-subtle)' }} />
                  {labelById.get(selected.targetGroupId)}
                </div>
              </div>
              <div className="arch-stat-card">
                <div className="arch-stat-value">{selected.count}</div>
                <div className="arch-stat-label">dependencies</div>
              </div>
              <div>
                <div className="arch-eyebrow">Relationship Types</div>
                <div className="space-y-2 mt-2">
                  {Object.entries(selected.edgeTypes).map(([type, count]) => (
                    <div key={type} className="flex justify-between" style={{ fontSize: 12, color: 'var(--cc-text-muted)' }}>
                      <span>{type}</span>
                      <span style={{ color: 'var(--cc-text)' }}>{count}</span>
                    </div>
                  ))}
                </div>
              </div>
              <div style={{ fontSize: 11, color: 'var(--cc-text-faint)' }}>{selected.underlyingEdgeIds.length} underlying edge ids preserved</div>
            </div>
          ) : (
            <div style={{ fontSize: 12, color: 'var(--cc-text-subtle)' }}>Select a non-empty cell to inspect edge type breakdown.</div>
          )}
        </div>
      </div>
    </div>
  )
}

function cellKey(cell: DependencyMatrixCell) {
  return `${cell.sourceGroupId}->${cell.targetGroupId}`
}
