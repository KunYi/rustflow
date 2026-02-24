# WASM Node 開發規範

**文件版本**：v1.0

**對應系統版本**：IIoT Gateway WASM Flow Fusion Engine v1.1

**撰寫日期**：2026-02-24

**目的**：定義開發者如何撰寫一個符合本系統標準的 WASM Node，讓它能被 Frontend 拖拉使用，並在 Flow Fusion 階段正確被連結與執行。

## 1. 核心原則

- 每個 Node 必須編譯成**單一標準 .wasm 檔案**
- 必須遵守**統一的 WIT Interface**（Component Model）
- 所有客製 Node 與 Built-in Node 在 Runtime 完全等價
- 開發者**不需要關心 Fusion**，只需產出符合規範的 .wasm 即可
- 安全性：Node 內部完全 Sandbox，僅能透過 Host Functions 與外界互動

## 2. 標準 Interface（flow-node.wit）

所有 Node 必須實作以下 WIT（放在專案 `wit/flow-node.wit`）：

```wit
package iiog:node@1.0.0;

world node {
  // 主要處理函式
  handle: func(
    input_port: string,
    msg: list<u8>          // Protobuf 序列化的 NodeMessage
  ) -> list<u8>;           // 回傳序列化的 [(output_port, NodeMessage), ...]

  // 狀態持久化（可選）
  save_state: func() -> list<u8>;
  load_state: func(state: list<u8>);

  // 選用：初始化
  init: func(config: list<u8>);   // Protobuf NodeConfig
}
```

## 3. 支援語言與工具鏈

| 語言   | 推薦工具鏈 | 編譯指令範例 |  推薦程度  |
|-------|----------|------------|-----------|
| Rust  | cargo-component + wit-bindgen | cargo component build --release  | ★★★★★ |
| Go    | TinyGo + wasi                 | tinygo build -target=wasi -o node.wasm | ★★★★☆ |
| C / C++ | wasi-sdk | clang --target=wasm32-wasi ... | ★★★☆☆ |
| AssemblyScript | asc + component model | asc node.ts -o node.wasm --component | ★★★☆☆ |
| Zig | zig build-exe --target=wasm32-wasi | zig build-exe ... | ★★★☆☆|

強烈推薦使用 Rust（開發體驗最佳，效能最高）

## 4. Host Functions（Rule Engine 提供）
Node 內可直接呼叫以下功能（透過 WIT 自動綁定）：
- log(level: string, message: string)
- send_to_port(port: string, msg: list<u8>) （開發模式用，Fusion 後由 Glue 處理）
- get_tag_meta(tag_id: string) -> list<u8>
- get_config() -> list<u8>
- save_state(state: list<u8>)
- emit_metric(name: string, value: f64)

## 5. Rust 完整開發範例（filter node）

### 5.1 Cargo.toml（使用 component）
```toml
[package]
name = "my-filter"
version = "0.1.0"
edition = "2021"

[dependencies]
wit-bindgen = "0.XX.0"
prost = "0.13"

[lib]
crate-type = ["cdylib"]
```

### 5.2 src/lib.rs

```rust
use wit_bindgen::generate;
use prost::Message;

generate!("flow-node");   // 自動生成 binding

struct MyFilter {
    threshold: f64,
}

impl node::Node for MyFilter {
    fn init(config: Vec<u8>) {
        // 解析 config Protobuf
        let cfg: NodeConfig = NodeConfig::decode(&config[..]).unwrap();
        let threshold = cfg.fields["threshold"].parse::<f64>().unwrap();
        // 儲存到 static 或 thread-local
    }

    fn handle(input_port: String, msg: Vec<u8>) -> Vec<u8> {
        let tag_update: TagUpdate = TagUpdate::decode(&msg[..]).unwrap();

        if let Some(value) = tag_update.value.as_ref() {
            if value.f64_value > threshold {
                // 回傳 true port
                return encode_output("true", tag_update);
            }
        }
        encode_output("false", tag_update)
    }

    fn save_state() -> Vec<u8> { vec![] }
    fn load_state(state: Vec<u8>) {}
}

// 輔助函式
fn encode_output(port: &str, msg: TagUpdate) -> Vec<u8> {
    // 序列化成 [(port, msg), ...] 的 Protobuf
    // ...
}
```

### 5.3 編譯指令
```bash
cargo component build --release
# 輸出：target/wasm32-wasip1/release/my_filter.wasm
```

## 6. 開發流程（完整步驟）
1. Fork 專案提供的 node-template（Rust / Go 皆有）
2. 實作 handle() 與 init()
3. 本地測試（使用 wasmtime run + 測試工具）
4. 編譯成 .wasm
5. Frontend → 「上傳 Custom Node」 → 選擇 .wasm 檔案
6. 填寫 node_type、ports、props schema
7. Deploy Flow 即可看到 Fusion 後的結果

## 7. 驗證與除錯
- 使用 wasmtime run --dir=. node.wasm 本地執行
- 開發模式下可開啟 per-node 載入，方便單步除錯
- Fusion 後 Trace 會顯示「node:xxx → node:yyy」的執行路徑

## 8. 最佳實踐與注意事項
- 避免使用 WASI 檔案系統與網路（除非透過 Host Function）
- 所有狀態必須透過 save_state / load_state 保存
- 輸入輸出務必使用 Protobuf（與系統一致）
- 單一 Node 執行時間建議 < 100µs（Fusion 後會更嚴格）
- Node 命名建議：custom:xxx-v1（版本號便於更新）
- 不要在 Node 內部開 thread（wasmtime 預設單執行緒）

## 9. 參考資源專案
- node-templates/ 目錄（Rust / Go 範例）
- WIT 標準：wit/flow-node.wit
- wasmtime Component Model 文件：https://docs.wasmtime.dev/
- cargo-component 使用指南：https://github.com/bytecodealliance/cargo-component

