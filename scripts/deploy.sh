#!/usr/bin/env bash
# Deploy crew + app-skill binaries to Cloud Mac Minis.
# Usage: ./scripts/deploy.sh [1|2|all]
#   1   = deploy to Mac Mini 1 only (69.194.3.128)
#   2   = deploy to Mac Mini 2 only (69.194.3.129)
#   all = deploy to both (default)
set -euo pipefail

# --- Targets ---
HOST_1="cloud@69.194.3.128"
PW_1="zjsgf128"
HOST_2="cloud@69.194.3.129"
PW_2="vbasx129"

TARGET="${1:-all}"
REMOTE_BIN="/Users/cloud/.cargo/bin"
PLIST="io.ominix.crew-serve"
BINARIES=(crew news_fetch deep-search deep_crawl send_email account_manager asr clock weather)

ssh_cmd() {
    local idx=$1; shift
    local host pw
    eval "host=\$HOST_$idx; pw=\$PW_$idx"
    sshpass -p "$pw" ssh -o PubkeyAuthentication=no "$host" "$@"
}
scp_cmd() {
    local idx=$1; shift
    local host pw
    eval "host=\$HOST_$idx; pw=\$PW_$idx"
    sshpass -p "$pw" scp -o PubkeyAuthentication=no "$@"
}
get_host() {
    eval "echo \$HOST_$1"
}

# Determine which targets to deploy to
case "$TARGET" in
    1)   TARGETS="1" ;;
    2)   TARGETS="2" ;;
    all) TARGETS="1 2" ;;
    *)   echo "Usage: $0 [1|2|all]"; exit 1 ;;
esac

# --- Build ---
echo "==> Building release binaries..."
cargo build --release -p crew-cli --features telegram,whatsapp,feishu,twilio,api
cargo build --release -p news_fetch -p deep-search -p deep-crawl -p send-email -p account-manager -p asr -p clock -p weather

# Build ominix-api if source is available
OMINIX_DIR="${OMINIX_DIR:-$HOME/home/ominix-api}"
if [ -d "$OMINIX_DIR" ]; then
    echo "==> Building ominix-api..."
    (cd "$OMINIX_DIR" && cargo build --release -p ominix-api)
    codesign -s - "$OMINIX_DIR/target/release/ominix-api" 2>/dev/null || true
fi

echo "==> Signing binaries locally..."
for bin in "${BINARIES[@]}"; do
    codesign -s - "target/release/$bin" 2>/dev/null || true
done

# --- Deploy to each target ---
for idx in $TARGETS; do
    REMOTE=$(get_host "$idx")
    echo ""
    echo "========================================"
    echo "==> Deploying to Mac Mini $idx ($REMOTE)"
    echo "========================================"

    echo "==> Uploading binaries..."
    for bin in "${BINARIES[@]}"; do
        echo "    $bin"
        scp_cmd "$idx" "target/release/$bin" "$REMOTE:/tmp/${bin}.new"
    done

    # Upload ominix-api if built
    if [ -d "$OMINIX_DIR" ] && [ -f "$OMINIX_DIR/target/release/ominix-api" ]; then
        echo "    ominix-api"
        scp_cmd "$idx" "$OMINIX_DIR/target/release/ominix-api" "$REMOTE:/tmp/ominix-api.new"
        if [ -f "$OMINIX_DIR/target/release/mlx.metallib" ]; then
            echo "    mlx.metallib"
            scp_cmd "$idx" "$OMINIX_DIR/target/release/mlx.metallib" "$REMOTE:/tmp/mlx.metallib.new"
        fi
    fi

    echo "==> Stopping launchd service..."
    ssh_cmd "$idx" "launchctl unload ~/Library/LaunchAgents/${PLIST}.plist 2>/dev/null || true"
    sleep 1
    ssh_cmd "$idx" "pkill -f 'crew serve' 2>/dev/null || true; pkill -f 'crew gateway' 2>/dev/null || true"
    sleep 1

    echo "==> Replacing binaries on remote..."
    for bin in "${BINARIES[@]}"; do
        ssh_cmd "$idx" "mv /tmp/${bin}.new ${REMOTE_BIN}/${bin} && codesign --force -s - ${REMOTE_BIN}/${bin}"
    done

    # Replace ominix-api if uploaded
    if ssh_cmd "$idx" "[ -f /tmp/ominix-api.new ]" 2>/dev/null; then
        echo "==> Replacing ominix-api on remote..."
        ssh_cmd "$idx" "launchctl unload ~/Library/LaunchAgents/io.ominix.ominix-api.plist 2>/dev/null || true; sleep 1"
        ssh_cmd "$idx" "mv /tmp/ominix-api.new ${REMOTE_BIN}/ominix-api && codesign --force -s - ${REMOTE_BIN}/ominix-api"
        if ssh_cmd "$idx" "[ -f /tmp/mlx.metallib.new ]" 2>/dev/null; then
            ssh_cmd "$idx" "mv /tmp/mlx.metallib.new ${REMOTE_BIN}/mlx.metallib"
        fi
        # Create launchd plist for ominix-api if it doesn't exist
        ssh_cmd "$idx" 'if [ ! -f ~/Library/LaunchAgents/io.ominix.ominix-api.plist ]; then
cat > ~/Library/LaunchAgents/io.ominix.ominix-api.plist << '"'"'PEOF'"'"'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>io.ominix.ominix-api</string>
    <key>ProgramArguments</key>
    <array>
        <string>/Users/cloud/.cargo/bin/ominix-api</string>
        <string>--port</string>
        <string>8080</string>
        <string>--models-dir</string>
        <string>/Users/cloud/.OminiX/models</string>
    </array>
    <key>KeepAlive</key>
    <true/>
    <key>RunAtLoad</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/Users/cloud/.ominix/api.log</string>
    <key>StandardErrorPath</key>
    <string>/Users/cloud/.ominix/api.log</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>PATH</key>
        <string>/Users/cloud/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin</string>
    </dict>
</dict>
</plist>
PEOF
echo "  ominix-api plist created"
fi'
        ssh_cmd "$idx" "launchctl load ~/Library/LaunchAgents/io.ominix.ominix-api.plist"
        echo "    ominix-api service started"
    fi

    echo "==> Ensuring ffmpeg is installed..."
    ssh_cmd "$idx" 'command -v ffmpeg &>/dev/null && echo "  ffmpeg: OK" || {
        if command -v brew &>/dev/null; then brew install ffmpeg; else echo "  WARN: install ffmpeg manually"; fi
    }' || echo "  WARN: could not check ffmpeg"

    echo "==> Cleaning stale skill dirs (bootstrap recreates them)..."
    for skill in news deep-search deep-crawl send-email account-manager asr clock weather; do
        ssh_cmd "$idx" "rm -rf /Users/cloud/.crew/skills/${skill}" 2>/dev/null || true
    done
    # Also clean bundled-app-skills and platform-skills so bootstrap picks up new binaries
    ssh_cmd "$idx" "rm -rf /Users/cloud/.crew/bundled-app-skills /Users/cloud/.crew/platform-skills" 2>/dev/null || true

    # Check ASR/TTS models
    echo "==> Checking voice models..."
    ssh_cmd "$idx" "ls -d ~/.OminiX/models/qwen3-asr-1.7b 2>/dev/null && echo '  ASR model: OK' || echo '  WARN: ASR model not found'"
    ssh_cmd "$idx" "ls -d ~/.OminiX/models/qwen3-tts 2>/dev/null && echo '  TTS model: OK' || echo '  WARN: TTS model not found'"

    echo "==> Starting launchd service..."
    ssh_cmd "$idx" "launchctl load ~/Library/LaunchAgents/${PLIST}.plist"

    echo "==> Verifying..."
    sleep 2
    ssh_cmd "$idx" "launchctl list | grep crew || echo 'WARNING: service not found'"
    echo "==> Mac Mini $idx deploy complete."
done

echo ""
echo "All deployments complete."
