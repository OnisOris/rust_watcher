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

## Optional TypeScript semantic analysis

The backend can analyze TypeScript and JavaScript with the parser alone, so the graph still works without extra tools.

For semantic diagnostics, references, definitions, and type definitions, install the local language server dependencies:

```bash
pnpm add -D typescript typescript-language-server
```

After installation, the backend auto-detects `node_modules/.bin/typescript-language-server` when running in `--typescript-analyzer auto` mode.
