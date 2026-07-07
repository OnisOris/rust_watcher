import { LiveCodeGraph } from '../../components/LiveCodeGraph'
import type { RawGraphProps } from './architectureTypes'
import type { ReactNode } from 'react'

export function RawGraphView({ controls, ...props }: RawGraphProps & { controls?: ReactNode }) {
  return (
    <div className="absolute inset-0 flex flex-col overflow-hidden">
      {controls && (
        <div className="shrink-0" style={{ background: 'var(--cc-panel)', borderBottom: '1px solid var(--cc-border)' }}>
          {controls}
        </div>
      )}
      <div className="relative flex-1 min-h-0">
        <LiveCodeGraph {...props} />
      </div>
    </div>
  )
}
