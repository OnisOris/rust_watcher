import { useCallback, useEffect, useMemo, useState, type ComponentProps } from 'react'
import { downloadGraphSvg } from '../api/svgExport'
import { LiveCodeGraph as BaseLiveCodeGraph } from './LiveCodeGraph'

export function LiveCodeGraph(props: ComponentProps<typeof BaseLiveCodeGraph>) {
  const [animationFrame, setAnimationFrame] = useState(0)

  useEffect(() => {
    let raf = 0
    let mounted = true

    const tick = () => {
      setAnimationFrame(frame => (frame + 1) % 1_000_000)
      if (mounted) raf = requestAnimationFrame(tick)
    }

    raf = requestAnimationFrame(tick)

    return () => {
      mounted = false
      cancelAnimationFrame(raf)
    }
  }, [])

  const highlightedTraceNodeIds = useMemo(
    () => new Set(props.highlightedTraceNodeIds ?? []),
    [props.highlightedTraceNodeIds, animationFrame],
  )
  const highlightedTraceEdgeIds = useMemo(
    () => new Set(props.highlightedTraceEdgeIds ?? []),
    [props.highlightedTraceEdgeIds, animationFrame],
  )

  const handleExportSvg = useCallback(() => {
    downloadGraphSvg({
      nodes: props.nodes,
      edges: props.edges,
      filters: props.filters,
      selectedNodeId: props.selectedNodeId,
      graphMode: props.graphMode,
      theme: props.theme,
    })
  }, [props.nodes, props.edges, props.filters, props.selectedNodeId, props.graphMode, props.theme])

  return (
    <div className="relative w-full h-full">
      <BaseLiveCodeGraph
        {...props}
        highlightedTraceNodeIds={highlightedTraceNodeIds}
        highlightedTraceEdgeIds={highlightedTraceEdgeIds}
      />
      <button
        type="button"
        onClick={handleExportSvg}
        className="absolute top-3 left-3 z-20 rounded-lg px-3 py-1.5 text-[11px] font-semibold transition hover:scale-[1.02] active:scale-[0.98]"
        style={{
          background: 'var(--cc-overlay)',
          border: '1px solid var(--cc-border)',
          color: 'var(--cc-text)',
          boxShadow: 'var(--cc-shadow)',
          backdropFilter: 'blur(10px)',
        }}
        title="Export visible graph to SVG"
      >
        Export SVG
      </button>
    </div>
  )
}
