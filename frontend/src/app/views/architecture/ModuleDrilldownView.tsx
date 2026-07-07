import { ChevronLeft, FolderTree, Maximize2 } from 'lucide-react'
import type { GraphNode } from '../../types'
import type { ProjectGroup, ProjectMapModel } from './architectureTypes'

interface ModuleDrilldownViewProps {
  model: ProjectMapModel
  nodes: GraphNode[]
  selectedGroupId: string | null
  onBack: () => void
  onSelectNode: (id: string | null) => void
  onOpenNode: (node: GraphNode) => void
}

export function ModuleDrilldownView({ model, nodes, selectedGroupId, onBack, onSelectNode, onOpenNode }: ModuleDrilldownViewProps) {
  const group = model.groups.find(candidate => candidate.id === selectedGroupId) ?? model.groups[0]
  const nodeById = new Map(nodes.map(node => [node.id, node]))
  const groupNodes = group ? group.nodeIds.map(id => nodeById.get(id)).filter((node): node is GraphNode => Boolean(node)) : []
  const fileNodes = groupNodes.filter(node => node.type === 'File')
  const symbolNodes = groupNodes.filter(node => node.type !== 'File').slice(0, 80)
  const externalLinks = model.edges.filter(edge => edge.sourceGroupId === group?.id || edge.targetGroupId === group?.id)

  if (!group) return <div className="arch-empty">No project groups available for drill-down.</div>

  return (
    <div className="w-full h-full flex flex-col" style={{ background: 'var(--cc-bg)' }}>
      <div className="h-10 flex items-center gap-2 px-4 shrink-0" style={{ borderBottom: '1px solid var(--cc-border)', background: 'var(--cc-panel)' }}>
        <button className="arch-icon-button" onClick={onBack} title="Back to Project Map"><ChevronLeft size={15} /></button>
        <FolderTree size={15} style={{ color: 'var(--cc-text-subtle)' }} />
        <span style={{ fontSize: 12, color: 'var(--cc-text-subtle)' }}>Project /</span>
        <span style={{ fontSize: 12, color: 'var(--cc-text)', fontWeight: 750 }}>{group.label}</span>
        <span style={{ fontSize: 11, color: 'var(--cc-text-subtle)' }}>{group.fileCount} files · {group.symbolCount} symbols</span>
        <button className="arch-action ml-auto" onClick={onBack}><Maximize2 size={13} /> Reset view</button>
      </div>
      <div className="flex-1 overflow-auto p-4 grid gap-4" style={{ gridTemplateColumns: 'minmax(240px, 320px) 1fr minmax(220px, 280px)' }}>
        <section className="arch-panel">
          <div className="arch-eyebrow">Internal Structure</div>
          <div className="mt-3 space-y-2">
            {(group.children ?? []).map(child => (
              <button key={child.id} className="arch-list-row" onClick={() => child.nodeIds[0] && onSelectNode(child.nodeIds[0])}>
                <span>{child.label}</span>
                <span>{child.fileCount || child.symbolCount}</span>
              </button>
            ))}
            {!group.children?.length && <div style={{ fontSize: 12, color: 'var(--cc-text-subtle)' }}>No directory subgroups detected.</div>}
          </div>
        </section>
        <section className="arch-panel">
          <div className="arch-eyebrow">Files and Symbols</div>
          <div className="grid gap-2 mt-3" style={{ gridTemplateColumns: 'repeat(auto-fit, minmax(220px, 1fr))' }}>
            {[...fileNodes, ...symbolNodes].slice(0, 120).map(node => (
              <button key={node.id} className="arch-node-row" onClick={() => onSelectNode(node.id)} onDoubleClick={() => onOpenNode(node)}>
                <span className="arch-node-type">{node.type}</span>
                <span className="truncate" style={{ color: 'var(--cc-text)', fontFamily: 'monospace' }}>{node.label}</span>
                <span className="truncate" style={{ color: 'var(--cc-text-faint)', fontSize: 10 }}>{node.file}</span>
              </button>
            ))}
          </div>
        </section>
        <section className="arch-panel">
          <div className="arch-eyebrow">External Context</div>
          <div className="mt-3 space-y-2">
            {externalLinks.slice(0, 12).map(edge => {
              const otherId = edge.sourceGroupId === group.id ? edge.targetGroupId : edge.sourceGroupId
              const other = model.groups.find(candidate => candidate.id === otherId)
              return (
                <div key={edge.id} className="arch-list-row-static">
                  <span>{other?.label ?? otherId}</span>
                  <span>{edge.count}</span>
                </div>
              )
            })}
          </div>
        </section>
      </div>
    </div>
  )
}
