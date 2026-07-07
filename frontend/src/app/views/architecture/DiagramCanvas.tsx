import { Maximize2, Move, ZoomIn, ZoomOut } from 'lucide-react'
import { useCallback, useLayoutEffect, useRef, useState, type ReactNode } from 'react'

interface DiagramCanvasProps {
  width: number
  height: number
  minZoom?: number
  maxZoom?: number
  initialZoom?: number
  autoFit?: boolean
  children: ReactNode
}

export function DiagramCanvas({
  width,
  height,
  minZoom = 0.45,
  maxZoom = 1.8,
  initialZoom = 1,
  autoFit = true,
  children,
}: DiagramCanvasProps) {
  const [zoom, setZoom] = useState(initialZoom)
  const [pan, setPan] = useState({ x: 0, y: 0 })
  const canvasRef = useRef<HTMLDivElement | null>(null)
  const dragRef = useRef<{ pointerId: number; x: number; y: number; panX: number; panY: number } | null>(null)

  const updateZoom = useCallback((nextZoom: number) => {
    setZoom(Math.max(minZoom, Math.min(maxZoom, nextZoom)))
  }, [maxZoom, minZoom])

  const fit = useCallback((element: HTMLDivElement | null) => {
    if (!element) return
    const rect = element.getBoundingClientRect()
    const nextZoom = Math.min(maxZoom, Math.max(minZoom, Math.min(rect.width / width, rect.height / height) * 0.92))
    setZoom(nextZoom)
    setPan({
      x: Math.max(24, (rect.width - width * nextZoom) / 2),
      y: Math.max(24, (rect.height - height * nextZoom) / 2),
    })
  }, [height, maxZoom, minZoom, width])

  useLayoutEffect(() => {
    if (!autoFit) return
    const element = canvasRef.current
    if (!element) return
    const handle = window.requestAnimationFrame(() => fit(element))
    const observer = new ResizeObserver(() => fit(element))
    observer.observe(element)
    return () => {
      window.cancelAnimationFrame(handle)
      observer.disconnect()
    }
  }, [autoFit, fit])

  return (
    <div
      ref={canvasRef}
      className="diagram-canvas"
      onWheel={event => {
        if (!event.ctrlKey && Math.abs(event.deltaY) < Math.abs(event.deltaX)) return
        event.preventDefault()
        updateZoom(zoom - event.deltaY * 0.001)
      }}
      onPointerDown={event => {
        const target = event.target as Element
        if (target.closest('[data-no-pan="true"]')) return
        dragRef.current = { pointerId: event.pointerId, x: event.clientX, y: event.clientY, panX: pan.x, panY: pan.y }
        event.currentTarget.setPointerCapture(event.pointerId)
      }}
      onPointerMove={event => {
        const drag = dragRef.current
        if (!drag || drag.pointerId !== event.pointerId) return
        setPan({ x: drag.panX + event.clientX - drag.x, y: drag.panY + event.clientY - drag.y })
      }}
      onPointerUp={event => {
        if (dragRef.current?.pointerId === event.pointerId) dragRef.current = null
      }}
      onPointerCancel={event => {
        if (dragRef.current?.pointerId === event.pointerId) dragRef.current = null
      }}
    >
      <div className="diagram-controls" data-no-pan="true">
        <button className="arch-icon-button" onClick={() => updateZoom(zoom + 0.12)} title="Zoom in"><ZoomIn size={14} /></button>
        <button className="arch-icon-button" onClick={() => updateZoom(zoom - 0.12)} title="Zoom out"><ZoomOut size={14} /></button>
        <button className="arch-icon-button" onClick={() => fit(canvasRef.current)} title="Fit diagram"><Maximize2 size={14} /></button>
        <button className="arch-icon-button" onClick={() => { setZoom(1); setPan({ x: 0, y: 0 }) }} title="Reset view"><Move size={14} /></button>
        <span className="diagram-zoom-label">{Math.round(zoom * 100)}%</span>
      </div>
      <div
        className="diagram-content"
        style={{
          width,
          height,
          transform: `translate(${pan.x}px, ${pan.y}px) scale(${zoom})`,
        }}
      >
        {children}
      </div>
    </div>
  )
}
