import type { ReactNode } from 'react'
import {
  Search,
  RefreshCw,
  Minimize2,
  Download,
  Settings,
  Zap,
  Sun,
  Moon,
  SlidersHorizontal,
  Wifi,
} from 'lucide-react'
import type { AnalyzerServiceStatus, AnalyzerStatus, GraphMode, AppState, ThemeMode, GraphLayoutMode } from '../types'
import { AnalyzerStatusSummary } from './AnalyzerStatusSummary'

interface TopToolbarProps {
  appState: AppState
  analyzerStatus: AnalyzerStatus
  analyzers?: AnalyzerServiceStatus[]
  message?: string | null
  projectName?: string | null
  lastUpdated?: string | null
  filesCount?: number
  mode: GraphMode
  onModeChange: (mode: GraphMode) => void
  layoutMode: GraphLayoutMode
  onLayoutModeChange: (mode: GraphLayoutMode) => void
  onSearchOpen: () => void
  onSettingsOpen: () => void
  onRecenter: () => void
  onCollapse: () => void
  onThemeToggle: () => void
  onClarityToggle: () => void
  clarityOpen: boolean
  clarityActive: boolean
  theme: ThemeMode
}

const MODES: { key: GraphMode; label: string; hint: string }[] = [
  { key: 'Macro', label: 'Macro', hint: 'Project scopes, files and external dependencies' },
  { key: 'Meso', label: 'Meso', hint: 'Important symbols with file context' },
  { key: 'Micro', label: 'Micro', hint: 'Detailed symbol-level relations' },
  { key: 'CallFlow', label: 'Call Flow', hint: 'Endpoint-to-handler and function chains' },
  { key: 'DataFlow', label: 'Data Flow', hint: 'Request, DTO and response type flow' },
  { key: 'Traits', label: 'Types & Impl', hint: 'Traits, impls, classes and type relationships' },
]

const LAYOUT_MODES: { key: GraphLayoutMode; label: string; hint: string }[] = [
  { key: 'Force', label: 'Force graph', hint: 'Classic force-directed graph' },
  { key: 'SemanticZones', label: 'Semantic zones', hint: 'Language zones, API boundary and package panels' },
  { key: 'PackageMap', label: 'Package map', hint: 'Package-oriented semantic zone map' },
  { key: 'Neighborhood', label: 'Local neighborhood', hint: 'Semantic placement for focused local context' },
]

const STATUS_CONFIG: Record<AnalyzerStatus | AppState, { label: string; color: string; dot: string; pulse: boolean }> = {
  Ready: { label: 'Ready', color: '#10B981', dot: '#10B981', pulse: false },
  Indexing: { label: 'Indexing', color: '#D97706', dot: '#D97706', pulse: true },
  Starting: { label: 'Starting', color: '#D97706', dot: '#D97706', pulse: true },
  Fallback: { label: 'Syntax fallback', color: '#D97706', dot: '#D97706', pulse: false },
  Stale: { label: 'Stale', color: '#D97706', dot: '#D97706', pulse: true },
  Error: { label: 'Error', color: '#DC2626', dot: '#DC2626', pulse: false },
  normal: { label: 'Ready', color: '#10B981', dot: '#10B981', pulse: false },
  indexing: { label: 'Indexing', color: '#D97706', dot: '#D97706', pulse: true },
  empty: { label: 'No project', color: 'var(--cc-muted)', dot: 'var(--cc-muted)', pulse: false },
  error: { label: 'Error', color: '#DC2626', dot: '#DC2626', pulse: false },
}

export function TopToolbar({
  appState,
  analyzerStatus,
  analyzers,
  message,
  projectName,
  lastUpdated,
  filesCount = 0,
  mode,
  onModeChange,
  layoutMode,
  onLayoutModeChange,
  onSearchOpen,
  onSettingsOpen,
  onRecenter,
  onCollapse,
  onThemeToggle,
  onClarityToggle,
  clarityOpen,
  clarityActive,
  theme,
}: TopToolbarProps) {
  const status = appState === 'empty' ? STATUS_CONFIG.empty : STATUS_CONFIG[analyzerStatus]
  const modeHint = MODES.find(item => item.key === mode)?.hint

  return (
    <div
      className="flex items-center gap-3 px-4 shrink-0"
      style={{
        height: 54,
        background: 'var(--cc-panel)',
        borderBottom: '1px solid var(--cc-border)',
        fontFamily: 'Inter, sans-serif',
      }}
    >
      <div className="flex items-center gap-2 shrink-0" title="Rust Code Center">
        <div
          className="flex items-center justify-center rounded-lg"
          style={{ width: 28, height: 28, background: 'linear-gradient(135deg, var(--cc-accent) 0%, var(--cc-crate) 100%)', boxShadow: '0 8px 24px rgba(14,165,233,0.22)' }}
        >
          <Zap size={15} color="#fff" />
        </div>
        <div style={{ minWidth: 0 }}>
          <div style={{ color: 'var(--cc-text)', fontSize: 13, fontWeight: 750, letterSpacing: '-0.02em' }}>
            Rust Code<span style={{ color: 'var(--cc-accent)' }}> Center</span>
          </div>
          <div style={{ color: 'var(--cc-text-faint)', fontSize: 10, marginTop: -1, fontFamily: 'JetBrains Mono, monospace' }}>
            {projectName ?? 'workspace'}
          </div>
        </div>
      </div>

      <div className="hidden xl:block" style={{ width: 1, height: 24, background: 'var(--cc-border)' }} />

      <AnalyzerStatusSummary
        analyzers={analyzers}
        overallStatus={analyzerStatus}
        message={message}
        lastUpdated={lastUpdated}
        filesCount={filesCount}
      />

      <div className="flex items-center gap-0.5 rounded-xl p-1 shrink-0" style={{ background: 'var(--cc-surface)', border: '1px solid var(--cc-border)' }}>
        {MODES.map(item => (
          <button
            key={item.key}
            onClick={() => onModeChange(item.key)}
            title={item.hint}
            className="rounded-lg transition-all"
            style={{
              padding: '5px 9px',
              fontSize: 11,
              lineHeight: 1,
              fontWeight: mode === item.key ? 750 : 600,
              color: mode === item.key ? 'var(--cc-accent)' : 'var(--cc-text-subtle)',
              background: mode === item.key ? 'var(--cc-selected-soft)' : 'transparent',
              border: mode === item.key ? '1px solid rgba(14,165,233,0.35)' : '1px solid transparent',
              cursor: 'pointer',
              whiteSpace: 'nowrap',
            }}
          >
            {item.label}
          </button>
        ))}
      </div>

      <div className="flex items-center gap-0.5 rounded-xl p-1 shrink-0" style={{ background: 'var(--cc-surface)', border: '1px solid var(--cc-border)' }}>
        {LAYOUT_MODES.map(item => (
          <button
            key={item.key}
            onClick={() => onLayoutModeChange(item.key)}
            title={item.hint}
            className="rounded-lg transition-all"
            style={{
              padding: '5px 9px',
              fontSize: 11,
              lineHeight: 1,
              fontWeight: layoutMode === item.key ? 750 : 600,
              color: layoutMode === item.key ? 'var(--cc-accent)' : 'var(--cc-text-subtle)',
              background: layoutMode === item.key ? 'var(--cc-selected-soft)' : 'transparent',
              border: layoutMode === item.key ? '1px solid rgba(14,165,233,0.35)' : '1px solid transparent',
              cursor: 'pointer',
              whiteSpace: 'nowrap',
            }}
          >
            {item.label}
          </button>
        ))}
      </div>

      <button
        onClick={onSearchOpen}
        className="flex items-center gap-2 rounded-xl transition-all"
        style={{
          flex: '1 1 280px',
          maxWidth: 420,
          minWidth: 220,
          height: 34,
          padding: '6px 12px',
          background: 'var(--cc-surface)',
          border: '1px solid var(--cc-border)',
          cursor: 'text',
          color: 'var(--cc-text-subtle)',
          fontSize: 12,
          lineHeight: 1,
          overflow: 'hidden',
        }}
      >
        <Search size={14} style={{ flexShrink: 0 }} />
        <span style={{ minWidth: 0, flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', textAlign: 'left' }}>
          Search symbol, file, endpoint, type, function...
        </span>
        <span className="ml-auto rounded" style={{ padding: '1px 6px', background: 'var(--cc-elevated)', color: 'var(--cc-text-subtle)', fontSize: 10, border: '1px solid var(--cc-border)', flexShrink: 0 }}>
          ⌘K
        </span>
      </button>

      <button
        onClick={onClarityToggle}
        className="flex items-center gap-1.5 rounded-xl transition-all shrink-0"
        title={`Graph clarity${modeHint ? ` · ${modeHint}` : ''}`}
        style={{
          height: 34,
          padding: '6px 11px',
          background: clarityOpen ? 'var(--cc-selected-soft)' : clarityActive ? 'rgba(14,165,233,0.08)' : 'var(--cc-surface)',
          border: clarityOpen || clarityActive ? '1px solid rgba(14,165,233,0.35)' : '1px solid var(--cc-border)',
          color: clarityOpen || clarityActive ? 'var(--cc-accent)' : 'var(--cc-text-subtle)',
          cursor: 'pointer',
          fontSize: 11,
          fontWeight: clarityOpen ? 750 : 650,
          whiteSpace: 'nowrap',
        }}
      >
        <SlidersHorizontal size={13} />
        <span>Clarity</span>
        {clarityActive && <span style={{ width: 6, height: 6, borderRadius: '50%', background: 'var(--cc-accent)' }} />}
      </button>

      <div className="flex items-center gap-1 shrink-0">
        <ToolbarButton icon={<RefreshCw size={14} />} label="Refresh / recenter graph" onClick={onRecenter} />
        <ToolbarButton icon={<Minimize2 size={14} />} label="Architecture focus" onClick={onCollapse} />
        <ToolbarButton icon={<Download size={14} />} label="Export graph" onClick={() => {}} />
        <ToolbarButton icon={<Wifi size={14} />} label="Live connection" onClick={() => {}} active={appState === 'normal'} accentColor={status.color} />
        <div style={{ width: 1, height: 18, background: 'var(--cc-border)', margin: '0 2px' }} />
        <ToolbarButton
          icon={theme === 'light' ? <Moon size={14} /> : <Sun size={14} />}
          label={theme === 'light' ? 'Dark theme' : 'Light theme'}
          onClick={onThemeToggle}
        />
        <ToolbarButton icon={<Settings size={14} />} label="Settings" onClick={onSettingsOpen} />
      </div>
    </div>
  )
}

function ToolbarButton({ icon, label, onClick, active, accentColor }: {
  icon: ReactNode
  label: string
  onClick: () => void
  active?: boolean
  accentColor?: string
}) {
  return (
    <button
      title={label}
      onClick={onClick}
      className="flex items-center justify-center rounded-lg transition-all"
      style={{
        width: 32,
        height: 32,
        color: active ? (accentColor ?? 'var(--cc-accent)') : 'var(--cc-muted)',
        background: active ? `${accentColor ?? '#0EA5E9'}18` : 'transparent',
        border: active ? `1px solid ${accentColor ?? '#0EA5E9'}38` : '1px solid transparent',
        cursor: 'pointer',
      }}
    >
      {icon}
    </button>
  )
}
