# ---- build ----
FROM rust:slim AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock* ./
COPY src ./src
RUN cargo build --release

# ---- runtime ----
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/quarity-back /usr/local/bin/quarity-back
COPY --from=builder /app/target/release/ingest /usr/local/bin/quarity-ingest
EXPOSE 8080
ENV BIND_ADDR=0.0.0.0:8080
CMD ["quarity-back"]
