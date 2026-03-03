# Rust Backend Development Guidelines

## Toolchain & Build

- Use stable Rust (latest stable) and set `edition = "2021"` or newer in Cargo.toml. Pin with `rust-toolchain.toml` to avoid drift.
- Enable strict lints: in `lib.rs`/`main.rs` add `#![deny(unsafe_code)]`, `#![warn(clippy::all, clippy::pedantic, clippy::nursery)]` and `#![warn(rust_2018_idioms)]`.
- Use `cargo fmt` and `cargo clippy --all-targets --all-features` before finishing tasks.
- Run `cargo test`, `cargo build --release`, and `cargo audit` (via `cargo-audit`) on CI.
- Use `cargo deny` to enforce dependency policy and security advisories.
- Prefer `mold` or `lld` for faster linking in dev when available.
- For workspace projects, use Cargo workspaces with `members = ["crates/*", "services/*"]`.
- Use `cargo-watch` in development for fast rebuild/test cycles.
- Enable incremental compilation for dev; disable for release builds.
- Use `RUSTFLAGS="-D warnings"` in CI to fail on warnings.

## Error Handling

- Never use `panic!` for recoverable errors. Return `Result<T, E>` with domain-specific error types.
- Use `thiserror` for error enums and `anyhow` only at application boundaries (CLI, main).
- Implement `From` conversions to wrap lower-level errors with context.
- Use `tracing-error` or `eyre` for rich error context where needed.
- Standardize error mapping to HTTP status codes at the boundary layer.
- Provide user-safe error responses: `{ code, message, details? }`. Never expose internal stack traces.
- Use `std::error::Error` and `source()` for error chaining.

## Async & Concurrency

- Use `tokio` as the default async runtime; configure features in Cargo.toml.
- Prefer `async/await` over manual polling/futures combinators for readability.
- Use `join!`/`try_join!` for parallel tasks; avoid sequential awaits for independent operations.
- Use `tokio::time::timeout` for external calls and long-running tasks.
- For CPU-bound work, use `tokio::task::spawn_blocking` or a dedicated thread pool.
- Avoid shared mutable state; when needed, use `Arc<Mutex<>>` or `Arc<RwLock<>>` with clear ownership.
- Use `CancellationToken` (tokio-util) for structured cancellation.
- Implement graceful shutdown with `signal::ctrl_c()` and drain in-flight requests.

## Type Design & API

- Use strong domain types (newtypes): `struct UserId(Uuid);` to avoid mixing IDs.
- Separate API DTOs from domain models; implement explicit mapping functions.
- Use `serde` with `#[serde(rename_all = "snake_case")]` for consistent JSON.
- Use `validator` or `serde_valid` for input validation at API boundaries.
- Prefer enums for closed sets of states; use `#[serde(tag = "type")]` for discriminated unions.
- Use generics with trait bounds to encode invariants rather than runtime checks.
- Expose traits for services to support dependency injection and testing.

## Web Frameworks

- Prefer `axum` for HTTP APIs (Tokio-first, Tower middleware, type-safe extractors).
- Use `tower` middleware for logging, auth, tracing, rate limiting, and compression.
- Use `serde_json::Value` only at boundaries; convert to typed structs ASAP.
- Keep handlers thin; move business logic into service layer.

## Database & ORM

- Prefer `sqlx` for compile-time checked queries or `sea-orm` for higher-level ORM.
- Use connection pooling with `sqlx::Pool` or `deadpool`.
- Store migrations with `sqlx migrate` or `refinery`. Never edit production schema manually.
- Use transactions for multi-step operations; handle retries for deadlocks.
- Implement cursor-based pagination for large datasets.

## Safety & Security

- Validate all external input and enforce size limits on payloads.
- Use `argon2` or `bcrypt` for password hashing with strong parameters.
- Use JWTs with `jsonwebtoken` or session cookies with secure, httpOnly, SameSite settings.
- Implement CORS with explicit allowlists; never use wildcard with credentials.
- Use `tower::limit` or custom rate limiters (Redis-backed if needed).
- Avoid `unsafe` unless strictly necessary; isolate and document any `unsafe` blocks.
- Redact secrets in logs; never log tokens or passwords.

## Serialization & Data

- Use `serde` for JSON; prefer `serde_with` for custom formats (e.g., ISO8601).
- Serialize dates as RFC 3339 strings (`chrono::DateTime<Utc>`).
- Use `serde_json::to_string` only at boundaries; keep typed structures internally.
- Implement ETag and cache headers for cacheable responses.
- Handle BigInt with string serialization when needed (e.g., `i128` via custom serializer).

## Testing

- Use `cargo test` with unit tests colocated in modules and integration tests in `tests/`.
- Use `rstest` for parameterized tests; use `mockall` for mocks.
- Use `testcontainers` for integration tests with real DBs.
- Test error handling and validation paths explicitly.
- Aim for high coverage on business logic; prioritize correctness over coverage metrics.

## Logging & Observability

- Use `tracing` and `tracing-subscriber` for structured logs.
- Add request IDs and user context to spans. Use `tower-http::trace` for HTTP tracing.
- Export metrics with `metrics` + Prometheus exporter.
- Integrate error tracking (Sentry) via `sentry` crate.
- Implement health checks (e.g., `/healthz`) for readiness/liveness.

## Performance

- Profile before optimizing: use `cargo flamegraph` and `tokio-console`.
- Minimize allocations; prefer `&str` over `String` where possible.
- Use `Bytes`/`BytesMut` for efficient IO buffers.
- Avoid blocking in async contexts; move to blocking threads.
- Use `mimalloc` or `jemalloc` only if proven necessary.
- Enable `lto = true` and `codegen-units = 1` for optimized release builds.

## Dependencies

- Keep dependencies minimal; audit before adding new crates.
- Pin versions in Cargo.lock; use `cargo update` carefully.
- Prefer well-maintained crates with recent releases and good docs.
- Use `cargo deny` to enforce license and security policies.

## Documentation

- Use Rustdoc comments (`///`) for public APIs and types.
- Document configuration in `.env.example` and README.
- Keep ADRs under `docs/adr/` for architectural decisions.
- Document error codes and API responses centrally.

## Code Style

- Run `cargo fmt` (rustfmt) and enforce it in CI.
- Use `snake_case` for variables/functions, `CamelCase` for types, `SCREAMING_SNAKE_CASE` for constants.
- Keep functions small and focused; prefer pure functions when possible.
- Avoid deep nesting; use early returns and `?` for error propagation.
- Prefer composition over inheritance; use traits for extensibility.
- Keep modules organized: `mod.rs` or the 2018 module layout with explicit paths.

## Project Structure

- Use a layered layout: `api/` (handlers), `service/` (business logic), `domain/` (entities), `infra/` (DB, queues), `config/`.
- Keep crates small and focused; split reusable logic into `crates/`.
- Avoid cyclic dependencies between crates; depend down the stack only.
- Expose narrow public APIs; keep most items `pub(crate)`.

## Configuration & Secrets

- Load configuration via `config` or `figment`; validate on startup and fail fast.
- Use environment variables for secrets; never commit `.env`.
- Provide `.env.example` with non-sensitive defaults.
- Separate config per environment (dev/staging/prod) and document differences.

## API Design

- Version public APIs (e.g., `/v1/`) and keep backward compatibility guarantees.
- Define stable error codes and response schemas for clients.
- Use pagination, filtering, and sorting conventions consistently.
- Avoid breaking changes; when unavoidable, deprecate and communicate clearly.

## CI/CD

- Build, test, lint, and audit on every PR.
- Use `cargo fmt --check`, `cargo clippy --all-targets --all-features -D warnings`.
- Cache Cargo registry and target directory for faster CI.
- Run `cargo deny` and `cargo audit` in CI.
- Produce reproducible release builds with locked dependencies.

## Security Practices

- Keep a security checklist for endpoints (auth, rate limit, input limits).
- Store secrets in a dedicated secret manager in production.
- Use TLS everywhere; enforce HSTS and secure headers at the edge.
- Keep dependencies updated and monitor advisories.

## Deployment & Operations

- Prefer stateless services; externalize state to DB/queue/cache.
- Use health checks and readiness probes; fail fast on misconfig.
- Implement graceful shutdown with timeouts.
- Emit structured logs and metrics with consistent labels.

## Code Review Checklist

- Correctness and error handling.
- Concurrency safety and async blocking.
- Security validation and auth checks.
- Observability (logs, metrics, traces).
- Test coverage for critical paths.

## Common Anti-Patterns

- Using `unwrap()`/`expect()` in production code.
- Returning `anyhow::Error` from core library code.
- Cloning data unnecessarily or allocating in hot paths.
- Mixing DTOs with domain models.
- Hidden mutable global state.