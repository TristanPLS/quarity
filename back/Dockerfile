# ---- build ----
FROM rust:slim AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock* ./
COPY src ./src
# migrations/ est lu À LA COMPILATION par la macro sqlx::migrate! (main.rs)
COPY migrations ./migrations
# benches/ : Cargo.toml declare [[bench]] matching -> cargo VALIDE l'existence du
# fichier au parse du manifeste, meme pour `cargo build --release` (qui ne le COMPILE
# pas). Sans cette copie, le build Docker echoue (« can't find `matching` bench »).
COPY benches ./benches
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
# Durcissement : conteneur NON-ROOT. uid dédié sans privilège — le binaire écoute sur
# 8080 (port non privilégié), ne sert que des migrations EMBARQUÉES (compile-time) et
# n'écrit sur aucun volume ; idem pour quarity-ingest. curl (healthcheck) reste dispo.
RUN useradd --system --no-create-home --uid 10001 quarity
USER quarity
EXPOSE 8080
ENV BIND_ADDR=0.0.0.0:8080
CMD ["quarity-back"]
