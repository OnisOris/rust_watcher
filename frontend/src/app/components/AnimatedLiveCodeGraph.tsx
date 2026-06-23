import { useEffect, useMemo, useState, type ComponentProps } from 'react'
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

  return (
    <BaseLiveCodeGraph
      {...props}
      highlightedTraceNodeIds={highlightedTraceNodeIds}
      highlightedTraceEdgeIds={highlightedTraceEdgeIds}
    />
  )
}
