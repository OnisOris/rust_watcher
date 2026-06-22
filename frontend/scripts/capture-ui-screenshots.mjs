import { chromium } from 'playwright'
import { spawn } from 'node:child_process'
import { mkdir, writeFile } from 'node:fs/promises'
import path from 'node:path'
import process from 'node:process'

const frontendRoot = process.cwd()
const repoRoot = path.resolve(frontendRoot, '..')
const phase = process.env.UI_REVIEW_PHASE || 'after'
const outRoot = path.resolve(repoRoot, 'tmp', 'ui-review', phase)
const port = Number(process.env.RUST_WATCHER_SCREENSHOT_PORT || 41737)
const url = `http://127.0.0.1:${port}`
const shouldBuild = process.env.UI_REVIEW_SKIP_BUILD !== '1'

const modes = [
  { label: 'Macro', slug: 'macro' },
  { label: 'Meso', slug: 'meso' },
  { label: 'Micro', slug: 'micro' },
  { label: 'Call Flow', slug: 'callflow' },
  { label: 'Data Flow', slug: 'dataflow' },
  { label: 'Types & Impl', slug: 'types' },
]

const viewports = [
  { width: 1600, height: 900, slug: '1600' },
  { width: 1920, height: 1080, slug: '1920' },
]

async function main() {
  await mkdir(outRoot, { recursive: true })
  if (shouldBuild) await run('pnpm', ['build'], { cwd: frontendRoot })

  const server = spawn('cargo', [
    'run',
    '-p',
    'web-server',
    '--',
    'serve',
    '--project',
    './example',
    '--port',
    String(port),
  ], {
    cwd: repoRoot,
    stdio: ['ignore', 'pipe', 'pipe'],
    env: { ...process.env, RUST_LOG: process.env.RUST_LOG || 'web_server=info' },
  })

  const serverLog = []
  server.stdout.on('data', chunk => serverLog.push(chunk.toString()))
  server.stderr.on('data', chunk => serverLog.push(chunk.toString()))

  try {
    await waitForServer(url, 90_000)
    const browser = await chromium.launch({ headless: true })
    const consoleMessages = []
    const pageErrors = []

    try {
      for (const viewport of viewports) {
        const page = await browser.newPage({ viewport })
        page.on('console', message => {
          const text = message.text()
          if (/error|warning|NetworkError|ReferenceError|TypeError/i.test(text)) {
            consoleMessages.push(`[${viewport.slug}] ${message.type()}: ${text}`)
          }
        })
        page.on('pageerror', error => {
          const message = `[${viewport.slug}] pageerror: ${error.message}`
          consoleMessages.push(message)
          pageErrors.push(message)
        })

        await page.goto(url, { waitUntil: 'domcontentloaded' })
        await waitForGraphStable(page)
        await assertGraphPresent(page)

        for (const mode of modes) {
          await clickButton(page, mode.label)
          await page.waitForTimeout(450)
          const file = path.join(outRoot, `example-force-${mode.slug}-${viewport.slug}.png`)
          await page.screenshot({ path: file, fullPage: false })
        }

        await page.close()
      }
    } finally {
      await browser.close()
    }

    await writeFile(path.join(outRoot, 'browser-console.log'), consoleMessages.join('\n') || 'No browser console errors captured.\n')
    await writeFile(path.join(outRoot, 'server.log'), serverLog.join(''))
    await writeNotes(outRoot, consoleMessages)
    if (pageErrors.length) {
      throw new Error(`Browser page errors captured:\n${pageErrors.join('\n')}`)
    }
    console.log(`Screenshots saved to ${path.relative(repoRoot, outRoot)}`)
  } finally {
    server.kill()
  }
}

async function run(command, args, options) {
  await new Promise((resolve, reject) => {
    const child = spawn(command, args, { ...options, stdio: 'inherit' })
    child.on('exit', code => code === 0 ? resolve() : reject(new Error(`${command} ${args.join(' ')} failed with ${code}`)))
    child.on('error', reject)
  })
}

async function waitForServer(targetUrl, timeoutMs) {
  const deadline = Date.now() + timeoutMs
  while (Date.now() < deadline) {
    try {
      const response = await fetch(targetUrl)
      if (response.ok) return
    } catch {
      // server still starting
    }
    await delay(500)
  }
  throw new Error(`Server did not become ready at ${targetUrl}`)
}

async function waitForGraphStable(page) {
  await page.waitForSelector('svg', { timeout: 45_000 })
  await page.waitForFunction(() => {
    const text = document.body.innerText
    return /Ready|Fallback|parser|indexed|Analyzer/i.test(text) && !/Indexing/.test(text)
  }, { timeout: 90_000 }).catch(() => undefined)
  await page.waitForTimeout(1200)
}

async function assertGraphPresent(page) {
  const result = await page.evaluate(() => {
    const graph = document.querySelector('svg')
    if (!graph) return { ok: false, reason: 'No SVG graph element found.' }
    const box = graph.getBoundingClientRect()
    if (box.width <= 0 || box.height <= 0) return { ok: false, reason: 'Graph has zero size.' }
    const text = document.body.innerText
    if (/ReferenceError|TypeError|NetworkError/i.test(text)) return { ok: false, reason: 'Error text found in document.' }
    return { ok: true, reason: '' }
  })
  if (!result.ok) throw new Error(result.reason)
}

async function clickButton(page, name) {
  const button = page.getByRole('button', { name, exact: true }).first()
  if (await button.count()) {
    await button.click()
    return
  }
  await page.getByText(name, { exact: true }).first().click()
}

async function writeNotes(directory, consoleMessages) {
  const note = `# UI Screenshot Review (${phase})

Captured from \`./example\` at ${new Date().toISOString()}.

## Matrix

- Layout: Force graph only
- Modes: Macro, Meso, Micro, Call Flow, Data Flow, Types & Impl
- Viewports: 1600x900, 1920x1080

## Console

${consoleMessages.length ? consoleMessages.map(item => `- ${item}`).join('\n') : '- No browser console errors captured.'}

## Visual notes

- TODO: inspect screenshots and record visible issues.
`
  await writeFile(path.join(directory, 'NOTES.md'), note)
}

function delay(ms) {
  return new Promise(resolve => setTimeout(resolve, ms))
}

main().catch(error => {
  console.error(error)
  process.exit(1)
})
