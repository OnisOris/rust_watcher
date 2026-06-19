import type { ReactNode } from 'react'
import { AlertCircle, RotateCcw, SlidersHorizontal, Tags, X } from 'lucide-react'
import type { GraphLabelMode, GraphLayoutSettings } from '../types'

type GraphLens = 'all' | 'architecture' | 'api'

interface DenseGraphSuggestionProps {
  graphLens: GraphLens
  totalNodes: number
  visibleNodes: number
  totalEdges: number
  visibleEdges: number
  labelMode: GraphLabelMode
  layoutSettings: GraphLayoutSettings
  onDismiss: () => void
  onLensChange: (lens: GraphLens) => void
  onLabelModeChange: (mode: GraphLabelMode) => void
  onLayoutSettingsChange: (settings: GraphLayoutSettings) => void
  onResetLayoutSettings: () => void
}

export function DenseGraphSuggestion({
  graphLens,
  totalNodes,
  visibleNodes,
  totalEdges,
  visibleEdges,
  labelMode,
  layoutSettings,
  onDismiss,
  onLensChange,
  onLabelModeChange,
  onLayoutSettingsChange,
  onResetLayoutSettings,
}: DenseGraphSuggestionProps) {
  return (
    <div
      className="overflow-hidden rounded-xl shadow-2xl"
      style={{
        width: 320,
        background: 'var(--cc-panel)',
        border: '1px solid var(--cc-border-strong)',
        boxShadow: 'var(--cc-shadow)',
        fontFamily: 'Inter, sans-serif',
      }}
    >
      <div className="flex items-start gap-2 px-3 py-3" style={{ borderBottom: '1px solid var(--cc-border)' }}>
        <div className="mt-0.5 rounded-md p-1" style={{ background: 'rgba(245,158,11,0.12)' }}>
          <AlertCircle size={13} color="#F59E0B" />
        </div>
        <div className="min-w-0 flex-1">
          <div style={{ fontSize: 12, color: 'var(--cc-text)', fontWeight: 650 }}>Graph clarity</div>
          <div style={{ fontSize: 10, color: 'var(--cc-text-subtle)', marginTop: 2 }}>
            Showing {visibleNodes}/{totalNodes} nodes · {visibleEdges}/{totalEdges} edges
          </div>
        </div>
        <button
          onClick={onDismiss}
          title="Collapse to toolbar tab"
          className="flex items-center justify-center rounded-md"
          style={{ width: 24, height: 24, background: 'transparent', border: '1px solid transparent', cursor: 'pointer', color: 'var(--cc-text-subtle)' }}
        >
          <X size={13} />
        </button>
      </div>

      <div className="px-3 py-3">
        <SectionHeader title="Scope" />
        <div className="grid grid-cols-3 gap-1.5">
          <LensButton label="Full" active={graphLens === 'all'} onClick={() => onLensChange('all')} />
          <LensButton label="Architecture" active={graphLens === 'architecture'} onClick={() => onLensChange('architecture')} />
          <LensButton label="API" active={graphLens === 'api'} onClick={() => onLensChange('api')} />
        </div>

        <div className="mt-3">
          <SectionHeader title="Labels" icon={<Tags size={12} color="#06B6D4" />} />
          <div className="grid grid-cols-3 gap-1.5">
            <LensButton label="Auto" active={labelMode === 'auto'} onClick={() => onLabelModeChange('auto')} />
            <LensButton label="Key" active={labelMode === 'key'} onClick={() => onLabelModeChange('key')} />
            <LensButton label="All" active={labelMode === 'all'} onClick={() => onLabelModeChange('all')} />
          </div>
        </div>

        <div
          className="mt-3 rounded-xl px-3 py-3"
          style={{ background: 'var(--cc-card)', border: '1px solid var(--cc-border)' }}
        >
          <div className="mb-2 flex items-center gap-2">
            <SlidersHorizontal size={12} color="#06B6D4" />
            <div className="min-w-0 flex-1" style={{ fontSize: 11, color: 'var(--cc-text)', fontWeight: 650 }}>Layout</div>
            <button
              onClick={onResetLayoutSettings}
              title="Reset layout tuning"
              className="flex items-center justify-center rounded-md"
              style={{ width: 24, height: 22, background: 'var(--cc-surface)', border: '1px solid var(--cc-border)', color: 'var(--cc-text-subtle)', cursor: 'pointer' }}
            >
              <RotateCcw size={11} />
            </button>
          </div>
          <div className="flex flex-col gap-2.5">
            <LayoutSlider
              label="Spacing"
              value={layoutSettings.spacing}
              min={0.7}
              max={2.6}
              step={0.05}
              onChange={spacing => onLayoutSettingsChange({ ...layoutSettings, spacing })}
            />
            <LayoutSlider
              label="Repulsion"
              value={layoutSettings.repulsion}
              min={0.5}
              max={2.8}
              step={0.05}
              onChange={repulsion => onLayoutSettingsChange({ ...layoutSettings, repulsion })}
            />
            <LayoutSlider
              label="Links"
              value={layoutSettings.linkLength}
              min={0.65}
              max={2.4}
              step={0.05}
              onChange={linkLength => onLayoutSettingsChange({ ...layoutSettings, linkLength })}
            />
            <LayoutSlider
              label="Damping"
              value={layoutSettings.damping}
              min={0.55}
              max={2.2}
              step={0.05}
              onChange={damping => onLayoutSettingsChange({ ...layoutSettings, damping })}
            />
          </div>
        </div>
      </div>
    </div>
  )
}

function SectionHeader({ title, icon }: { title: string; icon?: ReactNode }) {
  return (
    <div className="mb-2 flex items-center gap-1.5">
      {icon}
      <span style={{ fontSize: 10, color: 'var(--cc-text-faint)', fontWeight: 700, letterSpacing: '0.07em', textTransform: 'uppercase' }}>
        {title}
      </span>
    </div>
  )
}

function LayoutSlider({
  label,
  value,
  min,
  max,
  step,
  onChange,
}: {
  label: string
  value: number
  min: number
  max: number
  step: number
  onChange: (value: number) => void
}) {
  return (
    <label className="grid items-center gap-2" style={{ gridTemplateColumns: '72px 1fr 34px' }}>
      <span style={{ fontSize: 10, color: 'var(--cc-text-muted)', fontWeight: 550 }}>{label}</span>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={event => onChange(Number(event.target.value))}
        style={{
          width: '100%',
          accentColor: '#06B6D4',
          cursor: 'pointer',
        }}
      />
      <span style={{ fontSize: 10, color: 'var(--cc-text-subtle)', textAlign: 'right', fontVariantNumeric: 'tabular-nums' }}>
        {value.toFixed(2)}
      </span>
    </label>
  )
}

function LensButton({ label, active, onClick }: { label: string; active: boolean; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      className="rounded-md px-2 py-1.5 transition-all"
      style={{
        background: active ? 'rgba(6,182,212,0.14)' : 'var(--cc-surface)',
        border: active ? '1px solid rgba(6,182,212,0.35)' : '1px solid var(--cc-border)',
        color: active ? '#06B6D4' : 'var(--cc-text-muted)',
        cursor: 'pointer',
        fontSize: 10,
        fontWeight: active ? 650 : 500,
      }}
    >
      {label}
    </button>
  )
}
