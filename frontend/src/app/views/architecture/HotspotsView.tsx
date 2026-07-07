import { AlertCircle, AlertTriangle, Code2, ExternalLink, EyeOff, Info, Network, Table2 } from 'lucide-react'
import { useMemo, useState } from 'react'
import type { GraphNode } from '../../types'
import type { HotspotConfidence, HotspotIssue, HotspotSeverity } from './architectureTypes'

interface HotspotsViewProps {
  issues: HotspotIssue[]
  nodes: GraphNode[]
  onSelectNode: (id: string | null) => void
  onOpenNode: (node: GraphNode) => void
  onOpenNeighborhood: () => void
  onOpenMatrix?: () => void
  onOpenApiDataFlow?: () => void
  onOpenRawGraph?: () => void
}

type HotspotSection = HotspotSeverity | 'hidden-low'

const SEVERITY: Record<HotspotSeverity, { label: string; color: string; icon: typeof AlertTriangle }> = {
  critical: { label: 'Critical', color: '#EF4444', icon: AlertCircle },
  warning: { label: 'Warning', color: '#F59E0B', icon: AlertTriangle },
  info: { label: 'Info', color: '#38BDF8', icon: Info },
  noise: { label: 'Noise candidates', color: '#94A3B8', icon: EyeOff },
}

const CONFIDENCE: Record<HotspotConfidence, { label: string; color: string }> = {
  high: { label: 'high', color: '#EF4444' },
  medium: { label: 'medium', color: '#0EA5E9' },
  low: { label: 'low', color: '#94A3B8' },
}

export function HotspotsView({
  issues,
  nodes,
  onSelectNode,
  onOpenNode,
  onOpenNeighborhood,
  onOpenMatrix,
  onOpenApiDataFlow,
  onOpenRawGraph,
}: HotspotsViewProps) {
  const [showLowConfidence, setShowLowConfidence] = useState(false)
  const byId = useMemo(() => new Map(nodes.map(node => [node.id, node])), [nodes])
  const visibleIssues = showLowConfidence ? issues : issues.filter(issue => issue.confidence !== 'low')
  const hiddenLowIssues = issues.filter(issue => issue.confidence === 'low')
  const groups: HotspotSection[] = ['critical', 'warning', 'info', 'noise', 'hidden-low']

  return (
    <div className="w-full h-full overflow-auto" style={{ background: 'var(--cc-bg)' }}>
      <div className="h-10 flex items-center gap-3 px-4 sticky top-0 z-10" style={{ borderBottom: '1px solid var(--cc-border)', background: 'var(--cc-panel)' }}>
        <AlertTriangle size={15} style={{ color: '#F59E0B' }} />
        <span style={{ fontSize: 12, color: 'var(--cc-text)', fontWeight: 700 }}>Architecture Hotspots</span>
        <span style={{ fontSize: 11, color: 'var(--cc-text-subtle)' }}>{visibleIssues.length} visible · {hiddenLowIssues.length} low-confidence hidden</span>
        <label className="ml-auto flex items-center gap-2" style={{ fontSize: 11, color: 'var(--cc-text-subtle)', cursor: 'pointer' }}>
          <input
            type="checkbox"
            checked={showLowConfidence}
            onChange={event => setShowLowConfidence(event.target.checked)}
          />
          Show low-confidence issues
        </label>
      </div>
      <div className="p-4 space-y-5">
        {visibleIssues.length === 0 && (
          <div className="arch-empty-centered" style={{ minHeight: 360 }}>
            <EmptyMessage>No actionable hotspot heuristics fired for this graph.</EmptyMessage>
          </div>
        )}
        {groups.map(section => {
          if (section === 'hidden-low') {
            if (showLowConfidence || !hiddenLowIssues.length) return null
            return (
              <section key={section}>
                <div className="flex items-center gap-2 mb-2">
                  <EyeOff size={14} style={{ color: '#94A3B8' }} />
                  <div className="arch-eyebrow">Hidden low-confidence</div>
                  <span style={{ fontSize: 11, color: 'var(--cc-text-faint)' }}>{hiddenLowIssues.length}</span>
                </div>
                <div className="arch-empty" style={{ maxWidth: 680, textAlign: 'left' }}>
                  Symbol-level mutual references and type-only cycles are hidden by default. Enable low-confidence issues if you are debugging a specific symbol pair.
                </div>
              </section>
            )
          }

          const items = visibleIssues.filter(issue => issue.severity === section)
          if (!items.length) return null
          const config = SEVERITY[section]
          const Icon = config.icon
          return (
            <section key={section}>
              <div className="flex items-center gap-2 mb-2">
                <Icon size={14} style={{ color: config.color }} />
                <div className="arch-eyebrow">{config.label}</div>
                <span style={{ fontSize: 11, color: 'var(--cc-text-faint)' }}>{items.length}</span>
              </div>
              <div className="grid gap-2" style={{ gridTemplateColumns: 'repeat(auto-fit, minmax(320px, 1fr))' }}>
                {items.map(issue => (
                  <HotspotCard
                    key={issue.id}
                    issue={issue}
                    node={issue.nodeId ? byId.get(issue.nodeId) : undefined}
                    severityColor={config.color}
                    onSelectNode={onSelectNode}
                    onOpenNode={onOpenNode}
                    onOpenNeighborhood={onOpenNeighborhood}
                    onOpenMatrix={onOpenMatrix}
                    onOpenApiDataFlow={onOpenApiDataFlow}
                    onOpenRawGraph={onOpenRawGraph}
                  />
                ))}
              </div>
            </section>
          )
        })}
      </div>
    </div>
  )
}

function HotspotCard({
  issue,
  node,
  severityColor,
  onSelectNode,
  onOpenNode,
  onOpenNeighborhood,
  onOpenMatrix,
  onOpenApiDataFlow,
  onOpenRawGraph,
}: {
  issue: HotspotIssue
  node?: GraphNode
  severityColor: string
  onSelectNode: (id: string | null) => void
  onOpenNode: (node: GraphNode) => void
  onOpenNeighborhood: () => void
  onOpenMatrix?: () => void
  onOpenApiDataFlow?: () => void
  onOpenRawGraph?: () => void
}) {
  const confidence = CONFIDENCE[issue.confidence ?? 'medium']
  const locationText = formatLocations(issue)

  return (
    <article className="arch-card">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div style={{ fontSize: 13, color: 'var(--cc-text)', fontWeight: 760, lineHeight: 1.35 }}>{issue.title}</div>
          <div style={{ fontSize: 11, color: 'var(--cc-text-subtle)', marginTop: 4, lineHeight: 1.45 }}>{issue.description}</div>
        </div>
        <div className="flex flex-col gap-1 items-end shrink-0">
          <span className="arch-badge" style={{ borderColor: `${severityColor}66`, color: severityColor }}>{issue.kind}</span>
          <span className="arch-badge" style={{ borderColor: `${confidence.color}66`, color: confidence.color }}>{confidence.label}</span>
        </div>
      </div>
      {locationText && (
        <div style={{ fontSize: 11, color: 'var(--cc-text-faint)', marginTop: 10, lineHeight: 1.45, overflowWrap: 'anywhere' }}>
          {locationText}
        </div>
      )}
      {issue.suggestion && <div style={{ fontSize: 11, color: 'var(--cc-text-muted)', marginTop: 10, lineHeight: 1.45 }}>{issue.suggestion}</div>}
      <div className="flex items-center gap-2 mt-3">
        <span style={{ fontSize: 11, color: 'var(--cc-text-faint)' }}>{issue.connections} relationships</span>
        <div className="ml-auto flex flex-wrap justify-end gap-1">
          <button className="arch-action" disabled={!node} onClick={() => { if (node) { onSelectNode(node.id); onOpenNeighborhood() } }} title="Open local neighborhood"><Network size={13} /></button>
          <button className="arch-action" disabled={issue.kind !== 'cycle-candidate' && issue.kind !== 'boundary-violation'} onClick={onOpenMatrix} title="Open dependency matrix cell"><Table2 size={13} /></button>
          <button className="arch-action" disabled={!['unused-endpoint', 'frontend-call-without-handler', 'central-api-client'].includes(issue.kind)} onClick={onOpenApiDataFlow} title="Open API/Data Flow"><Code2 size={13} /></button>
          <button className="arch-action" onClick={onOpenRawGraph} title="Open raw graph"><Network size={13} /></button>
          <button className="arch-action" disabled={!node?.file} onClick={() => node && onOpenNode(node)} title="Open file"><ExternalLink size={13} /></button>
          <button className="arch-action" disabled title="Ignore">Ignore</button>
          <button className="arch-action" disabled title="Hide as noise"><EyeOff size={13} /></button>
        </div>
      </div>
    </article>
  )
}

function formatLocations(issue: HotspotIssue) {
  const files = (issue.files ?? []).slice(0, 3)
  const modules = (issue.modules ?? []).slice(0, 3)
  const parts = []
  if (files.length) parts.push(`${files.length} file${files.length === 1 ? '' : 's'}: ${files.join(', ')}`)
  if (modules.length) parts.push(`${modules.length} module${modules.length === 1 ? '' : 's'}: ${modules.join(', ')}`)
  return parts.join(' · ')
}

function EmptyMessage({ children }: { children: string }) {
  return <div className="arch-empty">{children}</div>
}
