type Matrix = { a: number; b: number; c: number; d: number; e: number; f: number }

type CanvasState = {
  transform: Matrix
  stack: Array<Omit<CanvasState, 'stack' | 'ops' | 'currentPath'>>
  fillStyle: string | CanvasGradient | CanvasPattern
  strokeStyle: string | CanvasGradient | CanvasPattern
  globalAlpha: number
  lineWidth: number
  font: string
  textAlign: CanvasTextAlign
  textBaseline: CanvasTextBaseline
  lineDash: number[]
  ops: SvgOp[]
  currentPath: string[]
}

type Paint = string | GradientPaint

type GradientPaint = {
  kind: 'radial-gradient'
  id: string
  x0: number
  y0: number
  r0: number
  x1: number
  y1: number
  r1: number
  stops: Array<{ offset: number; color: string }>
}

type SvgOp =
  | { kind: 'rect'; x: number; y: number; width: number; height: number; fill?: Paint; stroke?: Paint; lineWidth?: number; alpha: number; dash?: number[]; transform: Matrix }
  | { kind: 'path'; d: string; fill?: Paint; stroke?: Paint; lineWidth?: number; alpha: number; dash?: number[]; transform: Matrix }
  | { kind: 'text'; text: string; x: number; y: number; fill: Paint; alpha: number; font: string; align: CanvasTextAlign; baseline: CanvasTextBaseline; transform: Matrix }

type RadialGradientMeta = Omit<GradientPaint, 'id'>

const stateByContext = new WeakMap<CanvasRenderingContext2D, CanvasState>()
const gradientMetaByObject = new WeakMap<CanvasGradient, RadialGradientMeta>()
let patched = false
let gradientCounter = 0

function identity(): Matrix {
  return { a: 1, b: 0, c: 0, d: 1, e: 0, f: 0 }
}

function cloneMatrix(matrix: Matrix): Matrix {
  return { ...matrix }
}

function multiply(left: Matrix, right: Matrix): Matrix {
  return {
    a: left.a * right.a + left.c * right.b,
    b: left.b * right.a + left.d * right.b,
    c: left.a * right.c + left.c * right.d,
    d: left.b * right.c + left.d * right.d,
    e: left.a * right.e + left.c * right.f + left.e,
    f: left.b * right.e + left.d * right.f + left.f,
  }
}

function getState(ctx: CanvasRenderingContext2D): CanvasState {
  let state = stateByContext.get(ctx)
  if (!state) {
    state = {
      transform: identity(),
      stack: [],
      fillStyle: '#000000',
      strokeStyle: '#000000',
      globalAlpha: 1,
      lineWidth: 1,
      font: '10px sans-serif',
      textAlign: 'start',
      textBaseline: 'alphabetic',
      lineDash: [],
      ops: [],
      currentPath: [],
    }
    stateByContext.set(ctx, state)
  }
  return state
}

function cloneStateForStack(state: CanvasState): Omit<CanvasState, 'stack' | 'ops' | 'currentPath'> {
  return {
    transform: cloneMatrix(state.transform),
    fillStyle: state.fillStyle,
    strokeStyle: state.strokeStyle,
    globalAlpha: state.globalAlpha,
    lineWidth: state.lineWidth,
    font: state.font,
    textAlign: state.textAlign,
    textBaseline: state.textBaseline,
    lineDash: [...state.lineDash],
  }
}

function normalizeInitialCanvasTransform(ctx: CanvasRenderingContext2D, matrix: Matrix): Matrix {
  const dpr = window.devicePixelRatio || 1
  const rect = ctx.canvas.getBoundingClientRect()
  const isDevicePixelRoot =
    Math.abs(matrix.a - dpr) < 0.001 &&
    Math.abs(matrix.d - dpr) < 0.001 &&
    Math.abs(matrix.b) < 0.001 &&
    Math.abs(matrix.c) < 0.001 &&
    Math.abs(matrix.e) < 0.001 &&
    Math.abs(matrix.f) < 0.001 &&
    ctx.canvas.width === Math.round(rect.width * dpr) &&
    ctx.canvas.height === Math.round(rect.height * dpr)

  return isDevicePixelRoot ? identity() : matrix
}

function num(value: number) {
  if (!Number.isFinite(value)) return '0'
  return Number(value.toFixed(3)).toString()
}

function escapeText(value: string) {
  return value.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
}

function escapeAttr(value: string) {
  return escapeText(value).replace(/"/g, '&quot;')
}

function matrixAttr(matrix: Matrix) {
  return `matrix(${num(matrix.a)} ${num(matrix.b)} ${num(matrix.c)} ${num(matrix.d)} ${num(matrix.e)} ${num(matrix.f)})`
}

function paintFrom(value: string | CanvasGradient | CanvasPattern): Paint {
  if (typeof value === 'string') return value
  const gradient = gradientMetaByObject.get(value as CanvasGradient)
  if (!gradient) return '#000000'
  gradientCounter += 1
  return { ...gradient, id: `rw-gradient-${gradientCounter}` }
}

function paintValue(paint: Paint, defs: string[]) {
  if (typeof paint === 'string') return paint
  const stops = paint.stops.length > 0
    ? paint.stops
    : [{ offset: 0, color: '#f8fbff' }, { offset: 1, color: '#eef4fb' }]
  defs.push(
    `<radialGradient id="${paint.id}" gradientUnits="userSpaceOnUse" cx="${num(paint.x1)}" cy="${num(paint.y1)}" r="${num(paint.r1)}" fx="${num(paint.x0)}" fy="${num(paint.y0)}">
${stops.map(stop => `    <stop offset="${num(stop.offset * 100)}%" stop-color="${escapeAttr(stop.color)}"/>`).join('\n')}
  </radialGradient>`,
  )
  return `url(#${paint.id})`
}

function pathFromRoundRect(x: number, y: number, width: number, height: number, radius: number) {
  const r = Math.max(0, Math.min(radius, Math.abs(width) / 2, Math.abs(height) / 2))
  const right = x + width
  const bottom = y + height
  if (r <= 0) return `M ${num(x)} ${num(y)} H ${num(right)} V ${num(bottom)} H ${num(x)} Z`
  return [
    `M ${num(x + r)} ${num(y)}`,
    `H ${num(right - r)}`,
    `Q ${num(right)} ${num(y)} ${num(right)} ${num(y + r)}`,
    `V ${num(bottom - r)}`,
    `Q ${num(right)} ${num(bottom)} ${num(right - r)} ${num(bottom)}`,
    `H ${num(x + r)}`,
    `Q ${num(x)} ${num(bottom)} ${num(x)} ${num(bottom - r)}`,
    `V ${num(y + r)}`,
    `Q ${num(x)} ${num(y)} ${num(x + r)} ${num(y)}`,
    'Z',
  ].join(' ')
}

function pathFromArc(x: number, y: number, radius: number, startAngle: number, endAngle: number, counterclockwise?: boolean) {
  const fullCircle = Math.abs(Math.abs(endAngle - startAngle) - Math.PI * 2) < 0.001
  if (fullCircle) {
    return [
      `M ${num(x + radius)} ${num(y)}`,
      `A ${num(radius)} ${num(radius)} 0 1 0 ${num(x - radius)} ${num(y)}`,
      `A ${num(radius)} ${num(radius)} 0 1 0 ${num(x + radius)} ${num(y)}`,
    ].join(' ')
  }
  const sx = x + Math.cos(startAngle) * radius
  const sy = y + Math.sin(startAngle) * radius
  const ex = x + Math.cos(endAngle) * radius
  const ey = y + Math.sin(endAngle) * radius
  const largeArc = Math.abs(endAngle - startAngle) > Math.PI ? 1 : 0
  const sweep = counterclockwise ? 0 : 1
  return `M ${num(sx)} ${num(sy)} A ${num(radius)} ${num(radius)} 0 ${largeArc} ${sweep} ${num(ex)} ${num(ey)}`
}

function normalizeRadius(radius: unknown) {
  if (typeof radius === 'number') return radius
  if (Array.isArray(radius)) {
    const first = radius[0]
    return typeof first === 'number' ? first : typeof first?.x === 'number' ? first.x : 0
  }
  if (radius && typeof radius === 'object' && 'x' in radius && typeof (radius as DOMPointInit).x === 'number') return (radius as DOMPointInit).x ?? 0
  return 0
}

function dashAttr(dash?: number[]) {
  return dash && dash.length > 0 ? ` stroke-dasharray="${dash.map(num).join(' ')}"` : ''
}

function serializeOp(op: SvgOp, defs: string[]) {
  if (op.kind === 'text') {
    const fill = paintValue(op.fill, defs)
    const anchor = op.align === 'center' ? 'middle' : op.align === 'right' || op.align === 'end' ? 'end' : 'start'
    const baseline = op.baseline === 'middle' ? 'central' : op.baseline === 'top' || op.baseline === 'hanging' ? 'text-before-edge' : 'alphabetic'
    return `<text x="${num(op.x)}" y="${num(op.y)}" transform="${matrixAttr(op.transform)}" fill="${escapeAttr(fill)}" opacity="${num(op.alpha)}" text-anchor="${anchor}" dominant-baseline="${baseline}" style="font: ${escapeAttr(op.font)}">${escapeText(op.text)}</text>`
  }

  const fill = op.fill ? paintValue(op.fill, defs) : 'none'
  const stroke = op.stroke ? paintValue(op.stroke, defs) : 'none'
  const common = `transform="${matrixAttr(op.transform)}" fill="${escapeAttr(fill)}" stroke="${escapeAttr(stroke)}" opacity="${num(op.alpha)}"`
  const strokeAttrs = op.stroke ? ` stroke-width="${num(op.lineWidth ?? 1)}" stroke-linecap="round" stroke-linejoin="round"${dashAttr(op.dash)}` : ''

  if (op.kind === 'rect') {
    return `<rect x="${num(op.x)}" y="${num(op.y)}" width="${num(op.width)}" height="${num(op.height)}" ${common}${strokeAttrs}/>`
  }
  return `<path d="${escapeAttr(op.d)}" ${common}${strokeAttrs}/>`
}

function downloadText(filename: string, content: string, type: string) {
  const blob = new Blob([content], { type })
  const url = URL.createObjectURL(blob)
  const link = document.createElement('a')
  link.href = url
  link.download = filename
  document.body.appendChild(link)
  link.click()
  link.remove()
  window.setTimeout(() => URL.revokeObjectURL(url), 1000)
}

export function exportRecordedCanvasAsSvg(canvas: HTMLCanvasElement, filename: string) {
  ensureCanvasSvgRecorder()
  const ctx = canvas.getContext('2d')
  if (!ctx) return false
  const state = stateByContext.get(ctx)
  if (!state || state.ops.length === 0) return false

  const rect = canvas.getBoundingClientRect()
  const width = Math.max(1, Math.round(rect.width || canvas.width))
  const height = Math.max(1, Math.round(rect.height || canvas.height))
  gradientCounter = 0

  const defs: string[] = []
  const body = state.ops.map(op => serializeOp(op, defs)).join('\n  ')
  const svg = `<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}" role="img" aria-label="Rust watcher graph export">
  ${defs.length > 0 ? `<defs>\n  ${defs.join('\n  ')}\n  </defs>` : ''}
  ${body}
</svg>`

  downloadText(filename, svg, 'image/svg+xml;charset=utf-8')
  return true
}

function patchProperty(name: 'fillStyle' | 'strokeStyle' | 'globalAlpha' | 'lineWidth' | 'font' | 'textAlign' | 'textBaseline', onSet: (state: CanvasState, value: unknown) => void) {
  const proto = CanvasRenderingContext2D.prototype
  const descriptor = Object.getOwnPropertyDescriptor(proto, name)
  if (!descriptor?.get || !descriptor?.set) return

  Object.defineProperty(proto, name, {
    configurable: true,
    enumerable: descriptor.enumerable,
    get() {
      return descriptor.get!.call(this)
    },
    set(value: unknown) {
      onSet(getState(this as CanvasRenderingContext2D), value)
      descriptor.set!.call(this, value)
    },
  })
}

function patchMethod(name: string, factory: (original: (...args: unknown[]) => unknown) => (...args: unknown[]) => unknown) {
  const proto = CanvasRenderingContext2D.prototype as unknown as Record<string, (...args: unknown[]) => unknown>
  const original = proto[name]
  if (!original) return
  proto[name] = factory(original)
}

export function ensureCanvasSvgRecorder() {
  if (patched || typeof CanvasRenderingContext2D === 'undefined') return
  patched = true

  patchProperty('fillStyle', (state, value) => { state.fillStyle = value as string | CanvasGradient | CanvasPattern })
  patchProperty('strokeStyle', (state, value) => { state.strokeStyle = value as string | CanvasGradient | CanvasPattern })
  patchProperty('globalAlpha', (state, value) => { state.globalAlpha = Number(value) })
  patchProperty('lineWidth', (state, value) => { state.lineWidth = Number(value) })
  patchProperty('font', (state, value) => { state.font = String(value) })
  patchProperty('textAlign', (state, value) => { state.textAlign = value as CanvasTextAlign })
  patchProperty('textBaseline', (state, value) => { state.textBaseline = value as CanvasTextBaseline })

  patchMethod('save', original => function patchedSave(this: CanvasRenderingContext2D) {
    getState(this).stack.push(cloneStateForStack(getState(this)))
    return original.call(this)
  })

  patchMethod('restore', original => function patchedRestore(this: CanvasRenderingContext2D) {
    const state = getState(this)
    const previous = state.stack.pop()
    if (previous) Object.assign(state, previous)
    return original.call(this)
  })

  patchMethod('setTransform', original => function patchedSetTransform(this: CanvasRenderingContext2D, ...args: unknown[]) {
    const state = getState(this)
    if (typeof args[0] === 'number') {
      state.transform = normalizeInitialCanvasTransform(this, { a: args[0], b: Number(args[1] ?? 0), c: Number(args[2] ?? 0), d: Number(args[3] ?? 1), e: Number(args[4] ?? 0), f: Number(args[5] ?? 0) })
    } else {
      const matrix = (args[0] ?? {}) as DOMMatrix2DInit
      state.transform = normalizeInitialCanvasTransform(this, { a: matrix.a ?? 1, b: matrix.b ?? 0, c: matrix.c ?? 0, d: matrix.d ?? 1, e: matrix.e ?? 0, f: matrix.f ?? 0 })
    }
    return original.apply(this, args)
  })

  patchMethod('translate', original => function patchedTranslate(this: CanvasRenderingContext2D, x: unknown, y: unknown) {
    const state = getState(this)
    state.transform = multiply(state.transform, { a: 1, b: 0, c: 0, d: 1, e: Number(x), f: Number(y) })
    return original.call(this, x, y)
  })

  patchMethod('scale', original => function patchedScale(this: CanvasRenderingContext2D, x: unknown, y: unknown) {
    const state = getState(this)
    state.transform = multiply(state.transform, { a: Number(x), b: 0, c: 0, d: Number(y), e: 0, f: 0 })
    return original.call(this, x, y)
  })

  patchMethod('rotate', original => function patchedRotate(this: CanvasRenderingContext2D, angle: unknown) {
    const cos = Math.cos(Number(angle))
    const sin = Math.sin(Number(angle))
    const state = getState(this)
    state.transform = multiply(state.transform, { a: cos, b: sin, c: -sin, d: cos, e: 0, f: 0 })
    return original.call(this, angle)
  })

  patchMethod('transform', original => function patchedTransform(this: CanvasRenderingContext2D, a: unknown, b: unknown, c: unknown, d: unknown, e: unknown, f: unknown) {
    const state = getState(this)
    state.transform = multiply(state.transform, { a: Number(a), b: Number(b), c: Number(c), d: Number(d), e: Number(e), f: Number(f) })
    return original.call(this, a, b, c, d, e, f)
  })

  patchMethod('setLineDash', original => function patchedSetLineDash(this: CanvasRenderingContext2D, segments: unknown) {
    getState(this).lineDash = Array.from(segments as Iterable<number>)
    return original.call(this, segments)
  })

  patchMethod('beginPath', original => function patchedBeginPath(this: CanvasRenderingContext2D) {
    getState(this).currentPath = []
    return original.call(this)
  })

  patchMethod('moveTo', original => function patchedMoveTo(this: CanvasRenderingContext2D, x: unknown, y: unknown) {
    getState(this).currentPath.push(`M ${num(Number(x))} ${num(Number(y))}`)
    return original.call(this, x, y)
  })

  patchMethod('lineTo', original => function patchedLineTo(this: CanvasRenderingContext2D, x: unknown, y: unknown) {
    getState(this).currentPath.push(`L ${num(Number(x))} ${num(Number(y))}`)
    return original.call(this, x, y)
  })

  patchMethod('closePath', original => function patchedClosePath(this: CanvasRenderingContext2D) {
    getState(this).currentPath.push('Z')
    return original.call(this)
  })

  patchMethod('arc', original => function patchedArc(this: CanvasRenderingContext2D, x: unknown, y: unknown, radius: unknown, startAngle: unknown, endAngle: unknown, counterclockwise?: unknown) {
    getState(this).currentPath.push(pathFromArc(Number(x), Number(y), Number(radius), Number(startAngle), Number(endAngle), Boolean(counterclockwise)))
    return original.call(this, x, y, radius, startAngle, endAngle, counterclockwise)
  })

  patchMethod('roundRect', original => function patchedRoundRect(this: CanvasRenderingContext2D, x: unknown, y: unknown, width: unknown, height: unknown, radii?: unknown) {
    getState(this).currentPath.push(pathFromRoundRect(Number(x), Number(y), Number(width), Number(height), normalizeRadius(radii ?? 0)))
    return original.call(this, x, y, width, height, radii)
  })

  patchMethod('fill', original => function patchedFill(this: CanvasRenderingContext2D, ...args: unknown[]) {
    const state = getState(this)
    const d = state.currentPath.join(' ')
    if (d) state.ops.push({ kind: 'path', d, fill: paintFrom(state.fillStyle), alpha: state.globalAlpha, transform: cloneMatrix(state.transform) })
    return original.apply(this, args)
  })

  patchMethod('stroke', original => function patchedStroke(this: CanvasRenderingContext2D, ...args: unknown[]) {
    const state = getState(this)
    const d = state.currentPath.join(' ')
    if (d) state.ops.push({ kind: 'path', d, stroke: paintFrom(state.strokeStyle), lineWidth: state.lineWidth, alpha: state.globalAlpha, dash: [...state.lineDash], transform: cloneMatrix(state.transform) })
    return original.apply(this, args)
  })

  patchMethod('fillRect', original => function patchedFillRect(this: CanvasRenderingContext2D, x: unknown, y: unknown, width: unknown, height: unknown) {
    const state = getState(this)
    state.ops.push({ kind: 'rect', x: Number(x), y: Number(y), width: Number(width), height: Number(height), fill: paintFrom(state.fillStyle), alpha: state.globalAlpha, transform: cloneMatrix(state.transform) })
    return original.call(this, x, y, width, height)
  })

  patchMethod('strokeRect', original => function patchedStrokeRect(this: CanvasRenderingContext2D, x: unknown, y: unknown, width: unknown, height: unknown) {
    const state = getState(this)
    state.ops.push({ kind: 'rect', x: Number(x), y: Number(y), width: Number(width), height: Number(height), stroke: paintFrom(state.strokeStyle), lineWidth: state.lineWidth, alpha: state.globalAlpha, dash: [...state.lineDash], transform: cloneMatrix(state.transform) })
    return original.call(this, x, y, width, height)
  })

  patchMethod('clearRect', original => function patchedClearRect(this: CanvasRenderingContext2D, x: unknown, y: unknown, width: unknown, height: unknown) {
    const state = getState(this)
    const rect = this.canvas.getBoundingClientRect()
    if (Number(x) <= 0 && Number(y) <= 0 && Number(width) >= rect.width - 1 && Number(height) >= rect.height - 1) {
      state.ops = []
      state.currentPath = []
    }
    return original.call(this, x, y, width, height)
  })

  patchMethod('fillText', original => function patchedFillText(this: CanvasRenderingContext2D, text: unknown, x: unknown, y: unknown, maxWidth?: unknown) {
    const state = getState(this)
    state.ops.push({ kind: 'text', text: String(text), x: Number(x), y: Number(y), fill: paintFrom(state.fillStyle), alpha: state.globalAlpha, font: state.font, align: state.textAlign, baseline: state.textBaseline, transform: cloneMatrix(state.transform) })
    return typeof maxWidth === 'number' ? original.call(this, text, x, y, maxWidth) : original.call(this, text, x, y)
  })

  patchMethod('strokeText', original => function patchedStrokeText(this: CanvasRenderingContext2D, text: unknown, x: unknown, y: unknown, maxWidth?: unknown) {
    const state = getState(this)
    state.ops.push({ kind: 'text', text: String(text), x: Number(x), y: Number(y), fill: paintFrom(state.strokeStyle), alpha: state.globalAlpha, font: state.font, align: state.textAlign, baseline: state.textBaseline, transform: cloneMatrix(state.transform) })
    return typeof maxWidth === 'number' ? original.call(this, text, x, y, maxWidth) : original.call(this, text, x, y)
  })

  patchMethod('createRadialGradient', original => function patchedCreateRadialGradient(this: CanvasRenderingContext2D, x0: unknown, y0: unknown, r0: unknown, x1: unknown, y1: unknown, r1: unknown) {
    const gradient = original.call(this, x0, y0, r0, x1, y1, r1) as CanvasGradient
    const meta: RadialGradientMeta = { kind: 'radial-gradient', x0: Number(x0), y0: Number(y0), r0: Number(r0), x1: Number(x1), y1: Number(y1), r1: Number(r1), stops: [] }
    gradientMetaByObject.set(gradient, meta)
    const addColorStop = gradient.addColorStop.bind(gradient)
    try {
      gradient.addColorStop = (offset: number, color: string) => {
        meta.stops.push({ offset, color })
        addColorStop(offset, color)
      }
    } catch {
      // CanvasGradient may expose methods as read-only. Fallback stops are still exported.
    }
    return gradient
  })
}

ensureCanvasSvgRecorder()
