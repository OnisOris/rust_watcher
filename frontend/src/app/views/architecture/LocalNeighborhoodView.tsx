import { GitBranch, Radius } from 'lucide-react'
import { useMemo, useState } from 'react'
import { buildLocalNeighborhoodModel } from '../../api/localNeighborhood'
import type { GraphEdge, GraphNode, ProjectFile } from '../../types'

interface LocalNeighborhoodViewProps {
  nodes: GraphNode[]
  edges: GraphEdge[]
  files: ProjectFile[]
  selectedNodeId: string | null
  onSelectNode: (id: string | null) => void
  onOpenNode: (node: GraphNode) => void
}

export function LocalNeighborhoodView({ nodes, edges, files, selectedNodeId, onSelectNode, onOpenNode }: LocalNeighborhoodViewProps) {
  const [radius, setRadius] = useState<1 | 2 | 3>(1)
  const resolvedSelectedNodeId = useMemo(
    () => resolveSelectedNodeId(nodes, files, selectedNodeId),
    [nodes, files, selectedNodeId],
  )
  const model = useMemo(
    () => resolvedSelectedNodeId ? buildLocalNeighborhoodModel(nodes, edges, resolvedSelectedNodeId, radius) : null,
    [nodes, edges, resolvedSelectedNodeId, radius],
  )

  if (!resolvedSelectedNodeId || !model?.centerNode) {
    return (
      <div className="w-full h-full flex items-center justify-center" style={{ background: 'var(--cc-bg)' }}>
        <div className="arch-empty">Select a file, symbol, endpoint, type, or function to inspect its local neighborhood.</div>
      </div>
    )
  }

  return (
    <div className="w-full h-full flex flex-col" style={{ background: 'var(--cc-bg)' }}>
      <div className="h-10 flex items-center gap-3 px-4 shrink-0" style={{ borderBottom: '1px solid var(--cc-border)', background: 'var(--cc-panel)' }}>
        <GitBranch size={15} style={{ color: 'var(--cc-text-subtle)' }} />
        <span style={{ fontSize: 12, color: 'var(--cc-text)', fontWeight: 700 }}>Local Neighborhood</span>
        <span style={{ fontSize: 11, color: 'var(--cc-text-subtle)' }}>{model.visibleNodes.length} nodes · {model.visibleEdges.length} edges</span>
        <div className="ml-auto flex items-center gap-1">
          <Radius size={14} style={{ color: 'var(--cc-text-subtle)' }} />
          {([1, 2, 3] as const).map(value => (
            <button key={value} className={`arch-segment ${radius === value ? 'arch-segment-active' : ''}`} onClick={() => setRadius(value)}>R{value}</button>
          ))}
        </div>
      </div>
      <div className="flex-1 min-h-0 p-4 grid gap-4" style={{ gridTemplateColumns: 'minmax(220px, 1fr) minmax(280px, 360px) minmax(220px, 1fr)' }}>
        <NeighborhoodColumn title="Who uses this?" nodes={model.incomingNodes} empty="No incoming dependencies in this radius." onSelectNode={onSelectNode} />
        <section className="arch-panel flex flex-col items-center justify-center text-center">
          <span className="arch-node-type">{model.centerNode.type}</span>
          <h3 style={{ color: 'var(--cc-text)', fontSize: 18, fontWeight: 800, marginTop: 10, wordBreak: 'break-word' }}>{model.centerNode.label}</h3>
          <p style={{ color: 'var(--cc-text-subtle)', fontSize: 12, marginTop: 8, wordBreak: 'break-word' }}>{model.centerNode.file ?? model.centerNode.module ?? 'No source path'}</p>
          <div className="grid grid-cols-2 gap-2 mt-5 w-full">
            <div className="arch-stat-card"><div className="arch-stat-value">{model.incomingNodes.length}</div><div className="arch-stat-label">incoming</div></div>
            <div className="arch-stat-card"><div className="arch-stat-value">{model.outgoingNodes.length}</div><div className="arch-stat-label">outgoing</div></div>
          </div>
          <button className="arch-primary mt-5" disabled={!model.centerNode.file} onClick={() => onOpenNode(model.centerNode)}>Open in IDE</button>
          <div className="mt-4 flex flex-wrap justify-center gap-2">
            <span className="arch-badge">API {model.relatedApiNodes.length}</span>
            <span className="arch-badge">Types {model.relatedTypeNodes.length}</span>
            <span className="arch-badge">Tests {model.relatedTestNodes.length}</span>
          </div>
        </section>
        <NeighborhoodColumn title="What does this use?" nodes={model.outgoingNodes} empty="No outgoing dependencies in this radius." onSelectNode={onSelectNode} />
      </div>
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

function normalizePath(path?: string | null) {
  return (path ?? '').replaceAll('\\', '/').replace(/^\/+/, '')
}

function NeighborhoodColumn({ title, nodes, empty, onSelectNode }: { title: string; nodes: GraphNode[]; empty: string; onSelectNode: (id: string | null) => void }) {
  return (
    <section className="arch-panel overflow-auto">
      <div className="arch-eyebrow">{title}</div>
      <div className="mt-3 space-y-2">
        {nodes.map(node => (
          <button key={node.id} className="arch-node-row" onClick={() => onSelectNode(node.id)}>
            <span className="arch-node-type">{node.type}</span>
            <span className="truncate" style={{ color: 'var(--cc-text)', fontFamily: 'monospace' }}>{node.label}</span>
            <span className="truncate" style={{ color: 'var(--cc-text-faint)', fontSize: 10 }}>{node.file}</span>
          </button>
        ))}
        {!nodes.length && <div style={{ fontSize: 12, color: 'var(--cc-text-subtle)' }}>{empty}</div>}
      </div>
    </section>
  )
}
