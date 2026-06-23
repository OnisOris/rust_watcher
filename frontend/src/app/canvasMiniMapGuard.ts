const MINIMAP_W = 160
const MINIMAP_H = 100
const MINIMAP_MARGIN = 16
const MINIMAP_INSET_X = 8
const MINIMAP_INSET_Y = 8
const MINIMAP_INNER_W = MINIMAP_W - 16
const MINIMAP_INNER_H = MINIMAP_H - 22
const MINIMAP_VIEWPORT_COLOR = '#06b6d4'

type Rect = { x: number; y: number; w: number; h: number }

function colorString(style: string | CanvasGradient | CanvasPattern) {
  return typeof style === 'string' ? style.trim().toLowerCase() : ''
}

function minimapInnerRect(canvas: HTMLCanvasElement): Rect {
  const rect = canvas.getBoundingClientRect()
  const x = rect.width - MINIMAP_W - MINIMAP_MARGIN + MINIMAP_INSET_X
  const y = rect.height - MINIMAP_H - MINIMAP_MARGIN + MINIMAP_INSET_Y
  return { x, y, w: MINIMAP_INNER_W, h: MINIMAP_INNER_H }
}

function intersects(a: Rect, b: Rect) {
  return a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y
}

function isRunawayMinimapViewport(ctx: CanvasRenderingContext2D, rect: Rect, style: string | CanvasGradient | CanvasPattern) {
  if (colorString(style) !== MINIMAP_VIEWPORT_COLOR) return false
  const map = minimapInnerRect(ctx.canvas)
  const oversized = rect.w > MINIMAP_W || rect.h > MINIMAP_H
  const outsideMap = rect.x < map.x || rect.y < map.y || rect.x + rect.w > map.x + map.w || rect.y + rect.h > map.y + map.h
  return oversized || outsideMap || intersects(rect, map)
}

function clampToMinimap(ctx: CanvasRenderingContext2D, rect: Rect) {
  const map = minimapInnerRect(ctx.canvas)
  const x1 = Math.max(rect.x, map.x)
  const y1 = Math.max(rect.y, map.y)
  const x2 = Math.min(rect.x + rect.w, map.x + map.w)
  const y2 = Math.min(rect.y + rect.h, map.y + map.h)
  if (x2 <= x1 || y2 <= y1) return null
  return { x: x1, y: y1, w: x2 - x1, h: y2 - y1 }
}

const proto = CanvasRenderingContext2D.prototype
const originalFillRect = proto.fillRect
const originalStrokeRect = proto.strokeRect

if (!('__rustWatcherMinimapGuard' in proto)) {
  Object.defineProperty(proto, '__rustWatcherMinimapGuard', { value: true })

  proto.fillRect = function fillRectGuarded(x: number, y: number, w: number, h: number) {
    const rect = { x, y, w, h }
    if (isRunawayMinimapViewport(this, rect, this.fillStyle)) {
      const clamped = clampToMinimap(this, rect)
      if (!clamped) return
      return originalFillRect.call(this, clamped.x, clamped.y, clamped.w, clamped.h)
    }
    return originalFillRect.call(this, x, y, w, h)
  }

  proto.strokeRect = function strokeRectGuarded(x: number, y: number, w: number, h: number) {
    const rect = { x, y, w, h }
    if (isRunawayMinimapViewport(this, rect, this.strokeStyle)) {
      const clamped = clampToMinimap(this, rect)
      if (!clamped) return
      return originalStrokeRect.call(this, clamped.x, clamped.y, clamped.w, clamped.h)
    }
    return originalStrokeRect.call(this, x, y, w, h)
  }
}
