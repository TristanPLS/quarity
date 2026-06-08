# ---- build ----
FROM rust:slim AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock* ./
COPY src ./src
# migrations/ est lu À LA COMPILATION par la macro sqlx::migrate! (main.rs)
COPY migrations ./migrations
RUN cargo build --release

# ---- runtime ----
FROM debian:bookworm-slim AS runtime
# ca-certificates : TLS sortant (OpenAQ, ClickHouse). curl : healthcheck Docker du service
# back (GET /health) — voir docker-compose.yml.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl \
 && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/quarity-back /usr/local/bin/quarity-back
COPY --from=builder /app/target/release/ingest /usr/local/bin/quarity-ingest
EXPOSE 8080
ENV BIND_ADDR=0.0.0.0:8080
CMD ["quarity-back"]
