import { chromium } from 'playwright'
import { mkdir, writeFile } from 'node:fs/promises'
import path from 'node:path'
import process from 'node:process'

const frontendRoot = process.cwd()
const repoRoot = path.resolve(frontendRoot, '..')
const outRoot = path.resolve(repoRoot, process.env.CLOUD_SCREENSHOT_DIR ?? 'tmp/cloud-ui-review')
const baseUrl = process.env.CLOUD_SCREENSHOT_URL ?? 'http://127.0.0.1:41808'
const archivePath = process.env.CLOUD_SCREENSHOT_ARCHIVE
const existingWorkspaceId = process.env.CLOUD_SCREENSHOT_WORKSPACE
const username = process.env.CLOUD_SCREENSHOT_USERNAME ?? 'admin'
const password = process.env.CLOUD_SCREENSHOT_PASSWORD ?? 'dev-password'

if (!archivePath && !existingWorkspaceId) {
  throw new Error('CLOUD_SCREENSHOT_ARCHIVE or CLOUD_SCREENSHOT_WORKSPACE is required')
}

const browser = await chromium.launch({ headless: true })
const context = await browser.newContext({ viewport: { width: 1920, height: 1080 } })
const page = await context.newPage()
const consoleMessages = []
page.on('console', message => {
  if (/error|warning|ReferenceError|TypeError/i.test(message.text())) {
    consoleMessages.push(`${message.type()}: ${message.text()}`)
  }
})
page.on('pageerror', error => consoleMessages.push(`pageerror: ${error.message}`))

await mkdir(outRoot, { recursive: true })

try {
  await page.goto(`${baseUrl}/?mode=cloud`, { waitUntil: 'networkidle' })
  await shot('01-login')

  await page.getByPlaceholder('Username').fill(username)
  await page.getByPlaceholder('Password').fill(password)
  await page.getByRole('button', { name: 'Continue' }).click()
  await page.getByRole('heading', { name: 'Workspaces', exact: true }).waitFor()
  let workspaceId = existingWorkspaceId
  if (!workspaceId) {
    await shot('02-workspaces-empty')

    await page.locator('section').getByRole('button', { name: 'New analysis' }).click()
    await page.getByText('Analyze your codebase', { exact: true }).waitFor()
    await shot('03-new-analysis')

    const uploadResponse = page.waitForResponse(response => response.url().endsWith('/api/cloud/upload') && response.request().method() === 'POST')
    await page.locator('input[type=file]').setInputFiles(archivePath)
    const upload = await uploadResponse
    if (!upload.ok()) throw new Error(`Cloud upload failed: ${await upload.text()}`)
    const payload = await upload.json()
    workspaceId = payload.workspaceId
    await page.getByText(/Preparing analysis|Job /).waitFor({ timeout: 15_000 })
    await shot('04-analysis-progress')

    await page.waitForFunction(async jobId => {
      const token = localStorage.getItem('rust-watcher-cloud-session')
      const response = await fetch(`/api/cloud/jobs/${jobId}`, { headers: { Authorization: `Bearer ${token}` } })
      if (!response.ok) return false
      const job = await response.json()
      if (job.status === 'failed' || job.status === 'cancelled') throw new Error(job.message ?? `Cloud job ${job.status}`)
      return job.status === 'completed'
    }, payload.jobId, { timeout: 240_000 })
  }

  await page.goto(`${baseUrl}/?mode=cloud&workspace=${encodeURIComponent(workspaceId)}&tab=graph`, { waitUntil: 'domcontentloaded' })
  await page.locator('main, svg, canvas').first().waitFor({ timeout: 120_000 }).catch(() => undefined)
  await page.waitForTimeout(2_000)
  await shot('05-project-map')

  const views = [
    [/^Matrix\b/, '06-dependency-matrix'],
    [/^Neighborhood\b/, '07-neighborhood'],
    [/^Call Flow\b/, '08-call-flow'],
    [/^API\/Data\b/, '09-api-data-flow'],
    [/^Hotspots\b/, '10-hotspots'],
    [/^Module\b/, '10a-module'],
    [/^Raw\b/, '10b-raw-graph'],
  ]
  for (const [label, name] of views) {
    const button = page.getByRole('button', { name: label }).first()
    if (await button.count()) {
      await button.click()
      await page.waitForTimeout(1_200)
      await shot(name)
    }
  }

  await page.getByText('Workspaces', { exact: true }).first().click()
  await page.getByRole('heading', { name: 'Workspaces', exact: true }).waitFor()
  await shot('11-workspaces-ready')

  await page.getByText('Account', { exact: true }).first().click()
  await page.getByRole('heading', { name: 'Account settings' }).waitFor()
  await shot('12-account')
} finally {
  await writeFile(path.join(outRoot, 'browser-console.log'), consoleMessages.join('\n') || 'No browser console warnings or errors captured.\n')
  await browser.close()
}

async function shot(name) {
  await page.screenshot({ path: path.join(outRoot, `${name}-1920x1080.png`), fullPage: false })
}
