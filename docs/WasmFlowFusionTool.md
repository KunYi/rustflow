# WASM Flow Fusion Pipeline 工具選型與整合指南

**文件版本**：v1.0

**對應系統版本**：IIoT Gateway WASM Flow Fusion Engine v1.1

**撰寫日期**：2026-02-24

**目的**：讓後續開發者快速了解「將多個 Node .wasm + Glue Code 融合成單一 Flow.wasm」的工具選型與實作方式。

## 1. 為什麼需要 Fusion？

在 v1.1 架構中，我們希望：
- 使用者設計完 Graph 後，**整個 Flow（含所有 Nodes）** 融合成**單一 .wasm 檔案**
- 運行時只載入一個 wasmtime Instance
- 達到最高安全性（單一 Sandbox）與極致效能（無多次 WASM call、無 port routing overhead）

這需要一個 **Composition / Linking 工具** 在 Deploy 階段自動完成「多 .wasm → 單一 Flow.wasm」的工作。

## 2. 可用工具比較（2026 年 2 月最新）

| 工具                  | 成熟度 | Component Model 支援 | 動態生成能力 | Rust API 整合 | 推薦指數 | 備註 |
|-----------------------|--------|----------------------|--------------|---------------|----------|------|
| **WAC (wac-cli)**    | ★★★★★ | 原生最佳             | 優秀（.wac 檔） | 極佳（CLI + library） | ★★★★★ | **強烈推薦** |
| **wasm-tools compose** | ★★★★☆ | 良好                 | 良好         | 優秀          | ★★★★☆   | 官方底層工具 |
| **Binaryen wasm-merge** | ★★★☆☆ | 一般（需手動）       | 較差         | 一般          | ★★★☆☆   | 低階備用 |
| **wasm-pack / cargo-component** | ★★★★ | 開發端好             | 不適合 Fusion | -             | ★★☆☆☆   | 只適合單 Node |

**結論**：**首選 WAC**，次選 `wasm-tools compose`。兩者皆來自 Bytecode Alliance，穩定且活躍。

## 3. 推薦工具：WAC (WebAssembly Compositions)

- GitHub：https://github.com/bytecodealliance/wac
- 官方文件：https://bytecodealliance.github.io/wac/

### 安裝方式
```bash
# 方法一（推薦）
cargo install wac-cli

# 方法二（使用 cargo-binstall，更快）
cargo binstall wac-cli
```

### 基本指令範例
```bash
# 最簡單的 composition
wac plug main.wasm --plug filter.wasm --plug switch.wasm -o flow.wasm

# 使用 .wac 描述檔（我們專案推薦方式）
wac compose flow.wac -o flow.wasm
```

### .wac 描述檔範例（我們會自動生成）
```wac
package my-flow:1.0;

world flow {
  import tag-input: func(msg: list<u8>) -> list<u8>;
  import filter: func(input: list<u8>) -> list<u8>;
  import switch: func(input: list<u8>) -> list<u8>;

  export execute: func(tag_update: list<u8>) -> list<u8>;
}
```

## 4. 在本專案中的整合方式

### 4.1 Fusion Pipeline 完整步驟（Deploy 時執行）
1. Frontend → POST /deploy 傳 GraphRule
2. Backend 驗證 Graph + 確認所有 .wasm 已存在
3. Rule Engine 接收 GraphRule：
	- 動態生成 Glue Code（Rust guest）→ 編譯成 glue.wasm
	- 自動產生 flow.wac 描述檔（根據 Graph 的 nodes + wires + ports
	- 呼叫 wac compose 或 Rust wac library 生成 flow-{flow_id}.wasm
	- 可選：wasmtime compile flow.wasm -o flow.cwasm（AOT）
4. Rule Engine 載入此單一 .wasm / .cwasm 作為 Flow Instance
5. 後續每筆 TagUpdate 只呼叫一次 flow.execute(tag_update)

### 4.2 Rust 程式碼呼叫範例（未來會放在 fusion.rs）
```rust
use wac::Composer;

async fn fuse_flow(graph: GraphRule) -> Result<Vec<u8>, FusionError> {
    let mut composer = Composer::new();

    // 加入所有 Node
    for node in &graph.nodes {
        composer.plug(&node.wasm_hash, &load_wasm(&node.wasm_hash)?);
    }

    // 加入 Glue
    let glue = generate_glue_wasm(&graph)?;
    composer.plug("glue", &glue);

    // 執行 composition
    let flow_wasm = composer.compose("flow")?;

    // AOT 優化
    let cwasm = wasmtime::compile(&flow_wasm)?;
    Ok(cwasm)
}
```

## 5. 進階優化
- AOT 預編譯：wasmtime compile → .cwasm（啟動更快，CPU 使用更低）
- wasm-opt：Fusion 後執行 wasm-opt -O4 flow.wasm -o flow-opt.wasm
- WASI Virt：移除不必要的 WASI imports，讓 Flow.wasm 完全 sandbox
- 快取機制：相同 Graph hash 直接重用已 Fusion 的 .wasm

## 6. 開發模式 vs 生產模式

| 模式           | Fusion 方式                  | 優點                           | 使用時機         |
|----------------|------------------------------|--------------------------------|------------------|
| 開發 (Debug)   | Per-Node 獨立載入            | 方便單 Node 除錯、熱重載       | 本地開發、測試   |
| 生產 (Release) | 強制 Flow Fusion + AOT       | 最高安全性、最高效能、最小記憶體 | 正式部署、上線   |

## 7. 參考連結

- WAC Github：https://github.com/bytecodealliance/wac
- wasm-tools：https://github.com/bytecodealliance/wasm-tools
- wasmtime Component Model：https://docs.wasmtime.dev/
- Bytecode Alliance Component Model 教學：https://component-model.bytecodealliance.org/
