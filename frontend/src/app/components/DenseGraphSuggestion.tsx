import type { ReactNode } from 'react'
import { AlertCircle, X, Minimize2, EyeOff, Layers, Focus } from 'lucide-react'

interface DenseGraphSuggestionProps {
  onDismiss: () => void
  onCollapseModules: () => void
  onHideExternal: () => void
  onHideTests: () => void
  onDepth2: () => void
  onFocusBubble: () => void
}

export function DenseGraphSuggestion({ onDismiss, onCollapseModules, onHideExternal, onHideTests, onDepth2, onFocusBubble }: DenseGraphSuggestionProps) {
  return (
    <div
      className="absolute bottom-24 right-5 z-20 rounded-xl shadow-2xl"
      style={{
        width: 280,
        background: 'var(--cc-panel)',
        border: '1px solid var(--cc-border-strong)',
        boxShadow: 'var(--cc-shadow)',
        fontFamily: 'Inter, sans-serif',
      }}
    >
      {/* header */}
      <div className="flex items-center gap-2 px-3 py-3" style={{ borderBottom: '1px solid var(--cc-border)' }}>
        <AlertCircle size={14} color="#F59E0B" />
        <span style={{ fontSize: 12, color: '#F59E0B', fontWeight: 600, flex: 1 }}>Graph is dense.</span>
        <button onClick={onDismiss} style={{ background: 'none', border: 'none', cursor: 'pointer', color: 'var(--cc-text-subtle)', display: 'flex' }}>
          <X size={13} />
        </button>
      </div>

      <div className="px-3 py-2">
        <p style={{ fontSize: 11, color: 'var(--cc-text-muted)', marginBottom: 10 }}>Reduce visual noise?</p>

        <div className="flex flex-col gap-1.5">
          <SuggestionAction icon={<Minimize2 size={12} />} label="Collapse modules" onClick={onCollapseModules} />
          <SuggestionAction icon={<EyeOff size={12} />} label="Hide external crates" onClick={onHideExternal} />
          <SuggestionAction icon={<EyeOff size={12} />} label="Hide tests" onClick={onHideTests} />
          <SuggestionAction icon={<Layers size={12} />} label="Show only Depth 2" onClick={onDepth2} />
          <SuggestionAction
            icon={<Focus size={12} />}
            label="Switch to Focus Bubble"
            onClick={onFocusBubble}
            accent
          />
        </div>
      </div>
    </div>
  )
}

function SuggestionAction({ icon, label, onClick, accent }: { icon: ReactNode; label: string; onClick: () => void; accent?: boolean }) {
  return (
    <button
      onClick={onClick}
      className="flex items-center gap-2 rounded-lg px-3 py-2 w-full transition-all text-left"
      style={{
        background: accent ? 'rgba(6,182,212,0.08)' : 'var(--cc-surface)',
        border: accent ? '1px solid rgba(6,182,212,0.2)' : '1px solid var(--cc-border)',
        color: accent ? '#06B6D4' : 'var(--cc-text-muted)',
        cursor: 'pointer',
        fontSize: 12,
      }}
    >
      {icon}
      {label}
    </button>
  )
}
