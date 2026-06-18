import type { ReactNode } from 'react'
import { AlertCircle, Check, EyeOff, Focus, Layers, Minimize2, RotateCcw, SlidersHorizontal, Waypoints, X } from 'lucide-react'
import type { GraphLayoutSettings } from '../types'

type GraphLens = 'all' | 'architecture' | 'api'

interface DenseGraphSuggestionProps {
  graphLens: GraphLens
  totalNodes: number
  visibleNodes: number
  totalEdges: number
  visibleEdges: number
  depth: 1 | 2 | 3 | 'full'
  externalHidden: boolean
  testsHidden: boolean
  canFocus: boolean
  layoutSettings: GraphLayoutSettings
  onDismiss: () => void
  onLensChange: (lens: GraphLens) => void
  onLayoutSettingsChange: (settings: GraphLayoutSettings) => void
  onResetLayoutSettings: () => void
  onHideExternal: () => void
  onHideTests: () => void
  onDepth2: () => void
  onFocusBubble: () => void
}

export function DenseGraphSuggestion({
  graphLens,
  totalNodes,
  visibleNodes,
  totalEdges,
  visibleEdges,
  depth,
  externalHidden,
  testsHidden,
  canFocus,
  layoutSettings,
  onDismiss,
  onLensChange,
  onLayoutSettingsChange,
  onResetLayoutSettings,
  onHideExternal,
  onHideTests,
  onDepth2,
  onFocusBubble,
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
        <div className="grid grid-cols-3 gap-1.5">
          <LensButton label="Full" active={graphLens === 'all'} onClick={() => onLensChange('all')} />
          <LensButton label="Architecture" active={graphLens === 'architecture'} onClick={() => onLensChange('architecture')} />
          <LensButton label="API" active={graphLens === 'api'} onClick={() => onLensChange('api')} />
        </div>

        <div className="mt-3 flex flex-col gap-1.5">
          <SuggestionAction
            icon={<Minimize2 size={12} />}
            label="Architecture map"
            detail="Files, crates, endpoints"
            active={graphLens === 'architecture'}
            onClick={() => onLensChange('architecture')}
          />
          <SuggestionAction
            icon={<Waypoints size={12} />}
            label="API bridge only"
            detail="Frontend calls to Rust routes"
            active={graphLens === 'api'}
            onClick={() => onLensChange('api')}
          />
          <SuggestionAction
            icon={<EyeOff size={12} />}
            label="External crates"
            detail={externalHidden ? 'Hidden' : 'Visible'}
            active={externalHidden}
            onClick={onHideExternal}
          />
          <SuggestionAction
            icon={<EyeOff size={12} />}
            label="Tests"
            detail={testsHidden ? 'Hidden' : 'Visible'}
            active={testsHidden}
            onClick={onHideTests}
          />
          <SuggestionAction
            icon={<Layers size={12} />}
            label="Depth 2"
            detail={depth === 2 ? 'Active' : 'Show local neighborhood'}
            active={depth === 2}
            onClick={onDepth2}
          />
          <SuggestionAction
            icon={<Focus size={12} />}
            label="Focus selected"
            detail={canFocus ? 'Open focus bubble' : 'Select a node first'}
            disabled={!canFocus}
            onClick={onFocusBubble}
          />
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

function SuggestionAction({
  icon,
  label,
  detail,
  active,
  disabled,
  onClick,
}: {
  icon: ReactNode
  label: string
  detail: string
  active?: boolean
  disabled?: boolean
  onClick: () => void
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className="flex items-center gap-2 rounded-lg px-3 py-2 w-full transition-all text-left"
      style={{
        background: active ? 'rgba(6,182,212,0.12)' : 'var(--cc-card)',
        border: active ? '1px solid rgba(6,182,212,0.35)' : '1px solid var(--cc-border)',
        color: disabled ? 'var(--cc-text-faint)' : active ? '#06B6D4' : 'var(--cc-text-muted)',
        cursor: disabled ? 'not-allowed' : 'pointer',
        opacity: disabled ? 0.6 : 1,
      }}
    >
      <span className="flex items-center justify-center shrink-0" style={{ width: 18 }}>{active ? <Check size={12} /> : icon}</span>
      <span className="min-w-0 flex-1">
        <span style={{ display: 'block', fontSize: 12, fontWeight: 550 }}>{label}</span>
        <span style={{ display: 'block', fontSize: 10, color: active ? '#0891B2' : 'var(--cc-text-subtle)', marginTop: 1 }}>{detail}</span>
      </span>
    </button>
  )
}
