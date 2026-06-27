# rust-watcher frontend

React + TypeScript + Vite frontend for the rust_watcher live code graph.

## Setup

```bash
pnpm install
```

## Development

```bash
pnpm dev
```

The frontend expects the backend to provide the HTTP API under `/api` and the live graph WebSocket at `/ws`.

## Build

```bash
pnpm build
```

## Tests

```bash
pnpm test:run
```

## UI screenshot review

Use the screenshot workflow before merging graph UI/layout changes:

```bash
pnpm screenshots
```

The script builds the frontend, starts the Rust backend against `./example`, opens the app with Playwright Chromium, and captures the graph matrix into:

```text
tmp/ui-review/after/
```

For before/after comparisons:

```bash
UI_REVIEW_PHASE=before pnpm screenshots
UI_REVIEW_PHASE=after pnpm screenshots
```

Review the force graph across Architecture, Modules, Local Symbol, Call Flow, API/Data Flow, and Types & Impl at 1600x900 and 1920x1080. Notes and browser console output are written next to the screenshots.

## Optional TypeScript semantic analysis

The backend can analyze TypeScript and JavaScript with the parser alone, so the graph still works without extra tools.

For semantic diagnostics, references, definitions, and type definitions, install the local language server dependencies:

```bash
pnpm add -D typescript typescript-language-server
```

After installation, the backend auto-detects `node_modules/.bin/typescript-language-server` when running in `--typescript-analyzer auto` mode.
