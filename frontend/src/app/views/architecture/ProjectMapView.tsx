import { Flame, Grid2X2, Network } from 'lucide-react'
import { useMemo } from 'react'
import type { EdgeType, GraphNode } from '../../types'
import type { AggregatedProjectEdge, ProjectGroup, ProjectMapGrouping, ProjectMapModel } from './architectureTypes'
import { DiagramCanvas } from './DiagramCanvas'

interface ProjectMapViewProps {
  model: ProjectMapModel
  nodes: GraphNode[]
  selectedNodeId: string | null
  grouping: ProjectMapGrouping
  onGroupingChange: (grouping: ProjectMapGrouping) => void
  onSelectGroup: (groupId: string) => void
  onSelectNode: (nodeId: string | null) => void
  onOpenRawGraph?: () => void
  onOpenMatrix?: () => void
  onOpenHotspots?: () => void
}

const GROUP_COLORS: Record<string, { bg: string; border: string; text: string; accent: string }> = {
  frontend: { bg: 'rgba(37, 99, 235, 0.10)', border: '#3B82F6', text: '#93C5FD', accent: '#3B82F6' },
  backend: { bg: 'rgba(249, 115, 22, 0.10)', border: '#F97316', text: '#FDBA74', accent: '#F97316' },
  shared: { bg: 'rgba(20, 184, 166, 0.10)', border: '#14B8A6', text: '#5EEAD4', accent: '#14B8A6' },
  tests: { bg: 'rgba(148, 163, 184, 0.10)', border: '#94A3B8', text: '#CBD5E1', accent: '#94A3B8' },
  external: { bg: 'rgba(100, 116, 139, 0.10)', border: '#64748B', text: '#CBD5E1', accent: '#64748B' },
  generated: { bg: 'rgba(168, 85, 247, 0.10)', border: '#A855F7', text: '#D8B4FE', accent: '#A855F7' },
  unknown: { bg: 'rgba(125, 135, 149, 0.10)', border: '#7D8795', text: '#CBD5E1', accent: '#7D8795' },
}

const GROUP_SLOTS: Record<string, { row: number; column: number }> = {
  frontend: { row: 0, column: 0 },
  backend: { row: 0, column: 2 },
  external: { row: 1, column: 0 },
  shared: { row: 1, column: 1 },
  tests: { row: 1, column: 2 },
  generated: { row: 2, column: 1 },
  unknown: { row: 2, column: 2 },
}

const GROUPING_OPTIONS: Array<{ id: ProjectMapGrouping; label: string }> = [
  { id: 'architecture', label: 'Architecture' },
  { id: 'language', label: 'Language' },
  { id: 'directory', label: 'Directory' },
  { id: 'module', label: 'Module' },
  { id: 'runtime', label: 'Runtime' },
]

export function ProjectMapView({
  model,
  nodes,
  selectedNodeId,
  grouping,
  onGroupingChange,
  onSelectGroup,
  onSelectNode,
  onOpenRawGraph,
  onOpenMatrix,
  onOpenHotspots,
}: ProjectMapViewProps) {
  const byId = useMemo(() => new Map(nodes.map(node => [node.id, node])), [nodes])
  const positionedGroups = model.groups
  const positionedIds = useMemo(() => new Set(positionedGroups.map(group => group.id)), [positionedGroups])
  const layouts = useMemo(() => buildGroupLayouts(positionedGroups, byId, model.grouping), [positionedGroups, byId, model.grouping])
  const canvas = useMemo(() => ({
    width: Math.max(1320, Math.max(...layouts.map(layout => layout.x + layout.w), 0) + 96),
    height: Math.max(900, Math.max(...layouts.map(layout => layout.y + layout.h), 0) + 96),
  }), [layouts])
  const visibleEdges = useMemo(
    () => bundleParallelProjectEdges(model.edges.filter(edge => positionedIds.has(edge.sourceGroupId) && positionedIds.has(edge.targetGroupId))).slice(0, 18),
    [model.edges, positionedIds],
  )
  const routedEdges = useMemo(() => routeProjectEdges(visibleEdges, layouts), [visibleEdges, layouts])

  if (model.groups.length <= 1) {
    return (
      <SingleAreaProjectMap
        group={model.groups[0]}
        nodes={nodes}
        selectedNodeId={selectedNodeId}
        onSelectGroup={onSelectGroup}
        onSelectNode={onSelectNode}
      />
    )
  }

  return (
    <div className="w-full h-full flex flex-col" style={{ background: 'var(--cc-graph-bg)' }}>
      <div className="h-10 flex items-center gap-3 px-4 shrink-0" style={{ borderBottom: '1px solid var(--cc-border)', background: 'var(--cc-panel)' }}>
        <Network size={15} style={{ color: 'var(--cc-text-subtle)' }} />
        <span style={{ fontSize: 12, color: 'var(--cc-text)', fontWeight: 700 }}>Project Map</span>
        <span style={{ fontSize: 11, color: 'var(--cc-text-subtle)' }}>
          {nodes.length} nodes grouped into {model.groups.length} areas · {model.edges.length} cross-area links
        </span>
        <span
          className="arch-badge"
          title="Shows project-level containers/components and aggregated relationships. Use Module and Neighborhood views for code-level details."
        >
          C4-inspired architecture map
        </span>
        <div className="ml-auto flex items-center gap-1">
          {GROUPING_OPTIONS.map(option => (
            <button
              key={option.id}
              className={`arch-segment ${grouping === option.id ? 'arch-segment-active' : ''}`}
              onClick={() => onGroupingChange(option.id)}
            >
              {option.label}
            </button>
          ))}
        </div>
      </div>

      <DiagramCanvas width={canvas.width} height={canvas.height} initialZoom={0.92}>
          <svg width={canvas.width} height={canvas.height} viewBox={`0 0 ${canvas.width} ${canvas.height}`}>
            <defs>
              <marker id="arch-arrow" markerWidth="8" markerHeight="6" refX="8" refY="3" orient="auto">
                <polygon points="0 0,8 3,0 6" fill="var(--cc-text-subtle)" opacity="0.85" />
              </marker>
              <pattern id="arch-dotgrid" width="28" height="28" patternUnits="userSpaceOnUse">
                <circle cx="1" cy="1" r="0.7" fill="var(--cc-grid-dot)" />
              </pattern>
            </defs>
            <rect width={canvas.width} height={canvas.height} fill="url(#arch-dotgrid)" />
            {routedEdges.map(edge => <ProjectEdge key={edge.edge.id} routedEdge={edge} />)}
            {positionedGroups.map(group => {
              const layout = layouts.find(candidate => candidate.group.id === group.id)
              if (!layout) return null
              const color = GROUP_COLORS[group.kind] ?? GROUP_COLORS.unknown
              const isSelected = group.nodeIds.includes(selectedNodeId ?? '')
              const hiddenSymbolCount = Math.max(0, layout.symbols.length - layout.visibleSymbols.length)
              return (
                <g
                  key={group.id}
                  role="button"
                  tabIndex={0}
                  data-no-pan="true"
                  onClick={() => onSelectGroup(group.id)}
                  onDoubleClick={() => onOpenRawGraph?.()}
                  style={{ cursor: 'pointer' }}
                >
                  <rect
                    x={layout.x}
                    y={layout.y}
                    width={layout.w}
                    height={layout.h}
                    rx={8}
                    fill={color.bg}
                    stroke={isSelected ? color.accent : color.border}
                    strokeWidth={isSelected ? 2.4 : 1.2}
                  />
                  <foreignObject x={layout.x} y={layout.y} width={layout.w} height={layout.h}>
                    <div className="arch-map-card" style={{ borderColor: 'transparent' }}>
                      <div className="arch-map-card-title">{group.label}</div>
                      <div className="arch-map-card-meta">
                        {group.fileCount} files · {group.symbolCount} symbols · {group.incomingCount} in · {group.outgoingCount} out
                      </div>
                      {!!Object.keys(group.languageBreakdown ?? {}).length && (
                        <div className="arch-map-card-meta">
                          {formatLanguageBreakdown(group.languageBreakdown)}
                        </div>
                      )}
                      {!!layout.children.length && (
                        <>
                          <div className="arch-map-section-label">Top directories/files</div>
                          <div className="arch-map-chip-grid">
                            {layout.children.map(child => (
                              <div key={child.id} className="arch-map-chip">
                                <span style={{ color: color.text }}>{child.label}</span>
                                <span>{child.fileCount || child.symbolCount}</span>
                              </div>
                            ))}
                          </div>
                        </>
                      )}
                      {!!layout.topFiles.length && (
                        <>
                          <div className="arch-map-section-label">{model.autoExpanded ? 'Inside this area' : 'Top files'}</div>
                          <div className="arch-map-symbol-grid">
                            {layout.topFiles.map(file => (
                              <div key={file} className="arch-map-symbol" title={file}>
                                <span>{shortPath(file)}</span>
                              </div>
                            ))}
                          </div>
                        </>
                      )}
                      {!!layout.visibleSymbols.length && (
                        <>
                          <div className="arch-map-section-label">Key symbols</div>
                          <div className="arch-map-symbol-grid">
                            {layout.visibleSymbols.map(node => (
                              <button
                                key={node.id}
                                className="arch-map-symbol"
                                onClick={event => {
                                  event.stopPropagation()
                                  onSelectNode(node.id)
                                }}
                              >
                                <span>{node.label}</span>
                              </button>
                            ))}
                          </div>
                        </>
                      )}
                      <div className="flex gap-1 mt-2">
                        <button className="arch-action" title="Open as Raw Graph" onClick={event => { event.stopPropagation(); onOpenRawGraph?.() }}><Network size={12} /></button>
                        <button className="arch-action" title="Open Matrix for this group" onClick={event => { event.stopPropagation(); onOpenMatrix?.() }}><Grid2X2 size={12} /></button>
                        <button className="arch-action" title="Open Hotspots for this group" onClick={event => { event.stopPropagation(); onOpenHotspots?.() }}><Flame size={12} /></button>
                      </div>
                      {hiddenSymbolCount > 0 && (
                        <div className="arch-map-more">{hiddenSymbolCount} more symbols available in Module view</div>
                      )}
                    </div>
                  </foreignObject>
                </g>
              )
            })}
          </svg>
      </DiagramCanvas>
    </div>
  )
}

function SingleAreaProjectMap({
  group,
  nodes,
  selectedNodeId,
  onSelectGroup,
  onSelectNode,
}: {
  group?: ProjectGroup
  nodes: GraphNode[]
  selectedNodeId: string | null
  onSelectGroup: (groupId: string) => void
  onSelectNode: (nodeId: string | null) => void
}) {
  const byId = new Map(nodes.map(node => [node.id, node]))
  const groupNodes = group?.nodeIds.map(id => byId.get(id)).filter((node): node is GraphNode => Boolean(node)) ?? []
  const files = groupNodes.filter(node => node.type === 'File').slice(0, 8)
  const symbols = groupNodes.filter(node => node.type !== 'File').slice(0, 10)
  const color = group ? GROUP_COLORS[group.kind] ?? GROUP_COLORS.unknown : GROUP_COLORS.unknown

  return (
    <div className="w-full h-full flex flex-col" style={{ background: 'var(--cc-bg)' }}>
      <div className="h-10 flex items-center gap-3 px-4 shrink-0" style={{ borderBottom: '1px solid var(--cc-border)', background: 'var(--cc-panel)' }}>
        <Network size={15} style={{ color: 'var(--cc-text-subtle)' }} />
        <span style={{ fontSize: 12, color: 'var(--cc-text)', fontWeight: 700 }}>Project Map</span>
        <span style={{ fontSize: 11, color: 'var(--cc-text-subtle)' }}>
          {group ? `${group.label} only · no cross-area links` : 'No visible project areas'}
        </span>
      </div>
      <div className="flex-1 overflow-auto p-5">
        {group ? (
          <div className="arch-single-map">
            <section
              className="arch-panel"
              style={{
                borderColor: color.border,
                background: color.bg,
                maxWidth: 760,
                boxShadow: 'none',
              }}
            >
              <div className="flex items-start justify-between gap-4">
                <div>
                  <div style={{ color: 'var(--cc-text)', fontSize: 18, fontWeight: 820 }}>{group.label}</div>
                  <div style={{ color: 'var(--cc-text-subtle)', fontSize: 12, marginTop: 5 }}>
                    {group.fileCount} files · {group.symbolCount} symbols · {group.incomingCount} incoming · {group.outgoingCount} outgoing
                  </div>
                </div>
                <button className="arch-action" onClick={() => onSelectGroup(group.id)}>Drill down</button>
              </div>

              <div className="grid gap-4 mt-5" style={{ gridTemplateColumns: 'minmax(180px, 1fr) minmax(180px, 1fr)' }}>
                <div>
                  <div className="arch-eyebrow">Files</div>
                  <div className="mt-2 space-y-2">
                    {files.map(node => (
                      <button key={node.id} className="arch-list-row" onClick={() => onSelectNode(node.id)}>
                        <span style={{ color: node.id === selectedNodeId ? color.accent : 'var(--cc-text)', fontFamily: 'monospace' }}>{node.label}</span>
                        <span>{node.connections ?? ''}</span>
                      </button>
                    ))}
                    {!files.length && <div style={{ color: 'var(--cc-text-subtle)', fontSize: 12 }}>No file nodes in this filtered view.</div>}
                  </div>
                </div>
                <div>
                  <div className="arch-eyebrow">Key Symbols</div>
                  <div className="mt-2 grid gap-2" style={{ gridTemplateColumns: 'repeat(auto-fit, minmax(160px, 1fr))' }}>
                    {symbols.map(node => (
                      <button key={node.id} className="arch-node-row" onClick={() => onSelectNode(node.id)}>
                        <span className="arch-node-type">{node.type}</span>
                        <span className="truncate" style={{ color: node.id === selectedNodeId ? color.accent : 'var(--cc-text)', fontFamily: 'monospace' }}>{node.label}</span>
                        <span className="truncate" style={{ color: 'var(--cc-text-faint)', fontSize: 10 }}>{node.file}</span>
                      </button>
                    ))}
                    {!symbols.length && <div style={{ color: 'var(--cc-text-subtle)', fontSize: 12 }}>No symbol nodes in this filtered view.</div>}
                  </div>
                </div>
              </div>
            </section>
          </div>
        ) : (
          <div className="arch-empty-centered">
            <div className="arch-empty">No visible groups. Relax graph filters or open Raw Graph to inspect the current snapshot.</div>
          </div>
        )}
      </div>
    </div>
  )
}

function ProjectEdge({ routedEdge }: { routedEdge: RoutedProjectEdge }) {
  const { edge, points, labelPoint, sourceLabel, targetLabel } = routedEdge
  const path = points.map((point, pointIndex) => `${pointIndex === 0 ? 'M' : 'L'} ${point.x} ${point.y}`).join(' ')
  const width = Math.min(8, 1 + Math.log(edge.count + 1))
  const label = formatEdgeLabel(edge)
  const labelWidth = Math.max(76, label.length * 6 + 18)
  return (
    <g>
      <title>{edgeTooltip(edge, sourceLabel, targetLabel)}</title>
      <path d={path} fill="none" stroke="var(--cc-text-subtle)" strokeWidth={width} opacity="0.38" strokeLinecap="round" strokeLinejoin="round" markerEnd="url(#arch-arrow)" />
      <rect x={labelPoint.x - labelWidth / 2} y={labelPoint.y - 13} width={labelWidth} height="22" rx="6" fill="var(--cc-overlay)" stroke="var(--cc-border)" />
      <text x={labelPoint.x} y={labelPoint.y + 4} fill="var(--cc-text-muted)" fontSize="10" textAnchor="middle">
        {label}
      </text>
    </g>
  )
}

function formatEdgeLabel(edge: AggregatedProjectEdge) {
  const entries = Object.entries(edge.edgeTypes)
    .filter((entry): entry is [string, number] => typeof entry[1] === 'number')
    .sort((a, b) => b[1] - a[1])
  if (!entries.length) return `${edge.count} relationships`
  if (entries.length === 1) return `${edge.count} ${edgeTypeLabel(entries[0][0])}`
  const apiDataCount = (edge.edgeTypes.ApiCall ?? 0) + (edge.edgeTypes.DataFlow ?? 0)
  if (apiDataCount === edge.count && apiDataCount > 0) return `${edge.count} API/Data`
  return entries.slice(0, 2).map(([type, count]) => `${count} ${edgeTypeLabel(type)}`).join(' · ')
}

function edgeTooltip(edge: AggregatedProjectEdge, sourceLabel: string, targetLabel: string) {
  const lines = [`${sourceLabel} -> ${targetLabel}`, `${edge.count} relationships`, '']
  for (const [type, count] of Object.entries(edge.edgeTypes).sort((a, b) => (b[1] ?? 0) - (a[1] ?? 0))) {
    lines.push(`${type}: ${count}`)
  }
  if (edge.examples?.length) {
    lines.push('', 'Top edges:')
    for (const example of edge.examples.slice(0, 4)) {
      lines.push(`- ${example.sourceLabel} -> ${example.targetLabel}`)
    }
  }
  return lines.join('\n')
}

function edgeTypeLabel(type: string) {
  if (type === 'ApiCall') return 'ApiCall'
  if (type === 'DataFlow') return 'DataFlow'
  if (type === 'TypeReference') return 'TypeRef'
  return type
}

function formatLanguageBreakdown(breakdown?: Record<string, number>) {
  return Object.entries(breakdown ?? {})
    .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
    .slice(0, 4)
    .map(([language, count]) => `${language}: ${count}`)
    .join(' · ')
}

function shortPath(path: string) {
  const parts = path.split('/').filter(Boolean)
  if (parts.length <= 2) return path
  return parts.slice(-2).join('/')
}

interface GroupLayout extends Rect {
  group: ProjectGroup
  children: ProjectGroup[]
  topFiles: string[]
  symbols: GraphNode[]
  visibleSymbols: GraphNode[]
  width: number
  height: number
  centerX: number
  centerY: number
}

function buildGroupLayouts(groups: ProjectGroup[], byId: Map<string, GraphNode>, grouping: ProjectMapGrouping): GroupLayout[] {
  const roughLayouts = groups.map(group => {
    const children = group.children ?? []
    const topFiles = (group.topFiles ?? []).slice(0, group.children?.length ? 4 : 6)
    const symbols = group.nodeIds
      .map(id => byId.get(id))
      .filter((node): node is GraphNode => Boolean(node))
      .filter(node => node.type !== 'File')
      .sort((a, b) => (b.connections ?? 0) - (a.connections ?? 0) || a.label.localeCompare(b.label))
    const keySymbolIds = new Set(group.keySymbols ?? [])
    const visibleSymbols = (keySymbolIds.size
      ? symbols.filter(node => keySymbolIds.has(node.id))
      : symbols).slice(0, 8)
    const longestChild = Math.max(0, ...children.map(child => child.label.length), ...topFiles.map(file => shortPath(file).length))
    const longestSymbol = Math.max(0, ...visibleSymbols.map(node => node.label.length))
    const width = clamp(Math.max(longestChild, longestSymbol) * 8 + 210, minWidthFor(group), maxWidthFor(group))
    const chipColumns = width >= 390 ? 2 : 1
    const childRows = Math.ceil(children.length / chipColumns)
    const fileRows = Math.ceil(topFiles.length / 2)
    const symbolRows = Math.ceil(visibleSymbols.length / 2)
    const height = 82
      + (children.length ? 24 + childRows * 34 : 0)
      + (topFiles.length ? 24 + fileRows * 24 : 0)
      + (visibleSymbols.length ? 26 + symbolRows * 24 : 0)
      + 34
      + (symbols.length > visibleSymbols.length ? 26 : 8)
    return {
      group,
      children,
      topFiles,
      symbols,
      visibleSymbols,
      w: width,
      h: Math.max(minHeightFor(group), height),
    }
  })

  if (grouping !== 'architecture' || roughLayouts.some(layout => !GROUP_SLOTS[layout.group.id])) {
    return layoutInGrid(roughLayouts)
  }

  const byIdLayout = new Map(roughLayouts.map(layout => [layout.group.id, layout]))
  const columnWidths = [0, 0, 0]
  for (const [groupId, slot] of Object.entries(GROUP_SLOTS)) {
    const layout = byIdLayout.get(groupId)
    if (layout) columnWidths[slot.column] = Math.max(columnWidths[slot.column], layout.w)
  }
  const rowHeights = [0, 0, 0]
  for (const [groupId, slot] of Object.entries(GROUP_SLOTS)) {
    const layout = byIdLayout.get(groupId)
    if (layout) rowHeights[slot.row] = Math.max(rowHeights[slot.row], layout.h)
  }

  const gapX = 180
  const gapY = 120
  const columnX = [80]
  columnX[1] = columnX[0] + columnWidths[0] + gapX
  columnX[2] = columnX[1] + columnWidths[1] + gapX
  const rowY = [86]
  rowY[1] = rowY[0] + rowHeights[0] + gapY
  rowY[2] = rowY[1] + rowHeights[1] + gapY

  return roughLayouts.map(layout => {
    const slot = GROUP_SLOTS[layout.group.id]
    return withLayoutMetadata({
      ...layout,
      x: columnX[slot.column] + Math.max(0, (columnWidths[slot.column] - layout.w) / 2),
      y: rowY[slot.row] + Math.max(0, (rowHeights[slot.row] - layout.h) / 2),
    })
  })
}

function layoutInGrid(layouts: Array<Omit<GroupLayout, 'x' | 'y' | 'width' | 'height' | 'centerX' | 'centerY'>>) {
  const columns = Math.min(3, Math.max(1, Math.ceil(Math.sqrt(layouts.length))))
  const gapX = 130
  const gapY = 110
  const columnWidths = Array.from({ length: columns }, (_, column) =>
    Math.max(280, ...layouts.filter((_, index) => index % columns === column).map(layout => layout.w)),
  )
  const rowCount = Math.ceil(layouts.length / columns)
  const rowHeights = Array.from({ length: rowCount }, (_, row) =>
    Math.max(160, ...layouts.slice(row * columns, row * columns + columns).map(layout => layout.h)),
  )
  const xOffsets = columnWidths.reduce<number[]>((offsets, width, index) => {
    offsets[index] = index === 0 ? 80 : offsets[index - 1] + columnWidths[index - 1] + gapX
    return offsets
  }, [])
  const yOffsets = rowHeights.reduce<number[]>((offsets, height, index) => {
    offsets[index] = index === 0 ? 86 : offsets[index - 1] + rowHeights[index - 1] + gapY
    return offsets
  }, [])
  return layouts.map((layout, index) => {
    const column = index % columns
    const row = Math.floor(index / columns)
    return withLayoutMetadata({
      ...layout,
      x: xOffsets[column] + Math.max(0, (columnWidths[column] - layout.w) / 2),
      y: yOffsets[row] + Math.max(0, (rowHeights[row] - layout.h) / 2),
    })
  })
}

function withLayoutMetadata<T extends Rect>(layout: T): T & Pick<GroupLayout, 'width' | 'height' | 'centerX' | 'centerY'> {
  return {
    ...layout,
    width: layout.w,
    height: layout.h,
    centerX: layout.x + layout.w / 2,
    centerY: layout.y + layout.h / 2,
  }
}

function minWidthFor(group: ProjectGroup) {
  if (group.id === 'frontend' || group.id === 'backend') return 390
  if (group.id === 'shared') return 330
  return 270
}

function maxWidthFor(group: ProjectGroup) {
  if (group.id === 'frontend' || group.id === 'backend') return 520
  return 430
}

function minHeightFor(group: ProjectGroup) {
  if (group.id === 'frontend' || group.id === 'backend') return 260
  if (group.id === 'shared') return 190
  return 160
}

function bundleParallelProjectEdges(edges: AggregatedProjectEdge[]): AggregatedProjectEdge[] {
  const buckets = new Map<string, AggregatedProjectEdge>()
  for (const edge of edges) {
    const key = `${edge.sourceGroupId}->${edge.targetGroupId}`
    const existing = buckets.get(key)
    if (!existing) {
      buckets.set(key, {
        ...edge,
        id: key,
        edgeTypes: { ...edge.edgeTypes },
        underlyingEdgeIds: [...edge.underlyingEdgeIds],
        examples: [...(edge.examples ?? [])],
      })
      continue
    }
    existing.count += edge.count
    existing.underlyingEdgeIds = uniqueStrings([...existing.underlyingEdgeIds, ...edge.underlyingEdgeIds])
    existing.examples = [...(existing.examples ?? []), ...(edge.examples ?? [])].slice(0, 6)
    for (const [type, count] of Object.entries(edge.edgeTypes)) {
      const edgeType = type as EdgeType
      existing.edgeTypes[edgeType] = (existing.edgeTypes[edgeType] ?? 0) + (count ?? 0)
    }
  }
  return [...buckets.values()].sort((a, b) => b.count - a.count || a.id.localeCompare(b.id))
}

function routeProjectEdges(edges: AggregatedProjectEdge[], layouts: GroupLayout[]): RoutedProjectEdge[] {
  const layoutById = new Map(layouts.map(layout => [layout.group.id, layout]))
  const edgePlans = edges
    .map(edge => {
      const source = layoutById.get(edge.sourceGroupId)
      const target = layoutById.get(edge.targetGroupId)
      if (!source || !target) return null
      const sides = sidesFor(source, target)
      return { edge, source, target, ...sides }
    })
    .filter((plan): plan is EdgePlan => Boolean(plan))

  const ports = assignPorts(edgePlans)
  const obstacles = layouts.map(layout => inflateRect(layout, 24))
  const accepted: RoutedProjectEdge[] = []
  const occupiedLabels: Rect[] = []

  edgePlans.forEach((plan, index) => {
    const sourcePort = ports.get(`${plan.edge.id}:source`) ?? sideCenter(plan.source, plan.sourceSide)
    const targetPort = ports.get(`${plan.edge.id}:target`) ?? sideCenter(plan.target, plan.targetSide)
    const route = chooseBestRoute(plan, sourcePort, targetPort, obstacles, accepted, index)
    const label = formatEdgeLabel(plan.edge)
    const labelSize = { w: Math.max(76, label.length * 6 + 18), h: 22 }
    const labelPoint = placeLabel(route.points, labelSize, occupiedLabels)
    occupiedLabels.push({ x: labelPoint.x - labelSize.w / 2, y: labelPoint.y - labelSize.h / 2, w: labelSize.w, h: labelSize.h })
    accepted.push({
      edge: plan.edge,
      points: route.points,
      labelPoint,
      sourceLabel: plan.source.group.label,
      targetLabel: plan.target.group.label,
    })
  })

  return accepted
}

function sidesFor(source: GroupLayout, target: GroupLayout): Pick<EdgePlan, 'sourceSide' | 'targetSide' | 'orientation'> {
  const dx = target.centerX - source.centerX
  const dy = target.centerY - source.centerY
  if (Math.abs(dx) >= Math.abs(dy)) {
    return dx >= 0
      ? { sourceSide: 'right', targetSide: 'left', orientation: 'horizontal' }
      : { sourceSide: 'left', targetSide: 'right', orientation: 'horizontal' }
  }
  return dy >= 0
    ? { sourceSide: 'bottom', targetSide: 'top', orientation: 'vertical' }
    : { sourceSide: 'top', targetSide: 'bottom', orientation: 'vertical' }
}

function assignPorts(edgePlans: EdgePlan[]) {
  const incident = new Map<string, Array<{ edgeId: string; role: 'source' | 'target'; opposite: number }>>()
  for (const plan of edgePlans) {
    const sourceKey = `${plan.source.group.id}:${plan.sourceSide}`
    const targetKey = `${plan.target.group.id}:${plan.targetSide}`
    incident.set(sourceKey, [...(incident.get(sourceKey) ?? []), {
      edgeId: plan.edge.id,
      role: 'source',
      opposite: plan.sourceSide === 'left' || plan.sourceSide === 'right' ? plan.target.centerY : plan.target.centerX,
    }])
    incident.set(targetKey, [...(incident.get(targetKey) ?? []), {
      edgeId: plan.edge.id,
      role: 'target',
      opposite: plan.targetSide === 'left' || plan.targetSide === 'right' ? plan.source.centerY : plan.source.centerX,
    }])
  }

  const layoutByGroup = new Map(edgePlans.flatMap(plan => [[plan.source.group.id, plan.source], [plan.target.group.id, plan.target]] as Array<[string, GroupLayout]>))
  const ports = new Map<string, Point>()
  for (const [key, entries] of incident) {
    const [groupId, side] = key.split(':') as [string, Side]
    const rect = layoutByGroup.get(groupId)
    if (!rect) continue
    const sorted = entries.slice().sort((a, b) => a.opposite - b.opposite || a.edgeId.localeCompare(b.edgeId))
    sorted.forEach((entry, index) => {
      ports.set(`${entry.edgeId}:${entry.role}`, portAt(rect, side, index, sorted.length))
    })
  }
  return ports
}

function portAt(rect: Rect, side: Side, index: number, count: number): Point {
  const pad = 34
  const ratio = (index + 1) / (count + 1)
  if (side === 'left' || side === 'right') {
    return { x: side === 'left' ? rect.x : rect.x + rect.w, y: rect.y + pad + (rect.h - pad * 2) * ratio }
  }
  return { x: rect.x + pad + (rect.w - pad * 2) * ratio, y: side === 'top' ? rect.y : rect.y + rect.h }
}

function sideCenter(rect: Rect, side: Side): Point {
  if (side === 'left') return { x: rect.x, y: rect.y + rect.h / 2 }
  if (side === 'right') return { x: rect.x + rect.w, y: rect.y + rect.h / 2 }
  if (side === 'top') return { x: rect.x + rect.w / 2, y: rect.y }
  return { x: rect.x + rect.w / 2, y: rect.y + rect.h }
}

function chooseBestRoute(
  plan: EdgePlan,
  sourcePort: Point,
  targetPort: Point,
  obstacles: Rect[],
  accepted: RoutedProjectEdge[],
  index: number,
) {
  const candidateObstacles = obstacles.filter(rect =>
    !pointInRect({ x: plan.source.centerX, y: plan.source.centerY }, rect)
    && !pointInRect({ x: plan.target.centerX, y: plan.target.centerY }, rect),
  )
  const candidateRoutes = candidateRoutePoints(plan, sourcePort, targetPort, index)
  return candidateRoutes
    .map(points => ({ points: simplifyRoute(points), score: routeScore(points, candidateObstacles, accepted) }))
    .sort((a, b) => a.score - b.score || routeLength(a.points) - routeLength(b.points))[0]
    ?? { points: [sourcePort, targetPort], score: 0 }
}

function candidateRoutePoints(plan: EdgePlan, sourcePort: Point, targetPort: Point, index: number) {
  const laneOffset = (index % 7 - 3) * 16
  const minX = Math.min(plan.source.x, plan.target.x)
  const maxX = Math.max(plan.source.x + plan.source.w, plan.target.x + plan.target.w)
  const minY = Math.min(plan.source.y, plan.target.y)
  const maxY = Math.max(plan.source.y + plan.source.h, plan.target.y + plan.target.h)

  if (plan.orientation === 'horizontal') {
    const between = (sourcePort.x + targetPort.x) / 2
    const candidates = uniqueNumbers([
      between + laneOffset,
      between - laneOffset,
      minX - 64 - Math.abs(laneOffset),
      maxX + 64 + Math.abs(laneOffset),
      plan.sourceSide === 'right' ? plan.source.x + plan.source.w + 72 + laneOffset : plan.source.x - 72 + laneOffset,
      plan.targetSide === 'left' ? plan.target.x - 72 - laneOffset : plan.target.x + plan.target.w + 72 - laneOffset,
    ])
    return candidates.map(midX => [
      sourcePort,
      { x: midX, y: sourcePort.y },
      { x: midX, y: targetPort.y },
      targetPort,
    ])
  }

  const between = (sourcePort.y + targetPort.y) / 2
  const candidates = uniqueNumbers([
    between + laneOffset,
    between - laneOffset,
    minY - 64 - Math.abs(laneOffset),
    maxY + 64 + Math.abs(laneOffset),
    plan.sourceSide === 'bottom' ? plan.source.y + plan.source.h + 72 + laneOffset : plan.source.y - 72 + laneOffset,
    plan.targetSide === 'top' ? plan.target.y - 72 - laneOffset : plan.target.y + plan.target.h + 72 - laneOffset,
  ])
  return candidates.map(midY => [
    sourcePort,
    { x: sourcePort.x, y: midY },
    { x: targetPort.x, y: midY },
    targetPort,
  ])
}

function routeScore(points: Point[], obstacles: Rect[], accepted: RoutedProjectEdge[]) {
  const segments = segmentsFor(points)
  const obstacleIntersections = segments.reduce((count, segment) =>
    count + obstacles.filter(rect => segmentIntersectsRect(segment, rect)).length, 0)
  const existingEdgeCrossings = accepted.reduce((count, route) =>
    count + segmentsCrossingCount(segments, segmentsFor(route.points)), 0)
  const laneOverlap = accepted.reduce((count, route) => count + overlappingSegmentCount(segments, segmentsFor(route.points)), 0)
  return routeLength(points)
    + bendCount(points) * 80
    + obstacleIntersections * 10000
    + existingEdgeCrossings * 2000
    + laneOverlap * 300
}

function simplifyRoute(points: Point[]) {
  const result: Point[] = []
  for (const point of points) {
    const previous = result[result.length - 1]
    if (previous && previous.x === point.x && previous.y === point.y) continue
    result.push(point)
  }
  return result.filter((point, index) => {
    const previous = result[index - 1]
    const next = result[index + 1]
    if (!previous || !next) return true
    return !((previous.x === point.x && point.x === next.x) || (previous.y === point.y && point.y === next.y))
  })
}

function placeLabel(points: Point[], labelSize: { w: number; h: number }, occupiedLabels: Rect[]) {
  const base = labelPointFor(points)
  const offsets = [
    { x: 0, y: 0 },
    { x: 0, y: -14 },
    { x: 0, y: 14 },
    { x: 14, y: 0 },
    { x: -14, y: 0 },
    { x: 14, y: -14 },
    { x: -14, y: 14 },
  ]
  return offsets
    .map(offset => ({ x: base.x + offset.x, y: base.y + offset.y }))
    .find(point => !occupiedLabels.some(rect => rectsOverlap({ x: point.x - labelSize.w / 2, y: point.y - labelSize.h / 2, w: labelSize.w, h: labelSize.h }, rect)))
    ?? { x: base.x, y: base.y + occupiedLabels.length * 12 }
}

function labelPointFor(points: Point[]) {
  let best = { x: points[0].x, y: points[0].y }
  let bestLength = -1
  for (let index = 1; index < points.length; index += 1) {
    const previous = points[index - 1]
    const current = points[index]
    const length = Math.hypot(current.x - previous.x, current.y - previous.y)
    if (length > bestLength) {
      bestLength = length
      best = { x: (previous.x + current.x) / 2, y: (previous.y + current.y) / 2 }
    }
  }
  return best
}

function segmentsFor(points: Point[]): Segment[] {
  const segments: Segment[] = []
  for (let index = 1; index < points.length; index += 1) {
    segments.push({ a: points[index - 1], b: points[index] })
  }
  return segments
}

function routeLength(points: Point[]) {
  return segmentsFor(points).reduce((sum, segment) => sum + segmentLength(segment), 0)
}

function bendCount(points: Point[]) {
  let bends = 0
  for (let index = 2; index < points.length; index += 1) {
    const previous = points[index - 2]
    const current = points[index - 1]
    const next = points[index]
    const directionA = previous.x === current.x ? 'v' : 'h'
    const directionB = current.x === next.x ? 'v' : 'h'
    if (directionA !== directionB) bends += 1
  }
  return bends
}

function segmentLength(segment: Segment) {
  return Math.abs(segment.a.x - segment.b.x) + Math.abs(segment.a.y - segment.b.y)
}

function segmentIntersectsRect(segment: Segment, rect: Rect) {
  if (pointInRect(segment.a, rect) || pointInRect(segment.b, rect)) return true
  const left = rect.x
  const right = rect.x + rect.w
  const top = rect.y
  const bottom = rect.y + rect.h
  if (segment.a.x === segment.b.x) {
    const x = segment.a.x
    const [y1, y2] = sortedPair(segment.a.y, segment.b.y)
    return x >= left && x <= right && y2 >= top && y1 <= bottom
  }
  if (segment.a.y === segment.b.y) {
    const y = segment.a.y
    const [x1, x2] = sortedPair(segment.a.x, segment.b.x)
    return y >= top && y <= bottom && x2 >= left && x1 <= right
  }
  return false
}

function segmentsCrossingCount(segments: Segment[], existing: Segment[]) {
  let count = 0
  for (const segment of segments) {
    for (const other of existing) {
      if (segmentsCross(segment, other)) count += 1
    }
  }
  return count
}

function overlappingSegmentCount(segments: Segment[], existing: Segment[]) {
  let count = 0
  for (const segment of segments) {
    for (const other of existing) {
      if (segmentsOverlap(segment, other)) count += 1
    }
  }
  return count
}

function segmentsCross(a: Segment, b: Segment) {
  const aVertical = a.a.x === a.b.x
  const bVertical = b.a.x === b.b.x
  if (aVertical === bVertical) return false
  const vertical = aVertical ? a : b
  const horizontal = aVertical ? b : a
  const [vy1, vy2] = sortedPair(vertical.a.y, vertical.b.y)
  const [hx1, hx2] = sortedPair(horizontal.a.x, horizontal.b.x)
  const x = vertical.a.x
  const y = horizontal.a.y
  return x > hx1 && x < hx2 && y > vy1 && y < vy2
}

function segmentsOverlap(a: Segment, b: Segment) {
  if (a.a.x === a.b.x && b.a.x === b.b.x && a.a.x === b.a.x) {
    const [a1, a2] = sortedPair(a.a.y, a.b.y)
    const [b1, b2] = sortedPair(b.a.y, b.b.y)
    return Math.min(a2, b2) - Math.max(a1, b1) > 8
  }
  if (a.a.y === a.b.y && b.a.y === b.b.y && a.a.y === b.a.y) {
    const [a1, a2] = sortedPair(a.a.x, a.b.x)
    const [b1, b2] = sortedPair(b.a.x, b.b.x)
    return Math.min(a2, b2) - Math.max(a1, b1) > 8
  }
  return false
}

function inflateRect(rect: Rect, padding: number): Rect {
  return {
    x: rect.x - padding,
    y: rect.y - padding,
    w: rect.w + padding * 2,
    h: rect.h + padding * 2,
  }
}

function rectsOverlap(a: Rect, b: Rect) {
  return a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y
}

function pointInRect(point: Point, rect: Rect) {
  return point.x >= rect.x && point.x <= rect.x + rect.w && point.y >= rect.y && point.y <= rect.y + rect.h
}

function uniqueStrings(values: string[]) {
  return [...new Set(values)]
}

function uniqueNumbers(values: number[]) {
  return [...new Set(values.map(value => Math.round(value)))]
}

function sortedPair(a: number, b: number): [number, number] {
  return a <= b ? [a, b] : [b, a]
}

type Rect = { x: number; y: number; w: number; h: number }
type Point = { x: number; y: number }
type Segment = { a: Point; b: Point }
type Side = 'left' | 'right' | 'top' | 'bottom'
type RouteOrientation = 'horizontal' | 'vertical'

interface EdgePlan {
  edge: AggregatedProjectEdge
  source: GroupLayout
  target: GroupLayout
  sourceSide: Side
  targetSide: Side
  orientation: RouteOrientation
}

interface RoutedProjectEdge {
  edge: AggregatedProjectEdge
  points: Point[]
  labelPoint: Point
  sourceLabel: string
  targetLabel: string
}

function clamp(value: number, min: number, max: number) {
  return Math.max(min, Math.min(max, value))
}
