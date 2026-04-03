FROM rust:1-bookworm AS surf-browser-entrypoint-build

WORKDIR /workspace

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

RUN cargo build --release --locked -p surf --bin surf-browser-entrypoint

FROM mcr.microsoft.com/playwright:v1.58.2-noble

USER root

ENV PLAYWRIGHT_BROWSERS_PATH=/ms-playwright

RUN apt-get update \
  && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    fluxbox \
    novnc \
    websockify \
    x11vnc \
    xterm \
  && rm -rf /var/lib/apt/lists/*

COPY --from=surf-browser-entrypoint-build /workspace/target/release/surf-browser-entrypoint /usr/local/bin/surf-browser-entrypoint

RUN chmod +x /usr/local/bin/surf-browser-entrypoint \
  && mkdir -p /home/pwuser/.vnc /home/pwuser/.playwright-mcp-profile \
  && chown -R pwuser:pwuser /home/pwuser/.vnc /home/pwuser/.playwright-mcp-profile

USER pwuser

EXPOSE 8931 6080 5900

ENTRYPOINT ["/usr/local/bin/surf-browser-entrypoint"]
