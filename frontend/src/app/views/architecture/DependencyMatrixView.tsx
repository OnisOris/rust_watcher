import { ArrowUpRight, EyeOff, GitBranch, Layers3, Table2 } from 'lucide-react'
import { useEffect, useMemo, useState, type ReactNode } from 'react'
import { buildDependencyMatrixModel, defaultDependencyMatrixLevel } from '../../api/dependencyMatrix'
import type { GraphEdge, GraphFilters, GraphNode } from '../../types'
import type { DependencyMatrixBadge, DependencyMatrixCell, DependencyMatrixLevel } from './architectureTypes'
import { DiagramCanvas } from './DiagramCanvas'

interface DependencyMatrixViewProps {
  nodes: GraphNode[]
  edges: GraphEdge[]
  filters: GraphFilters
}

const LEVEL_OPTIONS: Array<{ level: DependencyMatrixLevel; label: string }> = [
  { level: 'area', label: 'Area' },
  { level: 'module', label: 'Module' },
  { level: 'directory', label: 'Directory' },
  { level: 'file', label: 'File' },
]

const BADGE_LABELS: Record<DependencyMatrixBadge, string> = {
  cycle: 'Cycle',
  strong: 'Strong',
  unexpected: 'Unexpected',
  violation: 'Violation',
  external: 'External',
  'type-only': 'Type-only',
}

export function DependencyMatrixView({ nodes, edges, filters }: DependencyMatrixViewProps) {
  const preferredLevel = useMemo(() => defaultDependencyMatrixLevel(nodes), [nodes])
  const [level, setLevel] = useState<DependencyMatrixLevel>(preferredLevel)
  const [showAllFiles, setShowAllFiles] = useState(false)

  useEffect(() => {
    setLevel(preferredLevel)
    setShowAllFiles(false)
  }, [preferredLevel])

  const model = useMemo(() => buildDependencyMatrixModel(nodes, edges, {
    level,
    includeTests: filters.showTests,
    includeExternal: filters.showExternal,
    includeGenerated: true,
    includeTypeRefs: true,
    expandAllFiles: showAllFiles,
  }), [edges, filters.showExternal, filters.showTests, level, nodes, showAllFiles])

  const groups = model.groups
  const cells = model.cells
  const [selectedKey, setSelectedKey] = useState<string | null>(cells[0] ? cellKey(cells[0]) : null)
  const cellByKey = useMemo(() => new Map(cells.map(cell => [cellKey(cell), cell])), [cells])
  const max = Math.max(1, ...cells.map(cell => cell.count))
  const selected = selectedKey ? cellByKey.get(selectedKey) : undefined
  const labelById = useMemo(() => new Map(groups.map(group => [group.id, group.label])), [groups])

  useEffect(() => {
    if (!selectedKey || !cellByKey.has(selectedKey)) {
      setSelectedKey(cells[0] ? cellKey(cells[0]) : null)
    }
  }, [cellByKey, cells, selectedKey])

  return (
    <div className="w-full h-full flex flex-col" style={{ background: 'var(--cc-bg)' }}>
      <div className="min-h-10 flex items-center gap-3 px-4 py-2 shrink-0" style={{ borderBottom: '1px solid var(--cc-border)', background: 'var(--cc-panel)' }}>
        <Table2 size={15} style={{ color: 'var(--cc-text-subtle)' }} />
        <span style={{ fontSize: 12, color: 'var(--cc-text)', fontWeight: 700 }}>Dependency Matrix</span>
        <span style={{ fontSize: 11, color: 'var(--cc-text-subtle)' }}>
          {groups.length}{model.truncated ? `/${model.totalGroups}` : ''} groups · {cells.length} non-empty cells
        </span>
        <div className="flex items-center gap-1 ml-2" data-no-pan="true">
          {LEVEL_OPTIONS.map(option => (
            <button
              key={option.level}
              className={`arch-mode-button ${level === option.level ? 'active' : ''}`}
              onClick={() => {
                setLevel(option.level)
                setShowAllFiles(false)
              }}
            >
              {option.label}
            </button>
          ))}
        </div>
        {model.truncated && level === 'file' ? (
          <button className="arch-mode-button ml-auto" onClick={() => setShowAllFiles(true)}>Show all files</button>
        ) : null}
      </div>

      {groups.length < 2 || cells.length === 0 ? (
        <MatrixEmptyState
          groupCount={groups.length}
          suggestedLevel={model.suggestedLevel}
          onSwitchToFile={() => setLevel('file')}
        />
      ) : (
        <div className="flex-1 flex min-h-0">
          <div className="flex-1 min-w-0">
            <DiagramCanvas width={Math.max(900, 180 + groups.length * 86)} height={Math.max(620, 170 + groups.length * 54)} initialZoom={groups.length > 18 ? 0.72 : 0.95}>
              <table className="border-collapse" style={{ fontFamily: 'monospace' }}>
                <thead>
                  <tr>
                    <th style={{ width: 172 }} />
                    {groups.map(group => (
                      <th key={group.id} style={{ width: 78, padding: 5, verticalAlign: 'bottom' }}>
                        <div
                          title={group.label}
                          style={{
                            writingMode: 'vertical-rl',
                            transform: 'rotate(180deg)',
                            height: 124,
                            color: 'var(--cc-text-subtle)',
                            fontSize: 10,
                            textAlign: 'left',
                            overflow: 'hidden',
                            textOverflow: 'ellipsis',
                            whiteSpace: 'nowrap',
                          }}
                        >
                          {group.label}
                        </div>
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {groups.map(row => (
                    <tr key={row.id}>
                      <td
                        title={row.label}
                        style={{
                          maxWidth: 164,
                          paddingRight: 12,
                          textAlign: 'right',
                          color: 'var(--cc-text-muted)',
                          fontSize: 11,
                          overflow: 'hidden',
                          textOverflow: 'ellipsis',
                          whiteSpace: 'nowrap',
                        }}
                      >
                        {row.label}
                      </td>
                      {groups.map(column => {
                        const key = `${row.id}->${column.id}`
                        const cell = cellByKey.get(key)
                        const isSelected = selectedKey === key
                        const intensity = cell ? Math.max(0.12, cell.count / max) : 0
                        return (
                          <td key={column.id} style={{ padding: 4 }}>
                            <button
                              data-no-pan="true"
                              onClick={() => cell && setSelectedKey(cellKey(cell))}
                              disabled={!cell || row.id === column.id}
                              title={cell ? `${row.label} -> ${column.label}: ${cell.count}${cell.badges.length ? ` · ${cell.badges.map(badge => BADGE_LABELS[badge]).join(', ')}` : ''}` : undefined}
                              style={{
                                width: 62,
                                height: 42,
                                borderRadius: 7,
                                border: isSelected ? '2px solid #38BDF8' : cell?.badges.includes('violation') ? '1px solid #FB7185' : '1px solid var(--cc-border)',
                                background: row.id === column.id ? 'var(--cc-card)' : cell ? cellBackground(cell, intensity) : 'transparent',
                                color: cell ? '#F8FAFC' : 'var(--cc-text-faint)',
                                fontSize: 11,
                                cursor: cell ? 'pointer' : 'default',
                                boxShadow: isSelected ? '0 0 0 2px rgba(56,189,248,0.18)' : 'none',
                              }}
                            >
                              {row.id === column.id ? (
                                <span>·</span>
                              ) : cell ? (
                                <span className="flex flex-col items-center leading-none gap-1">
                                  <strong>{cell.count}</strong>
                                  {cell.badges[0] ? <span style={{ fontSize: 8, opacity: 0.88 }}>{BADGE_LABELS[cell.badges[0]]}</span> : null}
                                </span>
                              ) : ''}
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

          <MatrixDetailPanel selected={selected} labelById={labelById} />
        </div>
      )}
    </div>
  )
}

function MatrixEmptyState({
  groupCount,
  suggestedLevel,
  onSwitchToFile,
}: {
  groupCount: number
  suggestedLevel?: DependencyMatrixLevel
  onSwitchToFile: () => void
}) {
  const coarse = suggestedLevel === 'file'
  return (
    <div className="flex-1 flex items-center justify-center p-6">
      <div className="arch-empty" style={{ maxWidth: 560, textAlign: 'left' }}>
        <div style={{ fontWeight: 750, color: 'var(--cc-text)', marginBottom: 8 }}>
          {coarse ? 'Matrix is too coarse at Area level.' : 'No dependency cells found.'}
        </div>
        <div>
          {coarse
            ? 'Try Module or File level to inspect dependencies inside this project.'
            : `Matrix needs at least two visible groups with dependencies. This filtered graph currently has ${groupCount || 'no'} visible group${groupCount === 1 ? '' : 's'}.`}
        </div>
        {coarse ? (
          <button className="arch-mode-button active mt-4" onClick={onSwitchToFile}>Switch to File level</button>
        ) : null}
      </div>
    </div>
  )
}

function MatrixDetailPanel({
  selected,
  labelById,
}: {
  selected?: DependencyMatrixCell
  labelById: Map<string, string>
}) {
  return (
    <div className="w-80 shrink-0 overflow-auto p-4" style={{ borderLeft: '1px solid var(--cc-border)', background: 'var(--cc-panel)' }}>
      {selected ? (
        <div className="space-y-4">
          <div>
            <div className="arch-eyebrow">Cell Detail</div>
            <div className="flex items-center gap-2 mt-2" style={{ color: 'var(--cc-text)', fontSize: 13, fontWeight: 750 }}>
              <span className="truncate">{labelById.get(selected.sourceGroupId)}</span>
              <ArrowUpRight size={14} style={{ color: 'var(--cc-text-subtle)', flexShrink: 0 }} />
              <span className="truncate">{labelById.get(selected.targetGroupId)}</span>
            </div>
          </div>
          <div className="arch-stat-card">
            <div className="arch-stat-value">{selected.count}</div>
            <div className="arch-stat-label">dependencies</div>
          </div>
          {selected.badges.length ? (
            <div className="flex flex-wrap gap-1">
              {selected.badges.map(badge => <MatrixBadge key={badge} badge={badge} />)}
            </div>
          ) : null}
          <DetailSection title="Relationship Types">
            {Object.entries(selected.edgeTypes).map(([type, count]) => (
              <div key={type} className="flex justify-between gap-3" style={{ fontSize: 12, color: 'var(--cc-text-muted)' }}>
                <span>{type}</span>
                <span style={{ color: 'var(--cc-text)' }}>{count}</span>
              </div>
            ))}
          </DetailSection>
          <DetailSection title="Files Involved">
            {selected.files.slice(0, 10).map(file => (
              <div key={file} title={file} style={{ fontSize: 11, color: 'var(--cc-text-muted)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                {file}
              </div>
            ))}
            {selected.files.length > 10 ? <div style={{ fontSize: 11, color: 'var(--cc-text-faint)' }}>+{selected.files.length - 10} more files</div> : null}
          </DetailSection>
          <DetailSection title="Important Edges">
            {selected.examples.map(example => (
              <div key={example.id} className="p-2 rounded-md" style={{ border: '1px solid var(--cc-border)', background: 'var(--cc-card)' }}>
                <div style={{ fontSize: 11, color: 'var(--cc-text)', fontWeight: 700 }}>
                  {example.sourceLabel} → {example.targetLabel}
                </div>
                <div style={{ fontSize: 10, color: 'var(--cc-text-subtle)', marginTop: 3 }}>
                  {example.type}
                  {example.sourceFile || example.targetFile ? ` · ${example.sourceFile || 'unknown'} → ${example.targetFile || 'unknown'}` : ''}
                </div>
              </div>
            ))}
          </DetailSection>
          <DetailSection title="Actions">
            <div className="grid grid-cols-2 gap-2">
              <ActionButton icon={<GitBranch size={13} />} label="Show graph" />
              <ActionButton icon={<Layers3 size={13} />} label="Neighborhood" />
              <ActionButton icon={<ArrowUpRight size={13} />} label="Call flow" />
              <ActionButton icon={<ArrowUpRight size={13} />} label="Open file" />
              <ActionButton icon={<EyeOff size={13} />} label="Hide type refs" wide />
            </div>
          </DetailSection>
          <div style={{ fontSize: 11, color: 'var(--cc-text-faint)' }}>{selected.underlyingEdgeIds.length} underlying edge ids preserved</div>
        </div>
      ) : (
        <div style={{ fontSize: 12, color: 'var(--cc-text-subtle)' }}>Select a non-empty cell to inspect files, examples, and relationship types.</div>
      )}
    </div>
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

function MatrixBadge({ badge }: { badge: DependencyMatrixBadge }) {
  const color = badge === 'violation' ? '#FB7185' : badge === 'unexpected' ? '#F59E0B' : badge === 'cycle' ? '#8B5CF6' : '#38BDF8'
  return (
    <span
      style={{
        border: `1px solid ${color}`,
        color,
        borderRadius: 999,
        padding: '2px 7px',
        fontSize: 10,
        fontWeight: 750,
        background: `${color}14`,
      }}
    >
      {BADGE_LABELS[badge]}
    </span>
  )
}

function ActionButton({ icon, label, wide }: { icon: ReactNode; label: string; wide?: boolean }) {
  return (
    <button
      className={wide ? 'col-span-2' : undefined}
      disabled
      style={{
        minHeight: 30,
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
      {icon}
      {label}
    </button>
  )
}

function cellBackground(cell: DependencyMatrixCell, intensity: number) {
  if (cell.badges.includes('violation')) return `rgba(244,63,94,${0.2 + intensity * 0.58})`
  if (cell.badges.includes('unexpected')) return `rgba(245,158,11,${0.18 + intensity * 0.5})`
  if (cell.badges.includes('cycle')) return `rgba(139,92,246,${0.2 + intensity * 0.52})`
  if (cell.badges.includes('external')) return `rgba(100,116,139,${0.2 + intensity * 0.5})`
  return `rgba(59,130,246,${0.18 + intensity * 0.62})`
}

function cellKey(cell: DependencyMatrixCell) {
  return `${cell.sourceGroupId}->${cell.targetGroupId}`
}
