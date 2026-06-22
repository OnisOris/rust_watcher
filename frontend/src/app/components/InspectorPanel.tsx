import type { ReactNode } from 'react'
import { ExternalLink, Pin, Layers } from 'lucide-react'
import type { AnalyzerServiceStatus, AnalyzerStatus, AppState, AppStatus, GraphEdge, GraphNode, TraceExplanation } from '../types'
import { analyzerStatusColor, sortAnalyzers, summarizeAnalyzers } from '../api/analyzerStatus'
import { inferNodeLanguage, languageColor, languageDisplay, languageIcon } from '../api/language'

interface InspectorPanelProps {
  selectedNode: GraphNode | null
  nodes: GraphNode[]
  edges: GraphEdge[]
  projectName?: string | null
  analyzerStatus?: AnalyzerStatus
  analyzers?: AnalyzerServiceStatus[]
  pythonAnalyzer?: AppStatus['pythonAnalyzer']
  appState?: AppState
  filesCount?: number
  totalNodes?: number
  totalEdges?: number
  visibleNodes?: number
  visibleEdges?: number
  message?: string | null
  onTogglePin: (id: string) => void
  onToggleCollapse: (id: string) => void
  collapsedGroups: Set<string>
  onSelectNode: (id: string) => void
  onOpenInEditor: (node: GraphNode) => void
  onTraceLoaded?: (trace: TraceExplanation) => void
  onClearTraceHighlight?: () => void
}

export function InspectorPanel(props: InspectorPanelProps) {
  return (
    <div className="flex flex-col overflow-hidden" style={{ width: 340, background: 'var(--cc-panel)', borderLeft: '1px solid var(--cc-border)', fontFamily: 'Inter, sans-serif' }}>
      {props.selectedNode ? <NodePanel {...props} node={props.selectedNode} /> : <OverviewPanel {...props} />}
    </div>
  )
}

function OverviewPanel(props: InspectorPanelProps) {
  const summary = summarizeAnalyzers(props.analyzers)
  const languages = languageCounts(props.nodes)
  const hiddenNodes = Math.max(0, (props.totalNodes ?? props.nodes.length) - (props.visibleNodes ?? props.nodes.length))
  const hiddenEdges = Math.max(0, (props.totalEdges ?? props.edges.length) - (props.visibleEdges ?? props.edges.length))
  const topNodes = props.nodes
    .map(node => ({ node, degree: props.edges.filter(edge => edge.source === node.id || edge.target === node.id).length }))
    .sort((a, b) => b.degree - a.degree)
    .slice(0, 6)

  return (
    <>
      <Header title="Project Overview" subtitle={props.projectName ?? 'workspace'} />
      <div className="overflow-y-auto flex-1 p-3 space-y-4" style={{ scrollbarWidth: 'thin' }}>
        <Card>
          <SectionTitle>Graph scope</SectionTitle>
          <div className="grid grid-cols-2 gap-2">
            <Stat label="Visible nodes" value={props.visibleNodes ?? props.nodes.length} color="var(--cc-accent)" />
            <Stat label="Visible edges" value={props.visibleEdges ?? props.edges.length} color="var(--cc-crate)" />
            <Stat label="Total nodes" value={props.totalNodes ?? props.nodes.length} color="#64748B" />
            <Stat label="Total edges" value={props.totalEdges ?? props.edges.length} color="#64748B" />
          </div>
          {(hiddenNodes > 0 || hiddenEdges > 0) && <Text>Hidden by filters: {hiddenNodes} nodes - {hiddenEdges} edges</Text>}
        </Card>

        <Card>
          <SectionTitle>Languages</SectionTitle>
          <div className="grid grid-cols-5 gap-1.5">
            {Object.entries(languages).map(([label, value]) => <SmallStat key={label} label={label} value={value} />)}
          </div>
        </Card>

        <Card>
          <SectionTitle>Analyzers</SectionTitle>
          <Text>{summary.ready} ready - {summary.fallback} fallback - {summary.error} error</Text>
          <Text>{props.message ?? props.appState ?? 'Ready'} - {props.filesCount ?? 0} files indexed</Text>
          <div className="space-y-1.5 mt-2">
            {sortAnalyzers(props.analyzers ?? []).slice(0, 5).map(analyzer => (
              <div key={analyzer.id} className="rounded-lg px-2 py-1.5" style={{ background: 'var(--cc-surface)', border: '1px solid var(--cc-border)' }}>
                <div className="flex items-center gap-1.5">
                  <span style={{ width: 6, height: 6, borderRadius: 999, background: analyzerStatusColor(analyzer.status) }} />
                  <span style={{ fontSize: 10, color: 'var(--cc-text)', fontWeight: 700 }}>{analyzer.label}</span>
                  <span style={{ marginLeft: 'auto', fontSize: 9, color: analyzerStatusColor(analyzer.status) }}>{analyzer.status}</span>
                </div>
              </div>
            ))}
          </div>
        </Card>

        <Card>
          <SectionTitle>Most connected</SectionTitle>
          <div className="space-y-1.5">
            {topNodes.map(({ node, degree }) => <NodeRow key={node.id} node={node} right={String(degree)} onClick={() => props.onSelectNode(node.id)} />)}
          </div>
        </Card>
      </div>
    </>
  )
}

function NodePanel(props: InspectorPanelProps & { node: GraphNode }) {
  const node = props.node
  const language = inferNodeLanguage(node)
  const incoming = props.edges.filter(edge => edge.target === node.id)
  const outgoing = props.edges.filter(edge => edge.source === node.id)
  const related = [...incoming.map(edge => edge.source), ...outgoing.map(edge => edge.target)]
    .map(id => props.nodes.find(item => item.id === id))
    .filter((item): item is GraphNode => Boolean(item))
    .slice(0, 8)
  const collapsed = props.collapsedGroups.has(node.id)
  const collapsible = node.type === 'File' || node.type === 'Module' || node.type === 'Object'

  return (
    <>
      <Header title={node.label} subtitle={node.file ?? node.module ?? node.crate ?? node.type} />
      <div className="overflow-y-auto flex-1 p-3 space-y-4" style={{ scrollbarWidth: 'thin' }}>
        <Card>
          <div className="flex flex-wrap gap-1.5">
            <Badge color="#64748B">{node.type}</Badge>
            <Badge color={languageColor(language)}>{languageIcon(language)} {languageDisplay(language)}</Badge>
            {node.visibility && <Badge color="#64748B">{node.visibility}</Badge>}
            {node.reachability === 'Detached' && <Badge color="#94A3B8">Detached</Badge>}
          </div>
          {node.signature && <pre className="mt-3 rounded-lg p-2" style={{ background: 'var(--cc-surface)', border: '1px solid var(--cc-border)', color: 'var(--cc-text-muted)', fontSize: 10, overflow: 'auto', whiteSpace: 'pre-wrap' }}>{node.signature}</pre>}
          {node.description && <Text>{node.description}</Text>}
        </Card>

        <Card>
          <SectionTitle>Location</SectionTitle>
          <Info label="File" value={node.file ?? 'unknown'} />
          <Info label="Module" value={node.module ?? 'unknown'} />
          <Info label="Crate" value={node.crate ?? 'unknown'} />
          <Info label="Line" value={node.line ? String(node.line) : 'unknown'} />
        </Card>

        <Card>
          <SectionTitle>Relations</SectionTitle>
          <div className="grid grid-cols-2 gap-2 mb-3">
            <SmallStat label="Incoming" value={incoming.length} />
            <SmallStat label="Outgoing" value={outgoing.length} />
          </div>
          <div className="space-y-1.5">
            {related.map(item => <NodeRow key={item.id} node={item} onClick={() => props.onSelectNode(item.id)} />)}
            {!related.length && <Text>No direct relations in current view.</Text>}
          </div>
        </Card>

        <Card>
          <SectionTitle>Actions</SectionTitle>
          <div className="grid grid-cols-2 gap-2">
            <Action icon={<Pin size={13} />} label={node.pinned ? 'Unpin Node' : 'Pin Node'} onClick={() => props.onTogglePin(node.id)} active={!!node.pinned} />
            <Action icon={<Layers size={13} />} label={collapsed ? 'Expand Group' : 'Collapse Group'} onClick={() => props.onToggleCollapse(node.id)} active={collapsed} disabled={!collapsible} />
            <Action icon={<ExternalLink size={13} />} label="Open in Editor" onClick={() => props.onOpenInEditor(node)} disabled={!node.file} />
          </div>
        </Card>
      </div>
    </>
  )
}

function languageCounts(nodes: GraphNode[]) {
  return {
    Rust: nodes.filter(node => inferNodeLanguage(node) === 'rust').length,
    TS: nodes.filter(node => inferNodeLanguage(node) === 'typescript' || inferNodeLanguage(node) === 'javascript').length,
    Python: nodes.filter(node => inferNodeLanguage(node) === 'python').length,
    QML: nodes.filter(node => inferNodeLanguage(node) === 'qml').length,
    API: nodes.filter(node => inferNodeLanguage(node) === 'endpoints').length,
  }
}

function Header({ title, subtitle }: { title: string; subtitle?: string | null }) {
  return <div className="px-4 py-3" style={{ borderBottom: '1px solid var(--cc-border)' }}><div style={{ fontSize: 11, color: 'var(--cc-text-faint)', fontWeight: 750, letterSpacing: '0.08em', textTransform: 'uppercase' }}>{title}</div>{subtitle && <div className="mt-1 truncate" style={{ fontSize: 10, color: 'var(--cc-text-subtle)' }}>{subtitle}</div>}</div>
}

function Card({ children }: { children: ReactNode }) {
  return <div className="rounded-xl p-3" style={{ background: 'var(--cc-card)', border: '1px solid var(--cc-border)' }}>{children}</div>
}

function SectionTitle({ children }: { children: ReactNode }) {
  return <p style={{ fontSize: 10, color: 'var(--cc-text-faint)', fontWeight: 700, letterSpacing: '0.07em', textTransform: 'uppercase', marginBottom: 8 }}>{children}</p>
}

function Text({ children }: { children: ReactNode }) {
  return <div style={{ marginTop: 8, fontSize: 10, color: 'var(--cc-text-subtle)', lineHeight: 1.45 }}>{children}</div>
}

function Stat({ label, value, color }: { label: string; value: number; color: string }) {
  return <div className="rounded-lg p-2 text-center" style={{ background: 'var(--cc-surface)', border: '1px solid var(--cc-border)' }}><div style={{ fontSize: 22, lineHeight: 1, fontWeight: 800, color }}>{value}</div><div style={{ fontSize: 9, color: 'var(--cc-text-subtle)', marginTop: 4 }}>{label}</div></div>
}

function SmallStat({ label, value }: { label: string; value: number }) {
  return <div className="rounded-md px-1.5 py-1.5 text-center" style={{ background: 'var(--cc-surface)', border: '1px solid var(--cc-border)' }}><div style={{ fontSize: 13, color: 'var(--cc-text)', fontWeight: 800 }}>{value}</div><div style={{ fontSize: 8, color: 'var(--cc-text-subtle)' }}>{label}</div></div>
}

function Badge({ children, color }: { children: ReactNode; color: string }) {
  return <span className="rounded-full px-2 py-0.5" style={{ color, background: `${color}18`, border: `1px solid ${color}44`, fontSize: 10, fontWeight: 700 }}>{children}</span>
}

function Info({ label, value }: { label: string; value: string }) {
  return <div className="flex items-start gap-2 py-1"><span style={{ width: 54, fontSize: 10, color: 'var(--cc-text-faint)' }}>{label}</span><span className="min-w-0 flex-1 break-all" style={{ fontSize: 10, color: 'var(--cc-text-muted)' }}>{value}</span></div>
}

function NodeRow({ node, right, onClick }: { node: GraphNode; right?: string; onClick?: () => void }) {
  const language = inferNodeLanguage(node)
  const color = languageColor(language)
  return <button onClick={onClick} className="w-full rounded-lg px-2 py-2 text-left" style={{ background: 'var(--cc-surface)', border: '1px solid var(--cc-border)', cursor: onClick ? 'pointer' : 'default' }}><div className="flex items-center gap-2"><span style={{ width: 24, height: 16, borderRadius: 999, border: `1px solid ${color}`, color, fontSize: 8, fontWeight: 800, display: 'inline-flex', alignItems: 'center', justifyContent: 'center' }}>{languageIcon(language)}</span><span className="truncate" style={{ fontSize: 10, color: 'var(--cc-text)', fontWeight: 700, flex: 1 }}>{node.label}</span>{right && <span style={{ fontSize: 9, color: 'var(--cc-text-faint)' }}>{right}</span>}</div><div className="truncate" style={{ fontSize: 9, color: 'var(--cc-text-subtle)', marginTop: 3 }}>{node.file ?? node.module ?? node.type}</div></button>
}

function Action({ icon, label, onClick, active, disabled }: { icon: ReactNode; label: string; onClick: () => void; active?: boolean; disabled?: boolean }) {
  return <button onClick={onClick} disabled={disabled} className="rounded-lg px-2 py-2 flex items-center gap-1.5 justify-center" style={{ background: active ? 'var(--cc-selected-soft)' : 'var(--cc-surface)', border: active ? '1px solid rgba(14,165,233,0.35)' : '1px solid var(--cc-border)', color: disabled ? 'var(--cc-text-faint)' : active ? 'var(--cc-accent)' : 'var(--cc-text-muted)', cursor: disabled ? 'not-allowed' : 'pointer', fontSize: 10, fontWeight: 650 }}>{icon}{label}</button>
}
