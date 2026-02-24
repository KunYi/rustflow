# IIoT Gateway - Flow/Rule Engine - System Requirements

**專案名稱**：IIoT Gateway - WASM Flow Fusion Engine

**版本**：v1.1 (Golden Spec - Flow Fusion 版)

**日期**：2026-02-24

**設計核心**：所有 Node 皆為標準 .wasm 檔案。使用者設計完流程後，系統自動將**整個 Flow 融合（Fuse / AOT Compile）成單一 .wasm 檔案**，再載入 wasmtime 執行。徹底取消獨立客製 Node 進程與 nng Plugin 代理，實現最高安全性與效能。

## 1. 系統架構概覽（最終版）

- **兩個主要進程**：
  1. **Protocol Drivers**（多個，負責 Tag Layer）
  2. **Rule Engine**（Rust 單一進程，內含 wasmtime VM + Flow Fusion Pipeline）
- **Backend**：Rust + Axum（可與 Rule Engine 同進程或分離）
- **Frontend**：Svelte + shadcn-svelte + Vite

**執行流程圖**

```
Frontend (設計 Graph)
    ↓ (Save)
Backend (儲存 GraphRule + .wasm 檔案)
    ↓ (Deploy)
Rule Engine Fusion Pipeline → 生成單一 Flow.wasm
    ↓
wasmtime 載入 Flow.wasm（單一 Instance）
    ↑
Protocol Drivers ──nng PUSH── TagUpdate
```

## 2. WASM Flow Fusion 機制（全新核心功能）

### 2.1 Fusion Pipeline（Deploy 階段）
當使用者在 Frontend 點擊「Deploy」時，系統執行以下步驟：

1. Backend 驗證 GraphRule（無循環、ports ≤4、所有 node 的 .wasm 已存在）
2. Rule Engine 收集所有 Node 的 .wasm 檔案
3. **自動生成 Glue Code**（Rust guest code）：
   - 內嵌整個 graph 的 routing table（port-based）
   - 呼叫每個 Node 的 `handle(input_port, msg)`
   - 實作 fan_out、switch、binary_combine 等內建邏輯
4. **Fusion 階段**（使用 wasmtime Linker + Component Model 或 wasm-merge + Binaryen）：
   - 把所有 Node .wasm + Glue Code 連結成**單一 Flow.wasm**
   - 可選 AOT 預編譯成 `.cwasm`（Cranelift native code）
5. Rule Engine 載入此單一 Flow.wasm 作為一個獨立 Instance
6. 啟動後，TagUpdate 直接呼叫 Flow.wasm 的 `execute_flow(tag_update)` → 內部一次性跑完整個 graph

### 2.2 運行時執行模型
- 每個 deployed Flow 只對應**一個 wasmtime Instance**
- 單次呼叫 `flow.execute()` 即可處理一筆或一批資料（支援 batching）
- 狀態持久化：整個 Flow 透過 Host Function 統一 `save_state()` / `load_state()`

### 2.3 開發 vs 生產模式
- **開發模式**（Debug）：保留 per-node WASM 載入（方便單獨測試）
- **生產模式**（Deploy）：強制執行 Flow Fusion（最高效能與安全）

## 3. 功能需求

### 3.1 Tag Layer（Protocol Driver）－不變
- Raw data → `TagUpdate` Protobuf → nng PUSH 到 Rule Engine

### 3.2 Rule Engine - WASM Flow Engine

#### 3.2.1 Limited Multi-Port Graph Rule
- 每個 Node 嚴格 **最多 4 inputs + 最多 4 outputs**
- 複雜邏輯必須靠 Node 組合（fan_out + switch + binary_combine 等）

#### 3.2.2 NodeDef（Protobuf）
```proto
message PortDef {
  string name = 1;   // "in1", "true", "out_temp", "agg_result"
  string type = 2;   // "tag", "message", "any"
}

message NodeDef {
  string node_type = 1;
  string wasm_hash = 2;           // 唯一識別 .wasm
  repeated PortDef inputs = 3;    // ≤ 4
  repeated PortDef outputs = 4;   // ≤ 4
  map<string, google.protobuf.Value> props = 5;
}

message Wire {
  string source_node = 1;
  string source_port = 2;   // 必須指定
  string target_node = 3;
  string target_port = 4;   // 必須指定
}
```

#### 3.2.3 內建 Node（皆預先編譯成 .wasm）
1. tag_input
2. filter (1→2)
3. switch (1→max4)
4. fan_out (1→max4)
5. binary_combine (2→1)
6. window
7. agg
8. 各種 Sink（mqtt/http/file）

#### 3.2.4 WASM Interface 標準（所有 Node 必須實作）
```wit
// flow-node.wit
interface node {
  handle: func(input_port: string, msg_ptr: u32, msg_len: u32) -> list<u8>;  // 回傳 (output_port, msg) 序列化
  save_state: func() -> list<u8>;
  load_state: func(state: list<u8>);
}
```

### 3.3 Custom Node 開發流程
1. 使用者用 Rust/Go/C/AssemblyScript 撰寫 Node
2. 編譯成標準 .wasm（遵守上述 WIT）
3. Frontend 上傳 .wasm → Backend 驗證 + 儲存
4. 使用時與 Built-in Node 完全一樣（Drag & Drop）

### 3.4 Backend 與 Frontend
- Backend：負責 .wasm 儲存、版本管理、Fusion 觸發
- Frontend：上傳 .wasm、Drag & Drop、Deploy 按鈕（觸發 Fusion）、顯示 Fusion 狀態與執行 Trace

## 4. 非功能需求

### 4.1 效能
- 單機 ≥ 50,000 tags/sec（Flow Fusion 後無 IPC、無多次 WASM call）
- P99 延遲 < 500µs
- 記憶體 < 180MB（單一 Instance per Flow）

### 4.2 安全性（最高等級）
- 整個 Flow 在單一 Sandbox Instance 內執行
- 完全隔離：客製程式碼無法存取 host 資源（除非明確開放 Host Function）
- 即使單一 Node 有惡意程式，也無法影響其他 Flow

### 4.3 可靠性
- Flow 崩潰只影響該 Flow（Rule Engine 可自動重載新 Fusion 版本）
- 熱更新：修改後重新 Fusion + 零停機切換

### 4.4 可擴充性
- 客製 Node 零依賴，只要產出標準 .wasm
- Fusion Pipeline 可擴充支援更多語言（TinyGo、Zig 等）

## 5. 技術棧
- Rule Engine：Rust + wasmtime 3.x（Component Model + Linker） + Binaryen（wasm-merge） + Tokio + Prost
- Fusion Pipeline：Rust guest code generator + wasmtime linker 或 wasm-tools
- Backend：Rust + Axum
- Frontend：Svelte + shadcn-svelte + pnpm + @xyflow/svelte + Vite
- 通訊：僅 nng（Drivers → Rule Engine）

## 6. 驗收標準
- 使用者設計一個 8 個 Node 的複雜流程（含 switch + fan_out + binary_combine），Deploy 後系統自動 Fusion 成單一 Flow.wasm
- 單機 40,000+ tags/sec 壓力測試，Flow 執行時間 < 300µs
- 上傳惡意 WASM（無限迴圈或記憶體爆炸），只影響該 Flow，不影響 Rule Engine 與其他 Flow
- Built-in 與 Custom Node 在 UI 上完全無差別，Fusion 過程對使用者透明
