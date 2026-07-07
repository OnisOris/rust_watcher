import { AlertCircle, AlertTriangle, ExternalLink, EyeOff, Info, Network, Star } from 'lucide-react'
import type { GraphNode } from '../../types'
import type { HotspotIssue, HotspotSeverity } from './architectureTypes'

interface HotspotsViewProps {
  issues: HotspotIssue[]
  nodes: GraphNode[]
  onSelectNode: (id: string | null) => void
  onOpenNode: (node: GraphNode) => void
  onOpenNeighborhood: () => void
}

const SEVERITY: Record<HotspotSeverity, { label: string; color: string; icon: typeof AlertTriangle }> = {
  critical: { label: 'Critical', color: '#EF4444', icon: AlertCircle },
  warning: { label: 'Warning', color: '#F59E0B', icon: AlertTriangle },
  info: { label: 'Info', color: '#38BDF8', icon: Info },
  noise: { label: 'Noise candidates', color: '#94A3B8', icon: EyeOff },
}

export function HotspotsView({ issues, nodes, onSelectNode, onOpenNode, onOpenNeighborhood }: HotspotsViewProps) {
  const byId = new Map(nodes.map(node => [node.id, node]))
  const groups: HotspotSeverity[] = ['critical', 'warning', 'info', 'noise']

  return (
    <div className="w-full h-full overflow-auto" style={{ background: 'var(--cc-bg)' }}>
      <div className="h-10 flex items-center gap-3 px-4 sticky top-0 z-10" style={{ borderBottom: '1px solid var(--cc-border)', background: 'var(--cc-panel)' }}>
        <AlertTriangle size={15} style={{ color: '#F59E0B' }} />
        <span style={{ fontSize: 12, color: 'var(--cc-text)', fontWeight: 700 }}>Architecture Hotspots</span>
        <span style={{ fontSize: 11, color: 'var(--cc-text-subtle)' }}>{issues.length} issues and noise candidates</span>
      </div>
      <div className="p-4 space-y-5">
        {issues.length === 0 && (
          <div className="arch-empty-centered" style={{ minHeight: 360 }}>
            <EmptyMessage>No hotspot heuristics fired for the current filtered graph. For this small Rust-only view, Raw Graph or Module drilldown is likely more useful.</EmptyMessage>
          </div>
        )}
        {groups.map(severity => {
          const items = issues.filter(issue => issue.severity === severity)
          if (!items.length) return null
          const config = SEVERITY[severity]
          const Icon = config.icon
          return (
            <section key={severity}>
              <div className="flex items-center gap-2 mb-2">
                <Icon size={14} style={{ color: config.color }} />
                <div className="arch-eyebrow">{config.label}</div>
                <span style={{ fontSize: 11, color: 'var(--cc-text-faint)' }}>{items.length}</span>
              </div>
              <div className="grid gap-2" style={{ gridTemplateColumns: 'repeat(auto-fit, minmax(280px, 1fr))' }}>
                {items.map(issue => {
                  const node = issue.nodeId ? byId.get(issue.nodeId) : undefined
                  return (
                    <article key={issue.id} className="arch-card">
                      <div className="flex items-start justify-between gap-3">
                        <div className="min-w-0">
                          <div style={{ fontSize: 13, color: 'var(--cc-text)', fontWeight: 760 }}>{issue.title}</div>
                          <div style={{ fontSize: 11, color: 'var(--cc-text-subtle)', marginTop: 4, lineHeight: 1.45 }}>{issue.description}</div>
                        </div>
                        <span className="arch-badge" style={{ borderColor: `${config.color}66`, color: config.color }}>{issue.kind}</span>
                      </div>
                      {issue.suggestion && <div style={{ fontSize: 11, color: 'var(--cc-text-muted)', marginTop: 10 }}>{issue.suggestion}</div>}
                      <div className="flex items-center gap-2 mt-3">
                        <span style={{ fontSize: 11, color: 'var(--cc-text-faint)' }}>{issue.connections} connections</span>
                        <div className="ml-auto flex gap-1">
                          <button className="arch-action" disabled={!node} onClick={() => { if (node) { onSelectNode(node.id); onOpenNeighborhood() } }} title="Show local graph"><Network size={13} /></button>
                          <button className="arch-action" disabled={!node?.file} onClick={() => node && onOpenNode(node)} title="Open file"><ExternalLink size={13} /></button>
                          <button className="arch-action" title="Mark important"><Star size={13} /></button>
                        </div>
                      </div>
                    </article>
                  )
                })}
              </div>
            </section>
          )
        })}
      </div>
    </div>
  )
}

function EmptyMessage({ children }: { children: string }) {
  return <div className="arch-empty">{children}</div>
}
