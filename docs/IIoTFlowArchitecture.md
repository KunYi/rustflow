# IIoT Flow Engine 系統架構設計文件

**版本：** 0.4.0
**套件名稱：** `iiot:flow`
**最後更新：** 2026-02-26

> **v0.4.0 變更摘要：**
> - 修正 Fusion 概念：DAG Fusion 的本質是「靜態 edge 替換」，不是展開 node 內部邏輯
> - 釐清 Mux/Demux 的決策邏輯屬於 node 內部實作，Fusion 只處理 output port → 下游 node 的靜態呼叫
> - 說明 DAG（fan-out / fan-in）完全可以 fusion，不只是 pipeline
> - 修正設計決策表中對 Mux/Demux fusion 的描述
>
> **v0.3.0 變更摘要：**
> - 新增核心章節：Node 規格化系統（使用者自定義 node 的完整機制）
> - WIT 加入 `node-descriptor` interface，透過 `describe()` 對外暴露 metadata
> - 明確定義所有內建 node 種類的 port 結構與 join 策略
> - 新增跨語言開發指引（Rust / C / C++ / 任何支援 wit-bindgen 的語言）
> - Properties 欄位設計：支援預設常數、型別標註、GUI schema

---

## 目錄

1. [系統概觀](#1-系統概觀)
2. [核心設計原則](#2-核心設計原則)
3. [WIT 型別系統](#3-wit-型別系統)
4. [Node 規格化系統](#4-node-規格化系統)
5. [內建 Node 種類規格](#5-內建-node-種類規格)
6. [自定義 Node 開發](#6-自定義-node-開發)
7. [DAG 設計與資料流](#7-dag-設計與資料流)
8. [DAG 即 WASM Instance](#8-dag-即-wasm-instance)
9. [Deploy Pipeline](#9-deploy-pipeline)
10. [Fusion 優化](#10-fusion-優化)
11. [AOT 編譯](#11-aot-編譯)
12. [Deployable Artifact](#12-deployable-artifact)
13. [Runtime 架構](#13-runtime-架構)
14. [錯誤處理策略](#14-錯誤處理策略)
15. [設計決策彙整](#15-設計決策彙整)

---

## 1. 系統概觀

IIoT Flow Engine 是一套以 **WebAssembly Component Model** 為基礎的工業物聯網資料流處理引擎。使用者透過視覺化拖拉介面設計資料處理 DAG，每個 Node 是一個符合 WIT 規格的 WASM 元件，可以用任何能編譯到 WASM 的語言開發。Deploy 階段自動進行靜態分析、全圖 Fusion 與 AOT 編譯，讓整個 DAG 以**單一 WASM instance** 在 Runtime 執行。

### 1.1 整體架構

```
┌───────────────────────────────────────────────────────────┐
│                    Node 開發（任意語言）                     │
│                                                           │
│  實作 WIT node-spec interface → 編譯成 node.wasm           │
│  describe() 回傳 NodeDescriptor（ports / props / kind）    │
└───────────────────────────────────────────────────────────┘
                     ↓ 上傳到 Node Registry
┌───────────────────────────────────────────────────────────┐
│                   設計階段（GUI / 瀏覽器）                   │
│                                                           │
│  從 Registry 載入 node.wasm → 呼叫 describe()              │
│  依 NodeDescriptor 渲染 port / property 編輯介面            │
│  使用者拖拉連線 → 產生 flow.json                            │
└───────────────────────────────────────────────────────────┘
                     ↓ Deploy
┌───────────────────────────────────────────────────────────┐
│                Deploy Pipeline（CLI / CI）                 │
│                                                           │
│  Parse IR → Validate → Full-DAG Fusion → AOT → Artifact  │
└───────────────────────────────────────────────────────────┘
                     ↓ 執行
┌───────────────────────────────────────────────────────────┐
│                   Runtime（Edge Device）                   │
│                                                           │
│  載入 flow.wasm（單一 instance）→ tick() → 自驅執行          │
└───────────────────────────────────────────────────────────┘
```

### 1.2 核心執行模型

**整個 DAG = 一個 WASM instance。**

Deploy Pipeline 將所有 node 的 WASM module 合併、fusion 成單一 `flow.wasm`，Mux/Demux 展開為 WASM 內部的 `if/block` 控制流，Sink 透過 Host Function 呼叫外部系統，sink-end 透過 Host Function 觸發通知與控制動作。

```
Runtime
  └─ flow.wasm（單一 WASM instance）
       ├─ [source]     → host_recv() → decode protobuf → push
       ├─ [transform]  → 純 WASM 計算，直接記憶體傳遞
       ├─ [mux/demux]  → if/block 控制流（fusion 展開）
       ├─ [join buf]   → WASM 線性記憶體，跨 tick 等待
       ├─ [sink]       → encode protobuf → host_send()
       └─ [sink-end]   → host_notify() / host_trigger()
```

---

## 2. 核心設計原則

| 原則 | 說明 |
|------|------|
| **Node 即 WASM 元件** | 任何語言只要能編譯到 WASM 並實作 WIT 規格，就是合法的 node |
| **Descriptor 驅動** | Node 透過 `describe()` 自我描述 port、property、種類，GUI 與 Pipeline 都依此運作 |
| **DAG 即 Instance** | 整個 DAG fusion 成單一 WASM instance，消除所有 node 間序列化 |
| **薄 Runtime** | Runtime 只管 instance 生命週期、host function 實作與 tick 排程 |
| **Push 模型** | Source 驅動，mux/demux 是 WASM 內部控制流，不是 runtime 路由 |
| **Host Function 作為邊界** | WASM 與外部世界的唯一介面是 Host Function import |
| **編譯期優化** | Validate、Fusion、型別特化、AOT 全部在 Deploy 時完成 |

---

## 3. WIT 型別系統

### 3.1 完整 WIT 定義

```wit
package iiot:flow@0.1.0;

// ── 基礎資料型別 ───────────────────────────────────────────
interface types {

    /// 可攜帶任意工業資料型別的 tagged union
    variant tag-value {
        bool-val(bool),
        i8-val(s8),   u8-val(u8),
        i16-val(s16), u16-val(u16),
        i32-val(s32), u32-val(u32),
        i64-val(s64), u64-val(u64),
        f32-val(f32), f64-val(f64),
        short-str(string),
        blob(list<u8>),
    }

    /// 型別描述符，用於 port 宣告與靜態型別推導
    enum value-kind {
        bool-val, i8-val, u8-val, i16-val, u16-val,
        i32-val, u32-val, i64-val, u64-val,
        f32-val, f64-val, short-str, blob,
        any,    // 不限型別，Deploy 時靜態推導填入具體型別
    }

    /// 資料流的原子單位，對應 SCADA / OPC-UA 的 Tag 模型
    record flow-msg {
        tag-id:    u32,       // 資料點識別碼
        msg-id:    u32,       // 訊息序號（去重 / 追蹤）
        value:     tag-value, // 實際值
        timestamp: u64,       // Unix timestamp（微秒）
        quality:   u8,        // OPC-UA 風格品質碼（0 = 良好）
    }

    /// 單一 output port 的輸出批次
    record port-msgs {
        port-id: u32,
        msgs:    list<flow-msg>,
    }

    /// Node process() 的回傳值（開發階段使用，fusion 後消除）
    record node-output {
        outputs: list<port-msgs>,
    }

    /// sink-end 觸發事件
    record trigger-event {
        event-type: string,
        payload:    list<flow-msg>,
        metadata:   list<tuple<string, string>>,
    }
}

// ── Node Descriptor 型別（describe() 的回傳結構）────────────
interface node-descriptor {
    use types.{value-kind};

    /// Node 的功能種類
    enum node-kind {
        source,     // 從外部產生資料，無 input port
        sink,       // 消費資料送往外部，無 output port
        sink-end,   // 終止節點，觸發 host 事件，無 output port
        transform,  // 通用轉換：N input → M output
        mux,        // 多路選擇：多個 data input + 1 condition input → 1 output
        demux,      // 多路分發：1 input + 1 condition input → 多個 output
        merge,      // 任一 input 到即輸出（OR 語意）
        join,       // 等待所有 input 到齊才輸出（AND / ZIP 語意）
    }

    /// Input port 的 join 觸發策略
    enum join-strategy {
        any,              // 任一 input 有資料即觸發（Merge 語意）
        all,              // 所有 data port 都有資料才觸發（Join / ZIP 語意）
        all-or-initial,   // 有初始值的 port 可用初始值代入，任一更新即觸發
    }

    /// Input port 的角色
    enum port-role {
        data,       // 一般資料
        condition,  // 控制/條件訊號（mux / demux 用）
    }

    /// Input port 定義
    record input-port-def {
        port-id:      u32,
        name:         string,
        kind:         value-kind,
        role:         port-role,
        // all-or-initial 策略時，此欄位為初始值的 JSON 表示
        initial-value: option<string>,
    }

    /// Output port 定義
    record output-port-def {
        port-id: u32,
        name:    string,
        kind:    value-kind,
    }

    /// Property 欄位定義（預先設定的常數，GUI 渲染表單用）
    enum prop-type {
        prop-bool,
        prop-i32,
        prop-u32,
        prop-f32,
        prop-f64,
        prop-string,
        prop-select,   // 下拉選單，choices 欄位提供選項
        prop-json,     // 任意 JSON 結構（進階設定）
    }

    record prop-def {
        key:          string,
        label:        string,          // GUI 顯示名稱
        prop-type:    prop-type,
        default-value: string,         // JSON 序列化的預設值
        required:     bool,
        choices:      list<string>,    // prop-select 時的選項清單
        description:  string,
    }

    /// Node 完整自我描述
    record node-spec {
        // 識別
        name:        string,   // 唯一識別名稱，e.g. "iiot:math-op"
        version:     string,   // semver，e.g. "1.0.0"
        kind:        node-kind,
        // Ports
        inputs:      list<input-port-def>,
        outputs:     list<output-port-def>,
        // Join 策略（適用於 kind = join / transform 有多個 input 時）
        join-strategy: join-strategy,
        // Properties（使用者在 GUI 設定的常數）
        props:       list<prop-def>,
        // UI 元資訊
        label:       string,   // GUI 顯示名稱
        description: string,   // 說明文字
        icon:        string,   // icon 名稱或 SVG data URI
        category:    string,   // GUI 分類，e.g. "math", "logic", "io"
        color:       string,   // GUI 節點顏色，hex e.g. "#4A90D9"
    }
}

// ── Node 執行介面（每個 node WASM 必須實作）──────────────────
interface node-spec-iface {
    use types.{flow-msg, node-output};
    use node-descriptor.{node-spec};

    /// 回傳 node 的完整靜態描述（GUI 與 Deploy Pipeline 呼叫）
    describe: func() -> node-spec;

    /// Runtime 啟動時呼叫，傳入使用者設定的 properties（JSON）
    /// 以及完整的 wiring context（有幾個 upstream/downstream port）
    init: func(props: string, wiring: string) -> result<_, string>;

    /// Push 入口：上游訊息抵達，input-port 指定來自哪個 upstream port
    process: func(input-port: u32, msgs: list<flow-msg>) -> result<node-output, string>;

    /// Source 專用：runtime 定時呼叫
    tick: func() -> result<node-output, string>;
}

// ── Protobuf 編解碼（Source / Sink 實作）──────────────────
interface proto-codec {
    decode: func(schema-id: u32, raw: list<u8>) -> result<list<flow-msg>, string>;
    encode: func(schema-id: u32, msgs: list<flow-msg>) -> result<list<u8>, string>;
}

// ── Host Function：WASM 與外部世界的唯一邊界 ───────────────
interface host-io {
    use types.{trigger-event};

    host-recv:      func(endpoint-id: u32) -> result<list<u8>, string>;
    host-send:      func(endpoint-id: u32, data: list<u8>) -> result<_, string>;
    host-notify:    func(event: trigger-event) -> result<_, string>;
    host-trigger:   func(action-id: u32, params: list<u8>) -> result<_, string>;
    host-timestamp: func() -> u64;
}

// ── Node World：每個 node .wasm 的開發規格 ──────────────────
world flow-node {
    export node-spec-iface;
    export proto-codec;    // source / sink 實作，其他 node 提供空實作
}

// ── Fused DAG World：Deploy 後產出的 flow.wasm ─────────────
world flow-dag {
    import host-io;
    export init:          func(config: string) -> result<_, string>;
    export tick-source-0: func() -> result<_, string>;
    export tick-source-1: func() -> result<_, string>;
    // ... 依 source 數量增加
}
```

### 3.2 兩個 World 的關係

```
開發階段                                Deploy 後
──────────────────────────────────────────────────────────
flow-node world                        flow-dag world
（每個 node 獨立 .wasm）                （整個 DAG 一個 flow.wasm）

node-math.wasm      ──┐
node-mux.wasm       ──┤
node-demux.wasm     ──┤  Full-DAG Fusion  ──▶  flow.wasm
node-proto-src.wasm ──┤                         import: host-io
node-proto-sink.wasm──┤                         export: init
node-sinkend.wasm   ──┘                         export: tick-source-N
```

---

## 4. Node 規格化系統

Node 規格化是本系統讓「任意語言開發、任意功能」的 node 都能被 GUI 和 Deploy Pipeline 正確處理的關鍵機制。

### 4.1 Node 生命週期

```
1. 開發者用任意語言實作 WIT node-spec-iface
   → 編譯成 node.wasm
   → 上傳到 Node Registry

2. GUI 載入 node.wasm
   → 呼叫 describe()
   → 依 node-spec 渲染拖拉元件（port 數量、property 表單、顏色分類）

3. 使用者拖拉設計 DAG
   → 設定每個 node 的 properties
   → 連接 port 間的 edge
   → 產生 flow.json

4. Deploy Pipeline 載入 flow.json 和所有 node.wasm
   → 呼叫每個 node 的 describe() 取得 node-spec
   → 執行 Validate（型別相容、port 對應、join 策略等）
   → Full-DAG Fusion
   → AOT → flow.wasm

5. Runtime 載入 flow.wasm
   → 只認識 init() 和 tick-source-N()
   → node 的個別 describe() 在 runtime 層完全不可見
```

### 4.2 Node Descriptor 的角色

`node-spec` 結構服務兩個消費者，各有側重：

| 欄位 | GUI 使用 | Deploy Pipeline 使用 |
|------|---------|---------------------|
| `kind` | 決定 node 的視覺形狀 | 決定 fusion 策略（mux→if/block） |
| `inputs` / `outputs` | 渲染 port 連接點 | Validate 型別相容性 |
| `input.role` | 標示 condition port（顏色不同） | 識別 mux/demux 的條件 port |
| `join-strategy` | 顯示 join 模式提示 | 決定 join buffer 的觸發邏輯 |
| `props` | 渲染 property 設定表單 | 把使用者設定值傳入 init() |
| `label` / `color` / `icon` | GUI 顯示 | 不使用 |

### 4.3 Properties 設計

Properties 是 node 的**編譯期常數**，由使用者在 GUI 設計階段設定，在 `init()` 時以 JSON 傳入，在 fusion 後燒進 WASM data section 成為真正的常數。

```
Properties vs. Input Port 的差異：

  Properties：
    - 設計階段設定，不隨資料流動態變化
    - 例如：運算子（+/-/*/÷）、threshold 值、output tag-id
    - GUI 渲染成表單

  Input Port：
    - 執行時從上游 node 動態接收
    - 例如：兩個要相加的數值、mux 的選擇訊號
    - GUI 渲染成連接點
```

### 4.4 Wiring Context

`init()` 接收兩個 JSON 參數：

**props**：使用者在 GUI 設定的 property 值
```json
{
  "operator": "+",
  "output-tag-id": 201,
  "initial-a": 0.0
}
```

**wiring**：deploy pipeline 傳入的完整連線資訊，讓 node 知道自己有幾個 upstream/downstream
```json
{
  "node-id": "math-add-1",
  "inputs": [
    { "port-id": 0, "name": "a", "from-node": "src-temp",  "resolved-type": "f32-val" },
    { "port-id": 1, "name": "b", "from-node": "src-press", "resolved-type": "f32-val" }
  ],
  "outputs": [
    { "port-id": 0, "name": "result", "to-nodes": ["sink-1"], "resolved-type": "f32-val" }
  ]
}
```

---

## 5. 內建 Node 種類規格

以下是系統內建的標準 node 規格。使用者自定義 node 可以繼承這些語意，也可以完全自訂。

### 5.1 Math Node（四則運算）

```
kind:          transform
inputs:
  port-0  name="a"  kind=any  role=data
  port-1  name="b"  kind=any  role=data
outputs:
  port-0  name="result"  kind=any
join-strategy: all-or-initial   ← 可選初始值，任一更新即計算

props:
  operator     prop-select  choices=["+","-","*","÷"]  default="+"
  output-tag   prop-u32     default=0
  initial-a    prop-f64     default=0.0   required=false
  initial-b    prop-f64     default=0.0   required=false
```

**join-strategy 的三種模式說明：**

```
all：
  兩個 port 都收到新值才計算
  適合：兩個 source 同步採樣，一定要配對計算

any（不適用 math，但說明語意）：
  任一 port 有新值就輸出（只用最新的值）

all-or-initial：
  port-a 有初始值 10.0，port-b 有初始值 0.0
  → 一開始 result = 10.0 + 0.0 = 10.0
  → port-b 收到新值 5.0 → result = 10.0 + 5.0 = 15.0  ← 只更新 b 就計算
  → port-a 收到新值 20.0 → result = 20.0 + 5.0 = 25.0  ← 只更新 a 就計算
  適合：一個慢變化的設定值 + 一個快變化的感測值
```

**Fusion 後展開：**
```rust
// props 內嵌為常數後
fn math_add_1_inline(a: f32, b: f32) -> f32 {
    a + b   // operator = "+" 已特化，無 match
}
```

### 5.2 Mux Node（多路選擇器）

```
kind:    mux
inputs:
  port-0  name="in-0"  kind=any       role=data
  port-1  name="in-1"  kind=any       role=data
  ...（port 數量由使用者在 GUI 設定，N 個 data port）
  port-N  name="sel"   kind=u8-val    role=condition
outputs:
  port-0  name="out"   kind=any
join-strategy: any   ← condition 或 data 任一更新就重新路由

props:
  data-port-count  prop-u32  default=2  description="data input 數量"
  output-tag       prop-u32  default=0
```

**執行語意：**
```
condition port 收到 sel=1
  → mux 記錄 active_input = 1

data port-1 收到新值 msgs
  → active_input == 1，所以把 msgs 送到 output port-0
  → active_input != 1，丟棄（或 hold）

data port-0 收到新值 msgs
  → active_input == 1，不是 port-0，丟棄
```

**Fusion 後展開為 WASM global + if/block：**
```
(global $mux_1_sel (mut i32) (i32.const 0))

on_condition(msg): global.set $mux_1_sel msg.value.u8
on_data_0(msgs):   if global.get $mux_1_sel == 0: output_inline(msgs)
on_data_1(msgs):   if global.get $mux_1_sel == 1: output_inline(msgs)
```

### 5.3 Demux Node（多路分發器）

```
kind:    demux
inputs:
  port-0  name="in"   kind=any     role=data
  port-1  name="sel"  kind=u8-val  role=condition
outputs:
  port-0  name="out-0"  kind=any
  port-1  name="out-1"  kind=any
  ...（port 數量由使用者設定）
join-strategy: any

props:
  output-port-count  prop-u32  default=2
```

**執行語意：**
```
condition port 收到 sel=2
  → demux 記錄 active_output = 2

data port 收到 msgs
  → 把 msgs 送到 output port-2
  → 其他 output port 不送
```

Demux 是 Mux 的鏡像，同樣展開為 WASM global + if/block。

### 5.4 Content Router Node（依內容路由）

```
kind:    demux   ← 語意上是 demux 的特殊版
inputs:
  port-0  name="in"  kind=any  role=data
outputs:
  port-0  name="out-0"  kind=any
  port-1  name="out-1"  kind=any
  ...
join-strategy: any

props:
  rules  prop-json  default=[]
  ← JSON: [{ "port": 0, "expr": "value > 80" }, { "port": 1, "expr": "value <= 80" }]
  default-port  prop-u32  default=0
```

與 Demux 的差異：Demux 靠外部 condition port 切換，Content Router 靠訊息本身的值決定路由，condition 是內建的 expr 運算。

### 5.5 Merge Node（任一輸入通過）

```
kind:    merge
inputs:
  port-0  name="in-0"  kind=any  role=data
  port-1  name="in-1"  kind=any  role=data
  ...
outputs:
  port-0  name="out"  kind=any
join-strategy: any   ← 任一 port 有資料就通過

props:
  input-port-count  prop-u32  default=2
  output-tag        prop-u32  default=0
```

任何上游有資料就轉發，不等待其他 upstream。可用於合併多個備援 source，或是把多條資料流匯聚到同一條下游。

### 5.6 Join Node（等待所有輸入）

```
kind:    join
inputs:
  port-0  name="in-0"  kind=any  role=data
  port-1  name="in-1"  kind=any  role=data
  ...
outputs:
  port-0  name="out"  kind=any  ← 輸出所有 input 的 msgs 合併
join-strategy: all   ← 全部到齊才觸發

props:
  input-port-count  prop-u32  default=2
  timeout-ms        prop-u32  default=500
  on-timeout        prop-select  choices=["fill-bad-quality","drop"]  default="fill-bad-quality"
```

### 5.7 Source Node（Protobuf 來源）

```
kind:    source
inputs:  （無）
outputs:
  port-0  name="out"  kind=any

props:
  endpoint-id  prop-u32     default=0
  schema-id    prop-u32     default=0
  interval-ms  prop-u32     default=100   description="tick 間隔（毫秒）"
```

### 5.8 Sink Node（Protobuf 輸出）

```
kind:    sink
inputs:
  port-0  name="in"  kind=any  role=data
outputs: （無）

props:
  endpoint-id  prop-u32  default=0
  schema-id    prop-u32  default=0
```

### 5.9 Sink-End Node（Host 事件觸發）

```
kind:    sink-end
inputs:
  port-0  name="in"  kind=any  role=data
outputs: （無）

props:
  event-type   prop-select  choices=["alarm","webhook","syscall","gpio","modbus"]
  action-id    prop-u32     default=0   description="host_trigger 的 action ID"
  metadata     prop-json    default={}  description="附加到 trigger-event 的 metadata"
```

---

## 6. 自定義 Node 開發

任何能編譯到 WASM 並實作 `flow-node world` WIT 介面的語言都可以開發自定義 node。

### 6.1 開發流程

```
1. 安裝 wit-bindgen 工具鏈
   cargo install wit-bindgen-cli

2. 取得 WIT 定義
   iiot-flow init my-node --lang rust

3. 實作 node-spec-iface interface
   → describe() 回傳 NodeSpec
   → init(props, wiring) 初始化
   → process(port, msgs) 處理資料

4. 編譯
   cargo build --target wasm32-unknown-unknown --release
   wasm-tools component new target/.../my-node.wasm \
       --adapt wasi_snapshot_preview1.wasm \
       -o my-node-component.wasm

5. 驗證
   iiot-flow node validate my-node-component.wasm

6. 發佈到 Node Registry
   iiot-flow node publish my-node-component.wasm
```

### 6.2 Rust 實作範例：自定義移動平均 Node

```rust
// wit-bindgen 產生的 binding
wit_bindgen::generate!({
    world: "flow-node",
    path: "wit/iiot-flow.wit",
});

use exports::iiot::flow::node_spec_iface::*;
use iiot::flow::types::*;
use iiot::flow::node_descriptor::*;

struct MovingAvgNode {
    window_size: u32,
    output_tag:  u32,
    buffer:      Vec<f32>,   // 內部狀態：滑動窗口
}

static mut NODE: Option<MovingAvgNode> = None;

impl Guest for MovingAvgNode {

    fn describe() -> NodeSpec {
        NodeSpec {
            name:        "iiot:moving-avg".to_string(),
            version:     "1.0.0".to_string(),
            kind:        NodeKind::Transform,
            inputs: vec![
                InputPortDef {
                    port_id: 0,
                    name:    "in".to_string(),
                    kind:    ValueKind::Any,
                    role:    PortRole::Data,
                    initial_value: None,
                },
            ],
            outputs: vec![
                OutputPortDef {
                    port_id: 0,
                    name:    "avg".to_string(),
                    kind:    ValueKind::F32Val,
                },
            ],
            join_strategy: JoinStrategy::Any,
            props: vec![
                PropDef {
                    key:           "window-size".to_string(),
                    label:         "窗口大小".to_string(),
                    prop_type:     PropType::PropU32,
                    default_value: "10".to_string(),
                    required:      true,
                    choices:       vec![],
                    description:   "移動平均的樣本數".to_string(),
                },
                PropDef {
                    key:           "output-tag".to_string(),
                    label:         "輸出 Tag ID".to_string(),
                    prop_type:     PropType::PropU32,
                    default_value: "0".to_string(),
                    required:      true,
                    choices:       vec![],
                    description:   "".to_string(),
                },
            ],
            label:       "移動平均".to_string(),
            description: "計算輸入值的 N 點移動平均".to_string(),
            icon:        "chart-line".to_string(),
            category:    "math".to_string(),
            color:       "#4A90D9".to_string(),
        }
    }

    fn init(props: String, _wiring: String) -> Result<(), String> {
        let p: serde_json::Value = serde_json::from_str(&props)
            .map_err(|e| e.to_string())?;

        let window_size = p["window-size"].as_u64().unwrap_or(10) as u32;
        let output_tag  = p["output-tag"].as_u64().unwrap_or(0) as u32;

        unsafe {
            NODE = Some(MovingAvgNode {
                window_size,
                output_tag,
                buffer: Vec::with_capacity(window_size as usize),
            });
        }
        Ok(())
    }

    fn process(input_port: u32, msgs: Vec<FlowMsg>) -> Result<NodeOutput, String> {
        let node = unsafe { NODE.as_mut().unwrap() };
        let mut out_msgs = vec![];

        for msg in msgs {
            // 取 f32 值（型別特化後 dispatch 會被消除）
            let v = match msg.value {
                TagValue::F32Val(v) => v,
                TagValue::F64Val(v) => v as f32,
                _ => return Err(format!("unexpected type at port {}", input_port)),
            };

            // 維護滑動窗口
            if node.buffer.len() >= node.window_size as usize {
                node.buffer.remove(0);
            }
            node.buffer.push(v);

            // 計算平均
            let avg = node.buffer.iter().sum::<f32>() / node.buffer.len() as f32;

            out_msgs.push(FlowMsg {
                tag_id:    node.output_tag,
                msg_id:    msg.msg_id,
                value:     TagValue::F32Val(avg),
                timestamp: msg.timestamp,
                quality:   msg.quality,
            });
        }

        Ok(NodeOutput {
            outputs: vec![PortMsgs { port_id: 0, msgs: out_msgs }],
        })
    }

    fn tick() -> Result<NodeOutput, String> {
        // transform node 不實作 tick
        Ok(NodeOutput { outputs: vec![] })
    }
}

export!(MovingAvgNode);
```

### 6.3 C 實作範例：自定義閾值過濾 Node

```c
// 由 wit-bindgen-c 產生的 header
#include "flow-node.h"

// describe() 實作
void exports_iiot_flow_node_spec_iface_describe(
    exports_iiot_flow_node_descriptor_node_spec_t *ret)
{
    // 設定 node 種類
    ret->kind = EXPORTS_IIOT_FLOW_NODE_DESCRIPTOR_NODE_KIND_TRANSFORM;
    ret->name = (iiot_flow_types_string_t){ .ptr = "iiot:threshold", .len = 15 };
    ret->version = (iiot_flow_types_string_t){ .ptr = "1.0.0", .len = 5 };

    // 設定 1 個 input port
    ret->inputs.len = 1;
    ret->inputs.ptr = malloc(sizeof(exports_iiot_flow_node_descriptor_input_port_def_t));
    ret->inputs.ptr[0].port_id = 0;
    ret->inputs.ptr[0].name = (iiot_flow_types_string_t){ .ptr = "in", .len = 2 };
    ret->inputs.ptr[0].kind = IIOT_FLOW_TYPES_VALUE_KIND_ANY;
    ret->inputs.ptr[0].role = EXPORTS_IIOT_FLOW_NODE_DESCRIPTOR_PORT_ROLE_DATA;
    ret->inputs.ptr[0].initial_value.is_some = false;

    // 設定 2 個 output port（高於閾值 / 低於閾值）
    ret->outputs.len = 2;
    ret->outputs.ptr = malloc(2 * sizeof(exports_iiot_flow_node_descriptor_output_port_def_t));
    ret->outputs.ptr[0].port_id = 0;
    ret->outputs.ptr[0].name = (iiot_flow_types_string_t){ .ptr = "above", .len = 5 };
    ret->outputs.ptr[0].kind = IIOT_FLOW_TYPES_VALUE_KIND_ANY;
    ret->outputs.ptr[1].port_id = 1;
    ret->outputs.ptr[1].name = (iiot_flow_types_string_t){ .ptr = "below", .len = 5 };
    ret->outputs.ptr[1].kind = IIOT_FLOW_TYPES_VALUE_KIND_ANY;

    // 設定 property：threshold 值
    ret->props.len = 1;
    ret->props.ptr = malloc(sizeof(exports_iiot_flow_node_descriptor_prop_def_t));
    ret->props.ptr[0].key = (iiot_flow_types_string_t){ .ptr = "threshold", .len = 9 };
    ret->props.ptr[0].prop_type = EXPORTS_IIOT_FLOW_NODE_DESCRIPTOR_PROP_TYPE_PROP_F64;
    ret->props.ptr[0].default_value = (iiot_flow_types_string_t){ .ptr = "0.0", .len = 3 };
    ret->props.ptr[0].required = true;

    ret->label = (iiot_flow_types_string_t){ .ptr = "閾值過濾", .len = 12 };
    ret->category = (iiot_flow_types_string_t){ .ptr = "logic", .len = 5 };
    ret->color = (iiot_flow_types_string_t){ .ptr = "#E67E22", .len = 7 };
}

static double g_threshold = 0.0;

bool exports_iiot_flow_node_spec_iface_init(
    iiot_flow_types_string_t *props,
    iiot_flow_types_string_t *wiring,
    iiot_flow_types_string_t *error)
{
    // 簡單解析 props JSON 取 threshold
    // 實際可用 jsmn 或 cJSON
    g_threshold = parse_double_field(props->ptr, "threshold");
    return true;
}

bool exports_iiot_flow_node_spec_iface_process(
    uint32_t input_port,
    exports_iiot_flow_types_list_flow_msg_t *msgs,
    exports_iiot_flow_node_spec_iface_node_output_t *ret,
    iiot_flow_types_string_t *error)
{
    // 分配兩個 output port 的 buffer
    // ... 依閾值把 msg 分到 port-0 或 port-1
    for (size_t i = 0; i < msgs->len; i++) {
        float v = get_f32_value(&msgs->ptr[i].value);
        if (v > g_threshold)
            append_to_port(ret, 0, &msgs->ptr[i]);
        else
            append_to_port(ret, 1, &msgs->ptr[i]);
    }
    return true;
}
```

### 6.4 跨語言支援矩陣

| 語言 | 工具鏈 | 成熟度 | 備註 |
|------|--------|--------|------|
| Rust | `wit-bindgen` + `cargo component` | ✅ 完整 | 推薦首選，工具鏈最完整 |
| C | `wit-bindgen-c` | ✅ 完整 | 適合嵌入式 / 資源受限環境 |
| C++ | `wit-bindgen-c` + C++ wrapper | ✅ 可用 | 同 C，需手動包 RAII |
| Go | `TinyGo` + `wit-bindgen-go` | 🔶 實驗 | TinyGo 有部分標準庫限制 |
| Python | `componentize-py` | 🔶 實驗 | 適合快速原型，效能較低 |
| AssemblyScript | `as-wit` | 🔶 實驗 | TypeScript-like 語法 |
| Zig | `wit-bindgen-zig` | 🔶 實驗 | 社群維護 |

---

## 7. DAG 設計與資料流

### 7.1 flow.json 格式

```json
{
  "version": "0.1.0",
  "nodes": [
    {
      "id":       "src-temp",
      "wasm":     "proto_source.wasm",
      "props": { "endpoint-id": 0, "schema-id": 1, "interval-ms": 100 }
    },
    {
      "id":       "src-ctrl",
      "wasm":     "proto_source.wasm",
      "props": { "endpoint-id": 1, "schema-id": 2, "interval-ms": 500 }
    },
    {
      "id":       "math-f2c",
      "wasm":     "math_op.wasm",
      "props": { "operator": "-", "output-tag": 201, "initial-b": 32.0 }
    },
    {
      "id":       "math-scale",
      "wasm":     "math_op.wasm",
      "props": { "operator": "*", "output-tag": 202, "initial-b": 0.5556 }
    },
    {
      "id":       "mux-1",
      "wasm":     "mux_node.wasm",
      "props": { "data-port-count": 2, "output-tag": 301 }
    },
    {
      "id":       "router-1",
      "wasm":     "content_router.wasm",
      "props": {
        "rules": [
          { "port": 0, "expr": "value > 80" },
          { "port": 1, "expr": "value <= 80" }
        ]
      }
    },
    {
      "id":       "sink-alarm",
      "wasm":     "proto_sink.wasm",
      "props": { "endpoint-id": 2, "schema-id": 3 }
    },
    {
      "id":       "sink-normal",
      "wasm":     "proto_sink.wasm",
      "props": { "endpoint-id": 3, "schema-id": 3 }
    },
    {
      "id":       "end-alert",
      "wasm":     "sink_end.wasm",
      "props": { "event-type": "alarm", "metadata": { "severity": "high" } }
    }
  ],
  "edges": [
    { "from": "src-temp",  "from-port": 0, "to": "math-f2c",   "to-port": 0, "to-role": "data" },
    { "from": "math-f2c",  "from-port": 0, "to": "math-scale", "to-port": 0, "to-role": "data" },
    { "from": "math-scale","from-port": 0, "to": "router-1",   "to-port": 0, "to-role": "data" },
    { "from": "src-temp",  "from-port": 0, "to": "mux-1",      "to-port": 0, "to-role": "data" },
    { "from": "src-ctrl",  "from-port": 0, "to": "mux-1",      "to-port": 2, "to-role": "condition" },
    { "from": "router-1",  "from-port": 0, "to": "sink-alarm",  "to-port": 0, "to-role": "data" },
    { "from": "router-1",  "from-port": 0, "to": "end-alert",   "to-port": 0, "to-role": "data" },
    { "from": "router-1",  "from-port": 1, "to": "sink-normal", "to-port": 0, "to-role": "data" },
    { "from": "mux-1",     "from-port": 0, "to": "sink-alarm",  "to-port": 0, "to-role": "data" }
  ]
}
```

### 7.2 範例 DAG 拓樸

```
src-temp ──(data)──▶ math-f2c ──▶ math-scale ──▶ router-1 ──(>80)──▶ sink-alarm
    │                                                  │                end-alert（host_notify）
    │                                               (<=80)──▶ sink-normal
    │
    └──(data port-0)──▶ mux-1 ──▶ sink-alarm
                           ↑
src-ctrl ──(condition)─────┘
```

---

## 8. DAG 即 WASM Instance

### 8.1 核心思想

> **把整個 DAG 視為一個程式，把每個 node 視為這個程式裡的一個函式，把每條 edge 視為靜態的函式呼叫。**

node 開發時是獨立的 WASM 元件，有自己的 `process()` 實作邏輯。Deploy 後所有 node 合併成一個 `flow.wasm`，node 間原本由 runtime 負責的「查 edge table → 呼叫下游」變成編譯期已知的直接函式呼叫，節點間不再有任何序列化邊界。

**關鍵釐清：Mux/Demux 的決策邏輯不是 fusion 的工作。**

Mux node 自己的 `process()` 已經包含完整的條件判斷邏輯（讀 condition global、決定輸出到哪個 port）。Fusion 只需要處理「node 回傳 `node-output { port-id: 1, msgs }` 之後，誰來把這個 port-1 的 msgs 送到對應的下游 node」——把這個動作從 runtime 的動態查表替換成靜態函式呼叫，這就是 fusion 對 Mux/Demux 所做的全部事情。

### 8.2 執行單元邊界

一個 `flow.json` = 一個 `flow.wasm` = 一個 WASM instance。

Fan-in join 是 instance 內部的等待點，join buffer 只是 WASM 線性記憶體裡的 struct，跨 tick 持久存在，不需要跨 instance 通訊。

### 8.3 多 Source 的 tick 入口

同一 DAG 若有多個 source，`flow.wasm` export 對應每個 source 的獨立 tick 入口，Runtime 依各 source 的 `interval-ms` 分別排程呼叫，共享 WASM 記憶體（mux state、join buffer）：

```wit
world flow-dag {
    import host-io;
    export init:          func(config: string) -> result<_, string>;
    export tick-source-0: func() -> result<_, string>;  // src-temp
    export tick-source-1: func() -> result<_, string>;  // src-ctrl
}
```

---

## 9. Deploy Pipeline

```
flow.json + node.wasm files
    │
    ▼
[Stage 1] Parse & Build IR
    │  呼叫每個 node 的 describe() 取得 node-spec
    │  建立 FlowGraph IR（含 port def、join strategy、props）
    │
    ▼
[Stage 2] Validate ──── 失敗 ──▶ Error Report（停止）
    │  ① Cycle Detection（DFS topo sort）
    │  ② 孤立 Node 檢查
    │  ③ Source / Sink / sink-end 邊界完整性
    │  ④ Condition Port 對應（mux/demux 必須有 condition port）
    │  ⑤ 型別推導與相容性（forward inference，填入 resolved_types）
    │  ⑥ Join Strategy 驗證（all-or-initial 必須有 initial-value）
    │
    ▼
[Stage 3] Full-DAG Fusion
    │  ① 全圖 wasm-merge（所有 node.wasm → flow_merged.wasm）
    │  ② 靜態 edge 替換（output port → 下游 node 的直接函式呼叫）
    │  ③ 型別特化（消除 variant dispatch）
    │  ④ Props 內嵌（props 燒進 data section，變為編譯期常數）
    │
    ▼
[Stage 4] AOT（選擇性）
    │  ① wasm-opt -O3（Optimized WASM）
    │  ② Wasmtime / WAMR compile（Native Binary）
    │
    ▼
[Stage 5] Emit Artifact
    │  flow.wasm / flow.cwasm + manifest.json
    ▼
Runtime 載入單一 instance 執行
```

---

## 10. Fusion 優化

### 10.1 IR 資料結構

```rust
struct FlowGraph {
    nodes:      HashMap<NodeId, IrNode>,
    edges:      Vec<IrEdge>,
    topo_order: Vec<NodeId>,
}

struct IrNode {
    id:             NodeId,
    kind:           NodeKind,     // 來自 describe()
    wasm_path:      String,
    inputs:         Vec<IrInputPort>,
    outputs:        Vec<IrOutputPort>,
    join_strategy:  JoinStrategy, // 來自 describe()
    props:          JsonValue,    // 使用者在 GUI 設定的值
    resolved_types: HashMap<PortId, ConcreteKind>,
}
```

### 10.2 Validate Pass 詳細

#### Pass ①：Cycle Detection
DFS topological sort，有 back edge 即報錯，成功後產出 `topo_order`。

#### Pass ②：孤立 Node 檢查
```
in-degree = 0  且 kind ≠ source            → 孤立，報錯
out-degree = 0 且 kind ∉ {sink, sink-end}  → 孤立，報錯
```

#### Pass ③：Source / Sink / sink-end 邊界
```
整張圖至少有一個 source
整張圖至少有一個 sink 或 sink-end
edge 起點不能是 sink / sink-end
edge 終點不能是 source
```

#### Pass ④：Condition Port 對應
```
有 role = condition 的 input port → kind 必須是 mux 或 demux
mux 節點必須恰好有一個 condition port
mux 節點必須有至少兩個 data input port
demux 節點必須恰好有一個 condition port
demux 節點必須有至少兩個 output port
```

#### Pass ⑤：型別推導與相容性
沿 `topo_order` 做 forward type inference：
```
source output → 具體型別 → 沿 edge 傳播
any port      → 接受，記錄 resolved_types
具體型別 port → 相符通過，不符報錯
```
推導完成後所有 port 的 resolved_types 都是具體型別，不再有 any。

#### Pass ⑥：Join Strategy 驗證
```
join-strategy = all-or-initial
→ 每個 input port 必須有 initial-value 或 required=false
→ 否則報錯：缺少初始值且無法保證 all-or-initial 語意
```

### 10.3 Fusion：全圖 wasm-merge

```bash
# 依 topo_order 合併所有 node wasm
wasm-merge \
    proto_source.wasm   node_src_temp    \
    proto_source.wasm   node_src_ctrl    \
    math_op.wasm        node_math_f2c    \
    math_op.wasm        node_math_scale  \
    mux_node.wasm       node_mux_1       \
    content_router.wasm node_router_1    \
    proto_sink.wasm     node_sink_alarm  \
    proto_sink.wasm     node_sink_normal \
    sink_end.wasm       node_end_alert   \
    -o flow_merged.wasm
```

合併後所有 node 的函式都在同一個 WASM module 內，名稱以 namespace 區分（如 `node_math_f2c::process`、`node_router_1::process`）。接下來的 edge 替換才有可能產生跨 node 的直接呼叫。

### 10.4 Fusion：靜態 edge 替換

這是 DAG Fusion 的核心步驟，也是與 pipeline fusion 最不同的地方。

**核心概念：**

未 fusion 時，runtime 執行的邏輯是：
```
output = node_A.process(msgs)
for port_msgs in output.outputs:
    for edge in edge_table[node_A][port_msgs.port_id]:   // 動態查表
        edge.to_node.process(edge.to_port, port_msgs.msgs)
```

fusion 後，這個動態查表被替換成編譯期已知的靜態呼叫序列，`node-output` 結構本身也不再需要實際分配：

```
// edge table（靜態，來自 flow.json）：
//   node_A port-0 → node_B port-0
//   node_A port-0 → node_C port-1   ← 同一個 port fan-out 給兩個下游
//   node_A port-1 → node_D port-0

// fusion 後產生的 dispatch wrapper：
func node_A_dispatch(msgs):
    output = node_A_process(msgs)
    // port-0 的下游（靜態已知，直接呼叫）
    node_B_process(0, output.port0)
    node_C_process(1, output.port0)   // fan-out：同資料傳兩個下游
    // port-1 的下游
    node_D_process(0, output.port1)
```

**DAG 的三種結構都可以正確 fusion：**

```
Fan-out（一個 node 接多個下游）：
  node_A → node_B
  node_A → node_C

  fusion：
    out = node_A_process(msgs)
    node_B_process(0, out.port0)   // 直接呼叫 B
    node_C_process(0, out.port0)   // 直接呼叫 C（複製同一份資料）

Fan-in，merge 語意（多個 upstream，任一到即通過）：
  node_A → node_D（port-0）
  node_B → node_D（port-1）

  fusion：
    // node_A 的 dispatch wrapper 中：
    node_D_process(0, out_a.port0)   // A 觸發時直接呼叫 D 的 port-0
    // node_B 的 dispatch wrapper 中：
    node_D_process(1, out_b.port0)   // B 觸發時直接呼叫 D 的 port-1
    // D 內部的 join-strategy = any，收到任一 port 就輸出
    // → 完全正確，無需 runtime 介入

Fan-in，join 語意（等待所有 upstream）：
  node_A → node_E（port-0）
  node_B → node_E（port-1）

  fusion：
    // 同上，dispatch wrapper 直接呼叫 E 的對應 port
    // E 內部的 join-strategy = all
    // → E 自己在 WASM 線性記憶體維護 buffer
    // → A tick 時：E 存入 buffer[0]，buffer 未齊，return 空 output
    // → B tick 時：E 存入 buffer[1]，buffer 齊全，執行計算往下推
    // → join 的等待語意完全在 E 的 process() 內處理，fusion 不需要知道
```

**Mux/Demux 的 fusion（只處理 edge，不碰內部邏輯）：**

```
Mux node 的 process() 自己決定輸出到 port-0 還是 port-1。
Fusion 只負責「port-0 接到誰、port-1 接到誰」這個靜態事實：

  mux_1 port-0 → sink_alarm port-0
  mux_1 port-1 → sink_normal port-0

  fusion 產生的 dispatch wrapper：
    func mux_1_dispatch(input_port, msgs):
        output = mux_1_process(input_port, msgs)  // node 自己決定輸出哪個 port
        if output.has_port(0):
            sink_alarm_process(0, output.port0)   // 靜態：port-0 接 sink_alarm
        if output.has_port(1):
            sink_normal_process(0, output.port1)  // 靜態：port-1 接 sink_normal

  // Mux 的條件判斷邏輯（讀 condition global）完全在 mux_1_process() 內部
  // Fusion 完全不需要知道 Mux 是怎麼決策的
```

wasm-opt 在後續優化時，可以進一步把 `mux_1_dispatch` 和下游的呼叫 inline，讓整條路徑成為一個連續的函式體，消除中間的函式呼叫開銷。

### 10.5 Fusion：型別特化

```
resolved_type = f32-val → 展開前的 match 消除：

match msg.value {                      →  let v = msg.value.f32_val;
    TagValue::F32Val(v) => v - 32.0,   →  let r = v - 32.0;   // 直接計算
    _ => panic!()
}
```

### 10.6 Fusion：Props 內嵌

Props 值燒進 WASM data section 後，wasm-opt 可做：
- **Constant propagation**：運算式中的常數直接替換
- **Dead branch elimination**：如 `operator = "+"` 時，其他 operator 的 branch 被消除
- **Strength reduction**：`* 0.5556` 可能被最佳化為乘法/加法組合

---

## 11. AOT 編譯

### 11.1 Optimized WASM

```bash
wasm-opt flow_merged.wasm -O3     \
    --enable-simd                 \
    --enable-bulk-memory          \
    --dce                         \
    --inlining-optimizing         \
    --precompute                  \
    -o flow.wasm
```

### 11.2 Native Binary

**Wasmtime AOT（x86 伺服器 / 工控機）：**
```bash
wasmtime compile flow.wasm -o flow.cwasm
```

**WAMR AOT（ARM / RISC-V 嵌入式）：**
```bash
wamrc --target=aarch64-unknown-linux-gnu \
      --cpu-features=+neon               \
      flow.wasm -o flow.aot
```

### 11.3 AOT Target 選擇

```
deploy --aot=none    → Optimized WASM（預設，最廣泛相容）
deploy --aot=native  → 自動偵測目標平台，產出 native binary
deploy --aot=both    → 同時產出兩種，manifest 記錄優先順序
```

---

## 12. Deployable Artifact

### 12.1 目錄結構

```
flow-deploy/
├── manifest.json    ← Runtime 的唯一入口
├── flow.wasm        ← 整個 DAG（Optimized WASM）
├── flow.cwasm       ← Native AOT（選擇性）
└── schema/proto/
    ├── sensor.proto
    └── control.proto
```

### 12.2 manifest.json

```json
{
  "version":  "0.1.0",
  "flow-id":  "production-line-A",
  "artifact": {
    "wasm":          "flow.wasm",
    "native-aot":    "flow.cwasm",
    "prefer-native": true
  },
  "sources": [
    { "tick-export": "tick-source-0", "interval-ms": 100, "description": "src-temp" },
    { "tick-export": "tick-source-1", "interval-ms": 500, "description": "src-ctrl" }
  ],
  "host-endpoints": [
    { "id": 0, "type": "tcp-client", "address": "192.168.1.10:5000", "role": "source" },
    { "id": 1, "type": "tcp-client", "address": "192.168.1.11:5000", "role": "source" },
    { "id": 2, "type": "tcp-client", "address": "alarm-server:6000",  "role": "sink"   },
    { "id": 3, "type": "tcp-client", "address": "data-server:6001",   "role": "sink"   }
  ],
  "sink-ends": [
    {
      "id":         "end-alert",
      "handler":    "webhook",
      "target":     "https://ops.example.com/alert",
      "timeout-ms": 3000
    }
  ]
}
```

---

## 13. Runtime 架構

### 13.1 Runtime 職責

```
1. 讀取 manifest.json
2. 載入 flow.wasm / flow.cwasm（依 prefer-native 決定）
3. 建立單一 WASM instance，注入 host-io import 實作
4. 呼叫 instance.init()
5. 依 manifest.sources 的 interval-ms 排程，
   定時呼叫各 source 對應的 tick-source-N 函式
6. 實作 host-io 四個 host function
```

### 13.2 Runtime 執行偽碼

```rust
fn run(manifest: Manifest) {
    let instance = WasmInstance::load(
        manifest.artifact.preferred_path(),
        HostIO {
            recv:      |id| manifest.endpoint(id).recv(),
            send:      |id, data| manifest.endpoint(id).send(data),
            notify:    |event| dispatch_sink_end(&manifest.sink_ends, event),
            trigger:   |action_id, params| host_action(action_id, params),
            timestamp: || system_time_us(),
        }
    )?;

    instance.call("init", &[])?;

    let mut scheduler = Scheduler::new();
    for source in &manifest.sources {
        let export   = source.tick_export.clone();
        let interval = source.interval_ms;
        scheduler.every(interval, move || {
            instance.call(&export, &[])?;
        });
    }

    scheduler.run_forever();
}
```

### 13.3 Runtime 不負責的事

| 不負責 | 負責的是誰 |
|--------|-----------|
| Node 間訊息路由 | 已 fusion 成靜態函式呼叫（dispatch wrapper） |
| Node 內部決策邏輯（Mux 條件判斷等） | Node 自己的 `process()` |
| Join / Mux state | WASM linear memory（node 自己管理） |
| 型別檢查 | Deploy Validate Pass |
| DAG 驗證 | Deploy Validate Pass |
| Node props 解析 | 已內嵌進 WASM data section |
| 訊息序列化 | 已消除（直接記憶體傳遞） |

---

## 14. 錯誤處理策略

### 14.1 Deploy 階段錯誤

```json
{
  "stage": "validate",
  "pass":  "type-inference",
  "errors": [
    {
      "code":     "TYPE_MISMATCH",
      "edge":     { "from": "src-temp", "from-port": 0, "to": "str-concat", "to-port": 0 },
      "expected": "short-str",
      "actual":   "f32-val",
      "message":  "str-concat port-0 期望 short-str，但 src-temp port-0 輸出 f32-val"
    }
  ]
}
```

### 14.2 Runtime 階段錯誤

| 錯誤類型 | 處理方式 |
|---------|---------|
| `tick-source-N()` 回傳 Err | 記錄 log，跳過此 tick |
| `init()` 回傳 Err | Fatal，停止 instance |
| WASM Trap | 隔離 instance，記錄 core dump，嘗試重新載入 |
| `host_recv()` 超時 | 由 host 實作決定：Err 或空資料 |
| `host_notify()` 失敗 | 記錄 log，不影響資料流 |
| `host_trigger()` 失敗 | 記錄 log + 告警，依 criticality 決定是否 fatal |
| Join buffer timeout | 以 `quality = 0xFF` 填空，強制觸發計算 |

---

## 15. 設計決策彙整

| 決策點 | 選擇 | 理由 |
|--------|------|------|
| Node 規格機制 | WIT interface + `describe()` 回傳 `node-spec` JSON | 跨語言通用，GUI 和 Pipeline 共用同一份 descriptor |
| 跨語言支援策略 | 任何語言只要能用 wit-bindgen 產生 binding | WIT 是 language-agnostic 的規格語言，天然支援多語言 |
| Properties 傳遞時機 | `init()` 時傳入，fusion 後燒進 WASM data section | 變成編譯期常數，讓 AOT 做 constant propagation |
| Wiring 傳遞時機 | `init()` 時傳入（wiring JSON） | Node 知道 upstream/downstream port 數量，自主管理 join buffer |
| join-strategy 設計 | any / all / all-or-initial 三種 | 覆蓋「任一更新」「全部到齊」「有初始值任一更新」三種工業場景 |
| Mux / Demux 的 condition port | 與 data port 用同一套 port def，以 role 區分 | 不需要特殊介面，所有 node 統一用 `process(port-id, msgs)` |
| DAG 執行單元 | 整個 DAG = 單一 WASM instance | 消除所有 node 間序列化，最大化 fusion 優化空間 |
| DAG Fusion 的本質 | 靜態 edge 替換（output port → 下游 node 的直接函式呼叫） | Runtime 的動態查表改為編譯期已知的靜態呼叫，fan-out/fan-in 都適用 |
| Mux/Demux 決策邏輯 | 完全在 node 自己的 `process()` 內，Fusion 不介入 | Node 決定輸出哪個 port，Fusion 只負責「那個 port 靜態接到哪個下游」 |
| sink-end 設計 | 獨立節點，透過 Host Function 觸發 | 副作用與資料流解耦，host 行為可 mock 測試 |
| AOT 工具 | Wasmtime（x86）+ WAMR（ARM/嵌入式） | 兩者均有成熟 IIoT 部署案例 |
| Runtime 薄層 | 只有 init + tick-source-N | 所有複雜度前移到 Deploy pipeline |


