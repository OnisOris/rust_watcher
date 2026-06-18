# Rust Code Command Center

Local browser app for exploring Rust and React/TypeScript projects as a live code graph.

![Rust Code Command Center](docs/rust-code-command-center.png)

## What It Does

- Builds an interactive graph of crates, files, modules, symbols, calls, traits, impls, React components, hooks, and API routes.
- Connects frontend API calls to Rust backend endpoints when both sides are in the same project.
- Provides graph modes for macro/meso/micro views, call flow, data flow, and trait/impl relationships.
- Includes depth filters, focus bubbles, graph clarity presets, light/dark themes, and layout tuning.

## Setup

Install and build the frontend:

```bash
cd frontend
pnpm install
pnpm build
```

Build the Rust workspace:

```bash
cargo build
```

## Run A Project

Index any local project and open the browser UI:

```bash
cargo run -p web-server -- serve --project /path/to/rust/project --open
```

If `--project` is omitted, the server indexes the current working directory.
The default host is `127.0.0.1`; `--port 0` picks a free local port.

## Frontend Development

Run the backend on the Vite proxy port, then start Vite:

```bash
cargo run -p web-server -- serve --project /path/to/rust/project --port 34127
cd frontend
pnpm dev
```

Override the backend proxy target with `VITE_BACKEND_URL` when needed.

## API

- `GET /api/health`
- `GET /api/status`
- `GET /api/graph/snapshot?mode=Macro`
- `GET /api/node/:id`
- `GET /api/search?q=query`
- `POST /api/focus`
- `POST /api/project/open`
- `GET /ws`

The app never exposes file mutation, deletion, shell execution, or write APIs.
