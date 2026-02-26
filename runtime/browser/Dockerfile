FROM mcr.microsoft.com/playwright:v1.58.2-noble

USER root

ARG PW_VERSION=1.59.0-alpha-1770338664000
ARG PW_VERSION_STABLE=1.58.2
ENV PLAYWRIGHT_BROWSERS_PATH=/ms-playwright

RUN apt-get update \
  && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    fluxbox \
    novnc \
    websockify \
    x11vnc \
    xterm \
  && rm -rf /var/lib/apt/lists/*

RUN npx -y "playwright@${PW_VERSION_STABLE}" install chromium \
  && npx -y "playwright@${PW_VERSION}" install chromium

COPY entrypoint.sh /usr/local/bin/surf-browser-entrypoint.sh

RUN chmod +x /usr/local/bin/surf-browser-entrypoint.sh \
  && mkdir -p /home/pwuser/.vnc /home/pwuser/.playwright-mcp-profile \
  && chown -R pwuser:pwuser /home/pwuser/.vnc /home/pwuser/.playwright-mcp-profile

USER pwuser

EXPOSE 8931 6080 5900

ENTRYPOINT ["/usr/local/bin/surf-browser-entrypoint.sh"]
