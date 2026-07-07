import { AlertTriangle, Grid2X2, Network, Table2 } from 'lucide-react'
import type { GraphViewMode } from './architectureTypes'

interface ClarityModePanelProps {
  nodeCount: number
  edgeCount: number
  onViewChange: (mode: GraphViewMode) => void
}

export function ClarityModePanel({ nodeCount, edgeCount, onViewChange }: ClarityModePanelProps) {
  return (
    <div className="arch-clarity">
      <div className="flex items-start gap-3">
        <AlertTriangle size={18} style={{ color: '#F59E0B', marginTop: 1 }} />
        <div className="min-w-0">
          <div style={{ fontSize: 13, color: 'var(--cc-text)', fontWeight: 780 }}>Raw graph is dense. Project Map is enabled for readability.</div>
          <div style={{ fontSize: 11, color: 'var(--cc-text-subtle)', marginTop: 3 }}>{nodeCount} nodes and {edgeCount} edges exceed the 150 / 400 readability threshold.</div>
          <div className="flex flex-wrap gap-2 mt-3">
            <button className="arch-action" onClick={() => onViewChange('project-map')}><Grid2X2 size={13} /> Project Map</button>
            <button className="arch-action" onClick={() => onViewChange('dependency-matrix')}><Table2 size={13} /> Matrix</button>
            <button className="arch-action" onClick={() => onViewChange('raw-graph')}><Network size={13} /> Raw graph anyway</button>
          </div>
        </div>
      </div>
    </div>
  )
}
