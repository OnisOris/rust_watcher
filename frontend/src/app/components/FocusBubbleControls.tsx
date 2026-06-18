import type { ReactNode } from 'react'
import { X, ChevronUp, ChevronDown, EyeOff, Eye, Pin, GitBranch, Layers } from 'lucide-react'

interface FocusBubbleControlsProps {
  nodeLabel: string
  onClose: () => void
  onExpandDepth: () => void
  onCollapseNoise: () => void
  onShowCallers: () => void
  onShowCallees: () => void
  onShowDataFlow: () => void
}

export function FocusBubbleControls({ nodeLabel, onClose, onExpandDepth, onCollapseNoise, onShowCallers, onShowCallees, onShowDataFlow }: FocusBubbleControlsProps) {
  return (
    <div
      className="absolute top-14 left-1/2 -translate-x-1/2 z-20 rounded-xl shadow-2xl"
      style={{
        background: 'var(--cc-panel)',
        border: '1px solid #06B6D460',
        boxShadow: '0 8px 32px rgba(6,182,212,0.15), 0 2px 8px rgba(22,35,54,0.12)',
        fontFamily: 'Inter, sans-serif',
        minWidth: 400,
      }}
    >
      <div className="flex items-center gap-3 px-3 py-2.5" style={{ borderBottom: '1px solid var(--cc-border)' }}>
        <div className="w-2 h-2 rounded-full" style={{ background: '#06B6D4' }} />
        <span style={{ fontSize: 12, color: 'var(--cc-text)', fontWeight: 500 }}>
          Focus Bubble
        </span>
        <span style={{ fontSize: 12, color: '#06B6D4', fontFamily: 'JetBrains Mono, monospace' }}>{nodeLabel}</span>
        <div className="flex-1" />
        <button onClick={onClose} style={{ background: 'none', border: 'none', cursor: 'pointer', color: 'var(--cc-text-subtle)', display: 'flex' }}>
          <X size={13} />
        </button>
      </div>

      <div className="flex items-center gap-1.5 px-3 py-2">
        <BubbleBtn icon={<ChevronUp size={12} />} label="Expand Depth" onClick={onExpandDepth} />
        <BubbleBtn icon={<ChevronDown size={12} />} label="Collapse Noise" onClick={onCollapseNoise} />
        <BubbleBtn icon={<GitBranch size={12} />} label="Callers" onClick={onShowCallers} />
        <BubbleBtn icon={<GitBranch size={12} />} label="Callees" onClick={onShowCallees} />
        <BubbleBtn icon={<Layers size={12} />} label="Data Flow" onClick={onShowDataFlow} />
        <BubbleBtn icon={<EyeOff size={12} />} label="Hide External" onClick={() => {}} />
        <BubbleBtn icon={<Pin size={12} />} label="Pin Context" onClick={() => {}} />
      </div>
    </div>
  )
}

function BubbleBtn({ icon, label, onClick }: { icon: ReactNode; label: string; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      title={label}
      className="flex items-center gap-1.5 rounded-lg px-2 py-1.5 transition-all"
      style={{
        background: 'var(--cc-surface)',
        border: '1px solid var(--cc-border)',
        color: 'var(--cc-text-muted)',
        cursor: 'pointer',
        fontSize: 11,
        whiteSpace: 'nowrap',
      }}
    >
      {icon}
      {label}
    </button>
  )
}
