import type { ReactNode } from 'react'
import {
  Search, RefreshCw, Minimize2, Download, Settings,
  Clock, Zap, Sun, Moon, SlidersHorizontal
} from 'lucide-react'
import type { AnalyzerStatus, GraphMode, AppState, ThemeMode } from '../types'

interface TopToolbarProps {
  appState: AppState
  analyzerStatus: AnalyzerStatus
  message?: string | null
  mode: GraphMode
  onModeChange: (mode: GraphMode) => void
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

const MODES: { key: GraphMode; label: string }[] = [
  { key: 'Macro', label: 'Macro' },
  { key: 'Meso', label: 'Meso' },
  { key: 'Micro', label: 'Micro' },
  { key: 'CallFlow', label: 'Call Flow' },
  { key: 'DataFlow', label: 'Data Flow' },
  { key: 'Traits', label: 'Traits & Impl' },
]

const STATUS_CONFIG: Record<AnalyzerStatus | AppState, { label: string; color: string; dot: string; pulse: boolean }> = {
  Ready: { label: 'Ready', color: '#34D399', dot: 'bg-emerald-400', pulse: false },
  Indexing: { label: 'Indexing…', color: '#F59E0B', dot: 'bg-amber-400', pulse: true },
  Starting: { label: 'Starting…', color: '#F59E0B', dot: 'bg-amber-400', pulse: true },
  Fallback: { label: 'Syntax fallback', color: '#F59E0B', dot: 'bg-amber-400', pulse: false },
  Stale: { label: 'Stale', color: '#F59E0B', dot: 'bg-amber-400', pulse: true },
  Error: { label: 'Error', color: '#F87171', dot: 'bg-red-400', pulse: false },
  normal: { label: 'Ready', color: '#34D399', dot: 'bg-emerald-400', pulse: false },
  indexing: { label: 'Indexing…', color: '#F59E0B', dot: 'bg-amber-400', pulse: true },
  empty: { label: 'No Project', color: 'var(--cc-muted)', dot: 'bg-gray-500', pulse: false },
  error: { label: 'Error', color: '#F87171', dot: 'bg-red-400', pulse: false },
}

export function TopToolbar({
  appState, analyzerStatus, message, mode, onModeChange, onSearchOpen, onSettingsOpen, onRecenter, onCollapse, onThemeToggle, onClarityToggle, clarityOpen, clarityActive, theme
}: TopToolbarProps) {
  const status = appState === 'empty' ? STATUS_CONFIG.empty : STATUS_CONFIG[analyzerStatus]

  return (
    <div
      className="flex items-center gap-3 px-4 shrink-0"
      style={{
        height: 48,
        background: 'var(--cc-panel)',
        borderBottom: '1px solid var(--cc-border)',
        fontFamily: 'Inter, sans-serif',
      }}
    >
      {/* Product name */}
      <div className="flex items-center gap-2 shrink-0">
        <div
          className="flex items-center justify-center rounded"
          style={{ width: 26, height: 26, background: 'linear-gradient(135deg, #06B6D4 0%, #7C3AED 100%)' }}
        >
          <Zap size={14} color="#fff" />
        </div>
        <span style={{ color: 'var(--cc-text)', fontSize: 13, fontWeight: 600, letterSpacing: '-0.01em' }}>
          Rust Code<span style={{ color: '#06B6D4' }}> Center</span>
        </span>
      </div>

      <div style={{ width: 1, height: 20, background: 'var(--cc-border)' }} />

      {/* rust-analyzer status */}
      <div className="flex items-center gap-2 shrink-0">
        <div className="relative">
          <div className={`w-2 h-2 rounded-full ${status.dot}`} />
          {status.pulse && (
            <div className={`absolute inset-0 w-2 h-2 rounded-full ${status.dot} animate-ping opacity-75`} />
          )}
        </div>
        <span style={{ color: status.color, fontSize: 11, fontWeight: 500 }}>
          rust-analyzer · {status.label}
        </span>
        {appState === 'normal' && analyzerStatus !== 'Fallback' && (
          <span style={{ color: 'var(--cc-text-subtle)', fontSize: 11 }}>
            <Clock size={10} className="inline mr-1" />
            Updated 2s ago
          </span>
        )}
        {analyzerStatus === 'Fallback' && (
          <span title={message ?? undefined} style={{ color: 'var(--cc-text-subtle)', fontSize: 11 }}>
            syntax graph only
          </span>
        )}
        {appState === 'indexing' && (
          <div className="flex items-center gap-1">
            <div className="h-1 rounded-full overflow-hidden" style={{ width: 48, background: 'var(--cc-border)' }}>
              <div className="h-full rounded-full animate-pulse" style={{ width: '45%', background: '#F59E0B' }} />
            </div>
            <span style={{ color: 'var(--cc-text-subtle)', fontSize: 10 }}>45%</span>
          </div>
        )}
      </div>

      <div style={{ width: 1, height: 20, background: 'var(--cc-border)' }} />

      {/* Graph mode switcher */}
      <div
        className="flex items-center gap-0.5 rounded-lg p-0.5 shrink-0"
        style={{ background: 'var(--cc-surface)', border: '1px solid var(--cc-border)' }}
      >
        {MODES.map(m => (
          <button
            key={m.key}
            onClick={() => onModeChange(m.key)}
            className="rounded transition-all"
            style={{
              padding: '3px 8px',
              fontSize: 11,
              lineHeight: 1,
              fontWeight: mode === m.key ? 600 : 400,
              color: mode === m.key ? 'var(--cc-text)' : 'var(--cc-text-subtle)',
              background: mode === m.key ? 'var(--cc-elevated)' : 'transparent',
              border: mode === m.key ? '1px solid var(--cc-border-strong)' : '1px solid transparent',
              cursor: 'pointer',
              transition: 'all 0.15s',
              whiteSpace: 'nowrap',
            }}
          >
            {m.label}
          </button>
        ))}
      </div>

      {/* Search */}
      <button
        onClick={onSearchOpen}
        className="flex items-center gap-2 rounded-lg transition-all"
        style={{
          flex: '1 1 280px',
          maxWidth: 360,
          minWidth: 220,
          height: 32,
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
        <Search size={13} style={{ flexShrink: 0 }} />
        <span style={{ minWidth: 0, flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', textAlign: 'left' }}>
          Search symbol, file, trait, function…
        </span>
        <span
          className="ml-auto rounded"
          style={{ padding: '1px 5px', background: 'var(--cc-elevated)', color: 'var(--cc-text-subtle)', fontSize: 10, border: '1px solid var(--cc-border)', flexShrink: 0 }}
        >
          ⌘K
        </span>
      </button>

      <button
        onClick={onClarityToggle}
        className="flex items-center gap-1.5 rounded-lg transition-all shrink-0"
        title="Graph clarity"
        style={{
          height: 32,
          padding: '6px 10px',
          background: clarityOpen ? 'rgba(6,182,212,0.14)' : clarityActive ? 'rgba(6,182,212,0.08)' : 'var(--cc-surface)',
          border: clarityOpen ? '1px solid rgba(6,182,212,0.38)' : '1px solid var(--cc-border)',
          color: clarityOpen || clarityActive ? '#06B6D4' : 'var(--cc-text-subtle)',
          cursor: 'pointer',
          fontSize: 11,
          fontWeight: clarityOpen ? 650 : 550,
          whiteSpace: 'nowrap',
        }}
      >
        <SlidersHorizontal size={13} />
        <span>Clarity</span>
        {clarityActive && (
          <span style={{ width: 6, height: 6, borderRadius: '50%', background: '#06B6D4' }} />
        )}
      </button>

      <div className="flex-1" />

      {/* Action buttons */}
      <div className="flex items-center gap-1 shrink-0">
        <ToolbarButton icon={<RefreshCw size={14} />} label="Recenter" onClick={onRecenter} />
        <ToolbarButton icon={<Minimize2 size={14} />} label="Collapse" onClick={onCollapse} />
        <ToolbarButton icon={<Download size={14} />} label="Export" onClick={() => {}} />
        <div style={{ width: 1, height: 16, background: 'var(--cc-border)', margin: '0 2px' }} />
        <ToolbarButton
          icon={theme === 'light' ? <Moon size={14} /> : <Sun size={14} />}
          label={theme === 'light' ? 'Dark Theme' : 'Light Theme'}
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
      className="flex items-center justify-center rounded transition-all"
      style={{
        width: 30,
        height: 30,
        color: active ? (accentColor ?? '#06B6D4') : 'var(--cc-muted)',
        background: active ? 'rgba(6,182,212,0.1)' : 'transparent',
        border: active ? `1px solid ${accentColor ?? '#06B6D4'}40` : '1px solid transparent',
        cursor: 'pointer',
      }}
    >
      {icon}
    </button>
  )
}
