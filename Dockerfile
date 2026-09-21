FROM node:22.14.0-bookworm-slim AS web
WORKDIR /app/web
COPY web/package*.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

FROM rust:1.98-bookworm AS server
WORKDIR /app
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
COPY migrations ./migrations
RUN cargo build --release --locked -p agentway-server

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && rm -rf /var/lib/apt/lists/* && useradd --create-home --uid 10001 agentway && mkdir /data && chown agentway:agentway /data
COPY --from=server /app/target/release/agentway-server /usr/local/bin/agentway-server
COPY --from=web /app/web/dist /app/web
USER agentway
ENV BIND_ADDR=0.0.0.0:8787 DATABASE_URL=sqlite:///data/agentway.db ASSET_DIR=/app/web PUBLISHING_DIR=/data/publishing BRIDGE_BIND_ADDR=0.0.0.0:8788
EXPOSE 8787
ENTRYPOINT ["agentway-server"]
