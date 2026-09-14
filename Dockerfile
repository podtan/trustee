# Trustee — multi-stage build (k8s deployment shape).
#
#   build:  docker build -t trustee:local .
#   run:    docker run -p 3000:3000 trustee:local
#
# Stage 1 compiles the release binary with the requested cargo features
# (default "web" — the HTTP/WebSocket server; the crate's default features
# are deliberately EMPTY, so the feature flag is load-bearing).
#
# Stage 2 is a minimal runtime: the binary + TLS roots + git (checkpoints)
# + curl (healthcheck), a non-root `trustee` user whose ~/.trustee carries
# the baked default config, and the self-signed-HTTPS healthcheck
# (curl -k: the server generates its own certificate at boot).

# ── Stage 1 — build ─────────────────────────────────────────────────────
FROM rust:trixie AS build
ARG FEATURES="web"
WORKDIR /build
COPY . .
RUN cargo build --release --features ${FEATURES}

# ── Stage 2 — runtime ───────────────────────────────────────────────────
FROM debian:trixie-slim

# ca-certificates: TLS roots for curl; git: checkpoint/mirror backend;
# curl: the healthcheck probe.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates git curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /build/target/release/trustee /usr/local/bin/trustee

# Non-root runtime user; HOME owns the baked default environment.
RUN useradd --create-home --home-dir /home/trustee --shell /usr/sbin/nologin trustee \
    && mkdir -p /home/trustee/.trustee/config \
                 /home/trustee/.trustee/tokens \
                 /home/trustee/.trustee/sessions \
    && chown -R trustee:trustee /home/trustee

ENV HOME=/home/trustee
USER trustee

# Bake the default config into /home/trustee/.trustee (runs as trustee, so
# everything it creates is already owned by trustee), then make sure the
# runtime directories exist no matter what init version created.
RUN trustee init --force \
    && mkdir -p /home/trustee/.trustee/config \
                /home/trustee/.trustee/tokens \
                /home/trustee/.trustee/sessions

EXPOSE 3000
ENTRYPOINT ["trustee"]
CMD ["web", "--addr", "0.0.0.0:3000"]

# The server serves SELF-SIGNED HTTPS by default — hence -k, and https
# (not http: the plain listener only exists under --no-tls).
HEALTHCHECK --interval=30s --timeout=5s \
    CMD curl -kfsS https://127.0.0.1:3000/api/v1/health || exit 1
