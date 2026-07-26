# Main

This app takes books written in markdown format, compiles them to epub via pandoc and emails them to a Kindle email address.

## Architecture

Frontend:
- /frontend
- Sveltekit 5 (with runes syntax)
- TypeScript
- pnpm
- static SPA target, no sveltekit backend

Backend:
- /backend
- Rust
- API endpoints from /api
- All else falls back to the SPA

## Backend

Do not edit cargo.toml - add or remove packages via `cargo add` and `cargo remove`

## Frontend

- Use runes, Sveltekit 5 syntax
- Use TypeScript types, no 'any'
- Use `pnpm add` and `pnpm remove` for packages, don't edit package.json for them