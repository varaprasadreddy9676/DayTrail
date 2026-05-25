# Contributing

Thanks for helping make DayTrail reliable. This project handles sensitive local work metadata, so changes should be conservative and well tested.

## Development Setup

```bash
npm ci --prefix apps/desktop
npm run release:check
```

## Before Opening a PR

- Run focused tests for the files you changed.
- Run `npm run release:check` before marking work ready.
- Do not commit generated bundles, `node_modules`, Rust `target` directories, local databases, logs, or captured exports.
- Do not add mock data to production UI paths.
- Keep privacy behavior explicit: if a control is visible, it must work or be clearly read-only.

## Code Style

- Rust code should be formatted with `cargo fmt`.
- TypeScript/React should pass `npm --prefix apps/desktop run check`.
- Prefer source-backed state over hardcoded placeholder UI.
- For new capture sources, document exactly what is captured and what is redacted.

## Licensing

Unless stated otherwise, contributions are licensed as MIT OR Apache-2.0.

