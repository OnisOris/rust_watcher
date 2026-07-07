import { Clipboard, Code2, GitBranch, MapPin, Network, Pin, Radius, Workflow } from 'lucide-react'
import type { ReactNode } from 'react'
import { useEffect } from 'react'
import { useMemo, useState } from 'react'
import { buildLocalNeighborhoodModel, type LocalNeighborhoodRadius } from '../../api/localNeighborhood'
import type { EdgeType, GraphEdge, GraphNode, ProjectFile } from '../../types'
import type { LocalNeighborhoodGroup } from './architectureTypes'

interface LocalNeighborhoodViewProps {
  nodes: GraphNode[]
  edges: GraphEdge[]
  files: ProjectFile[]
  selectedNodeId: string | null
  depth?: 1 | 2 | 3 | 'full'
  onSelectNode: (id: string | null) => void
  onOpenNode: (node: GraphNode) => void
  onOpenRawGraph?: () => void
  onOpenCallFlow?: () => void
  onOpenApiDataFlow?: () => void
}

const RADIUS_HELP: Record<LocalNeighborhoodRadius, string> = {
  1: 'R1: direct incoming/outgoing only',
  2: 'R2: one more hop through direct neighbors',
  3: 'R3: deep neighborhood, may be noisy',
}

export function LocalNeighborhoodView({
  nodes,
  edges,
  files,
  selectedNodeId,
  depth,
  onSelectNode,
  onOpenNode,
  onOpenRawGraph,
  onOpenCallFlow,
  onOpenApiDataFlow,
}: LocalNeighborhoodViewProps) {
  const [radius, setRadius] = useState<LocalNeighborhoodRadius>(1)
  const [copyState, setCopyState] = useState<'idle' | 'copied'>('idle')
  const resolvedSelectedNodeId = useMemo(
    () => resolveSelectedNodeId(nodes, files, selectedNodeId),
    [nodes, files, selectedNodeId],
  )
  const model = useMemo(
    () => resolvedSelectedNodeId ? buildLocalNeighborhoodModel(nodes, edges, resolvedSelectedNodeId, radius) : null,
    [nodes, edges, resolvedSelectedNodeId, radius],
  )

  useEffect(() => {
    if (depth === 1 || depth === 2 || depth === 3) setRadius(depth)
    if (depth === 'full') setRadius(3)
  }, [depth])

  if (!resolvedSelectedNodeId || !model?.centerNode) {
    return (
      <div className="w-full h-full flex items-center justify-center" style={{ background: 'var(--cc-bg)' }}>
        <div className="arch-empty">Select a file, symbol, endpoint, type, or function to inspect its local neighborhood.</div>
      </div>
    )
  }

  const hasRelationships = model.incomingNodes.length + model.outgoingNodes.length > 0
  const center = model.centerNode
  const visibleFiles = unique(model.visibleNodes.flatMap(node => node.file ? [normalizePath(node.file)] : []))

  return (
    <div className="w-full h-full flex flex-col" style={{ background: 'var(--cc-bg)' }}>
      <div className="min-h-10 flex items-center gap-3 px-4 py-2 shrink-0" style={{ borderBottom: '1px solid var(--cc-border)', background: 'var(--cc-panel)' }}>
        <GitBranch size={15} style={{ color: 'var(--cc-text-subtle)' }} />
        <span style={{ fontSize: 12, color: 'var(--cc-text)', fontWeight: 700 }}>Local Neighborhood</span>
        <span style={{ fontSize: 11, color: 'var(--cc-text-subtle)' }}>{model.visibleNodes.length} nodes · {model.visibleEdges.length} edges</span>
        <div className="ml-auto flex items-center gap-2">
          <span style={{ fontSize: 11, color: 'var(--cc-text-subtle)' }}>{RADIUS_HELP[radius]}</span>
          <Radius size={14} style={{ color: 'var(--cc-text-subtle)' }} />
          {([1, 2, 3] as const).map(value => (
            <button key={value} className={`arch-segment ${radius === value ? 'arch-segment-active' : ''}`} onClick={() => setRadius(value)}>R{value}</button>
          ))}
        </div>
      </div>

      {!hasRelationships ? (
        <div className="flex-1 flex items-center justify-center p-6">
          <div className="arch-empty" style={{ maxWidth: 560 }}>
            No relationships found for this node in the current graph/filter mode. Try Raw Graph or enable more edge types.
          </div>
        </div>
      ) : (
        <div className="flex-1 min-h-0 p-4 grid gap-4" style={{ gridTemplateColumns: 'minmax(260px, 1fr) minmax(340px, 420px) minmax(260px, 1fr)' }}>
          <RelationColumn title="Who calls/imports/uses this?" groups={model.incomingGroups} empty="No incoming dependencies in this radius." onSelectNode={onSelectNode} />

          <section className="arch-panel overflow-auto">
            <div className="flex items-start justify-between gap-3">
              <div className="min-w-0">
                <div className="flex flex-wrap gap-2">
                  <span className="arch-node-type">{center.type}</span>
                  {center.language ? <span className="arch-badge">{center.language}</span> : null}
                  {center.visibility ? <span className="arch-badge">{center.visibility}</span> : null}
                </div>
                <h3 style={{ color: 'var(--cc-text)', fontSize: 18, fontWeight: 800, marginTop: 10, wordBreak: 'break-word' }}>{center.label}</h3>
                <p style={{ color: 'var(--cc-text-subtle)', fontSize: 12, marginTop: 8, wordBreak: 'break-word' }}>{center.file ?? 'No source path'}</p>
              </div>
            </div>

            <div className="mt-4 grid gap-2" style={{ fontSize: 12 }}>
              {center.line ? <MetaRow label="Line" value={String(center.line)} /> : null}
              {center.module ? <MetaRow label="Module" value={center.module} /> : null}
              {center.crate ? <MetaRow label="Crate" value={center.crate} /> : null}
            </div>

            <div className="grid grid-cols-2 gap-2 mt-5">
              <BreakdownCard title="Incoming" counts={model.incomingCountByType} />
              <BreakdownCard title="Outgoing" counts={model.outgoingCountByType} />
            </div>

            <div className="mt-5">
              <div className="arch-eyebrow">Related</div>
              <div className="grid grid-cols-3 gap-2 mt-2">
                <RelatedChip label="API" count={model.relatedApiNodes.length} />
                <RelatedChip label="Types" count={model.relatedTypeNodes.length} />
                <RelatedChip label="Tests" count={model.relatedTestNodes.length} />
              </div>
            </div>

            {model.isDense ? (
              <div className="arch-empty mt-4" style={{ textAlign: 'left', padding: 10 }}>
                This neighborhood is dense. Showing top {model.denseNodeLimit ?? 80} nodes by degree.
              </div>
            ) : null}

            <div className="mt-5">
              <div className="arch-eyebrow">Files Involved</div>
              <div className="mt-2 space-y-1">
                {visibleFiles.slice(0, 8).map(file => (
                  <div key={file} title={file} style={{ color: 'var(--cc-text-muted)', fontSize: 11, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{file}</div>
                ))}
                {visibleFiles.length > 8 ? <div style={{ color: 'var(--cc-text-faint)', fontSize: 11 }}>+{visibleFiles.length - 8} more files</div> : null}
              </div>
            </div>

            <div className="mt-5">
              <div className="arch-eyebrow">Actions</div>
              <div className="grid grid-cols-2 gap-2 mt-2">
                <ActionButton icon={<MapPin size={13} />} label="Open in IDE" disabled={!center.file} onClick={() => onOpenNode(center)} />
                <ActionButton icon={<Network size={13} />} label="Raw graph" onClick={onOpenRawGraph} />
                <ActionButton icon={<Workflow size={13} />} label="Call flow" onClick={onOpenCallFlow} />
                <ActionButton icon={<Code2 size={13} />} label="API/Data" onClick={onOpenApiDataFlow} />
                <ActionButton icon={<Pin size={13} />} label="Pin node" disabled />
                <ActionButton
                  icon={<Clipboard size={13} />}
                  label={copyState === 'copied' ? 'Copied' : 'Copy path'}
                  disabled={!center.file}
                  onClick={() => {
                    if (!center.file) return
                    void navigator.clipboard?.writeText(center.file)
                    setCopyState('copied')
                    window.setTimeout(() => setCopyState('idle'), 1200)
                  }}
                />
                <ActionButton icon={<Network size={13} />} label="Hide as noise" disabled wide />
              </div>
            </div>
          </section>

          <RelationColumn title="What does this call/import/use?" groups={model.outgoingGroups} empty="No outgoing dependencies in this radius." onSelectNode={onSelectNode} />
        </div>
      )}
    </div>
  )
}

function resolveSelectedNodeId(nodes: GraphNode[], files: ProjectFile[], selectedNodeId: string | null) {
  if (!selectedNodeId) return null
  if (nodes.some(node => node.id === selectedNodeId)) return selectedNodeId
  const selectedFile = files.find(file => file.id === selectedNodeId)
  if (!selectedFile) return selectedNodeId
  const normalizedPath = normalizePath(selectedFile.path)
  const fileName = selectedFile.name.toLowerCase()
  const fileNode = nodes.find(node => {
    if (node.type !== 'File') return false
    const nodePath = normalizePath(node.file)
    return nodePath === normalizedPath
      || nodePath.endsWith(`/${normalizedPath}`)
      || normalizedPath.endsWith(`/${nodePath}`)
      || node.label.toLowerCase() === fileName
  })
  return fileNode?.id ?? selectedNodeId
}

function RelationColumn({
  title,
  groups,
  empty,
  onSelectNode,
}: {
  title: string
  groups: LocalNeighborhoodGroup[]
  empty: string
  onSelectNode: (id: string | null) => void
}) {
  return (
    <section className="arch-panel overflow-auto">
      <div className="arch-eyebrow">{title}</div>
      <div className="mt-3 space-y-3">
        {groups.map(group => (
          <div key={`${group.edgeType}:${group.label}`}>
            <div className="flex items-center justify-between mb-2">
              <span style={{ color: 'var(--cc-text)', fontSize: 12, fontWeight: 800 }}>{group.label}</span>
              <span className="arch-badge">{group.edges.length}</span>
            </div>
            <div className="space-y-2">
              {group.nodes.map(node => (
                <button key={node.id} className="arch-node-row" onClick={() => onSelectNode(node.id)}>
                  <span className="arch-node-type">{node.type}</span>
                  <span className="truncate" style={{ color: 'var(--cc-text)', fontFamily: 'monospace' }}>{node.label}</span>
                  <span className="truncate" style={{ color: 'var(--cc-text-faint)', fontSize: 10 }}>{node.file}</span>
                </button>
              ))}
            </div>
          </div>
        ))}
        {!groups.length && <div style={{ fontSize: 12, color: 'var(--cc-text-subtle)' }}>{empty}</div>}
      </div>
    </section>
  )
}

function BreakdownCard({ title, counts }: { title: string; counts: Partial<Record<EdgeType, number>> }) {
  const entries = Object.entries(counts).sort(([a], [b]) => a.localeCompare(b))
  return (
    <div className="arch-stat-card" style={{ textAlign: 'left' }}>
      <div className="arch-stat-label" style={{ marginBottom: 8 }}>{title}</div>
      {entries.length ? entries.map(([type, count]) => (
        <div key={type} className="flex justify-between" style={{ fontSize: 11, color: 'var(--cc-text-muted)', gap: 10 }}>
          <span>{edgeLabel(type as EdgeType)}</span>
          <span style={{ color: 'var(--cc-text)', fontWeight: 800 }}>{count}</span>
        </div>
      )) : <div style={{ fontSize: 11, color: 'var(--cc-text-faint)' }}>None</div>}
    </div>
  )
}

function RelatedChip({ label, count }: { label: string; count: number }) {
  return (
    <div className="arch-stat-card" style={{ padding: 8 }}>
      <div className="arch-stat-value" style={{ fontSize: 16 }}>{count}</div>
      <div className="arch-stat-label">{label}</div>
    </div>
  )
}

function MetaRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex justify-between gap-3">
      <span style={{ color: 'var(--cc-text-faint)' }}>{label}</span>
      <span className="truncate" style={{ color: 'var(--cc-text-muted)', textAlign: 'right' }}>{value}</span>
    </div>
  )
}

function ActionButton({
  icon,
  label,
  onClick,
  disabled,
  wide,
}: {
  icon: ReactNode
  label: string
  onClick?: () => void
  disabled?: boolean
  wide?: boolean
}) {
  return (
    <button
      className={wide ? 'col-span-2' : undefined}
      disabled={disabled || !onClick}
      onClick={onClick}
      style={{
        minHeight: 32,
        border: '1px solid var(--cc-border)',
        borderRadius: 7,
        background: 'var(--cc-card)',
        color: disabled || !onClick ? 'var(--cc-text-faint)' : 'var(--cc-text-muted)',
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

function edgeLabel(type: EdgeType) {
  if (type === 'TypeReference') return 'Type refs'
  if (type === 'ApiCall') return 'API calls'
  if (type === 'DataFlow') return 'Data flow'
  return type
}

function normalizePath(path?: string | null) {
  return (path ?? '').replaceAll('\\', '/').replace(/^\/+/, '')
}

function unique<T>(values: T[]) {
  return [...new Set(values)]
}
