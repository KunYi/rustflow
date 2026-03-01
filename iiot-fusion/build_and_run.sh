#!/usr/bin/env bash
# build_and_run.sh - 一鍵編譯全部並執行
set -e
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "════════════════════════════════════════════════"
echo " IIoT Flow Fusion MVP - Build & Run"
echo "════════════════════════════════════════════════"

# ── 確認 wasm32-wasip2 target 已安裝 ──────────────────────────────
echo ""
echo "▶ 確認 wasm32-wasip2 target..."
rustup target add wasm32-wasip2 2>/dev/null && echo "  ✅ wasm32-wasip2 ready"

# ── 編譯 5 個 WASM Node ───────────────────────────────────────────
# nodes/.cargo/config.toml 已設定預設 target = wasm32-wasip2
# 所以這裡只需要 --release，不需要 --target
echo ""
echo "▶ 編譯 WASM Nodes..."
NODES=(source-node node-a node-b node-c sink-node)
for node in "${NODES[@]}"; do
    echo "  [WASM] $node"
    cargo build -p "$node" --target wasm32-wasip2 --release 2>&1
done

# ── 收集 .wasm 到 wasm_out/ ───────────────────────────────────────
mkdir -p "$SCRIPT_DIR/wasm_out"
for node in "${NODES[@]}"; do
    snake=$(echo "$node" | tr '-' '_')
    src="$SCRIPT_DIR/target/wasm32-wasip2/release/${snake}.wasm"
    dst="$SCRIPT_DIR/wasm_out/${snake}.wasm"
    cp "$src" "$dst"
    size=$(du -sh "$dst" | cut -f1)
    echo "  ✅ ${snake}.wasm (${size})"
done

# ── 編譯 Host（native，用 release profile）────────────────────────
echo ""
echo "▶ 編譯 Host (native x86_64)..."
cargo build -p iiot-flow-host --release 2>&1
# cargo build -p iiot-flow-host --profile release-host 2>&1
echo "  ✅ target/release/iiot-flow-host"

# ── 執行 ─────────────────────────────────────────────────────────
echo ""
echo "▶ 執行..."
echo "════════════════════════════════════════════════"
echo ""
"$SCRIPT_DIR/target/release/iiot-flow-host" "$SCRIPT_DIR/wasm_out"
