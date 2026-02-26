#!/usr/bin/env bash
set -euo pipefail

DISPLAY_NUM="${DISPLAY_NUM:-99}"
DISPLAY=":${DISPLAY_NUM}"
XVFB_WHD="${XVFB_WHD:-1920x1080x24}"
MCP_PORT="${MCP_PORT:-8931}"
VNC_PORT="${VNC_PORT:-5900}"
NOVNC_PORT="${NOVNC_PORT:-6080}"
VNC_PASSWORD="${VNC_PASSWORD:-surf}"
MCP_VERSION="${MCP_VERSION:-0.0.64}"
PROFILE_DIR="${PROFILE_DIR:-/home/pwuser/.playwright-mcp-profile}"
ALLOWED_HOSTS="${ALLOWED_HOSTS:-*}"
BROWSER_CHANNEL="${BROWSER_CHANNEL:-chromium}"

export DISPLAY

mkdir -p /home/pwuser/.vnc "$PROFILE_DIR"
x11vnc -storepasswd "$VNC_PASSWORD" /home/pwuser/.vnc/passwd > /dev/null

Xvfb "$DISPLAY" -screen 0 "$XVFB_WHD" -ac +extension RANDR > /tmp/xvfb.log 2>&1 &

SOCKET_PATH="/tmp/.X11-unix/X${DISPLAY_NUM}"
for _ in $(seq 1 50); do
  if [[ -S "$SOCKET_PATH" ]]; then
    break
  fi
  sleep 0.1
done

fluxbox > /tmp/fluxbox.log 2>&1 &

x11vnc \
  -display "$DISPLAY" \
  -rfbport "$VNC_PORT" \
  -rfbauth /home/pwuser/.vnc/passwd \
  -forever \
  -shared \
  -noxdamage \
  -o /tmp/x11vnc.log > /dev/null 2>&1 &

websockify --web /usr/share/novnc/ "$NOVNC_PORT" "localhost:$VNC_PORT" > /tmp/websockify.log 2>&1 &

exec npx -y "@playwright/mcp@${MCP_VERSION}" \
  --host 0.0.0.0 \
  --allowed-hosts "$ALLOWED_HOSTS" \
  --browser "$BROWSER_CHANNEL" \
  --port "$MCP_PORT" \
  --user-data-dir "$PROFILE_DIR"
