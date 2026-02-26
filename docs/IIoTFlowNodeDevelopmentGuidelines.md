# IIoT Flow Engine — Platform Development Kit（PDK）

**版本：** 1.0.0
**套件名稱：** `iiot:flow`
**最後更新：** 2026-02-26

> 本文件是 IIoT Flow Engine 的 Node 開發者指南。閱讀完畢後，你將能夠用任何支援 WASM 的語言開發自訂 node、在本地測試，並發佈到 Node Registry。

---

## 目錄

1. [PDK 概觀](#1-pdk-概觀)
2. [Auto Type Conversion](#2-auto-type-conversion)
3. [WIT 規格參考](#3-wit-規格參考)
4. [CLI 工具](#4-cli-工具)
5. [Rust 開發指引](#5-rust-開發指引)
6. [C 開發指引](#6-c-開發指引)
7. [C++ 開發指引](#7-c-開發指引)
8. [測試框架](#8-測試框架)
9. [最佳實踐](#9-最佳實踐)
10. [Node Registry 發佈](#10-node-registry-發佈)

---

## 1. PDK 概觀

### 1.1 PDK 目錄結構

```
iiot-flow-pdk/
├── wit/
│   └── iiot-flow.wit          ← 完整 WIT 規格（唯一的真理來源）
├── crates/
│   └── iiot-flow-pdk/         ← Rust PDK crate
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs          ← re-export + 輔助 macro
│           ├── convert.rs      ← auto type conversion helper
│           └── test.rs         ← 測試框架
├── c/
│   ├── include/
│   │   ├── iiot_flow.h         ← wit-bindgen-c 產生的 header
│   │   └── iiot_flow_pdk.h     ← PDK 輔助函式
│   └── lib/
│       └── iiot_flow_pdk.c
├── examples/
│   ├── rust/
│   │   ├── math-op/            ← 四則運算 node（Rust）
│   │   ├── moving-avg/         ← 移動平均 node（Rust）
│   │   └── threshold/          ← 閾值過濾 node（Rust）
│   ├── c/
│   │   ├── math-op/            ← 四則運算 node（C）
│   │   └── threshold/          ← 閾值過濾 node（C）
│   └── cpp/
│       └── moving-avg/         ← 移動平均 node（C++）
├── cli/
│   └── iiot-flow-cli/          ← CLI 工具原始碼
└── docs/
    ├── pdk.md                  ← 本文件
    └── architecture.md         ← 系統架構文件
```

### 1.2 安裝 PDK

```bash
# 安裝 CLI 工具
cargo install iiot-flow-cli

# 驗證安裝
iiot-flow --version
# iiot-flow-cli 1.0.0

# Rust 開發：在 Cargo.toml 加入
[dependencies]
iiot-flow-pdk = "1.0.0"
```

### 1.3 Node 開發的五個步驟

```
1. iiot-flow new <name> --lang <rust|c|cpp>
   → 產生含 WIT binding 的 node 專案骨架

2. 實作 describe()
   → 宣告 node 的 kind、ports、props

3. 實作 init(props, wiring)
   → 從 props JSON 讀取設定值，初始化內部狀態

4. 實作 process(input_port, msgs)
   → 核心計算邏輯，回傳 NodeOutput

5. iiot-flow node test / validate / publish
   → 本地測試、格式驗證、發佈到 Registry
```

---

## 2. Auto Type Conversion

### 2.1 設計概念

當使用者在 GUI 連接兩個 node 的 port，若上游 output 的型別與下游 input 的型別不同（但都在數值族內），Deploy Pipeline 會自動在兩個 node 之間插入一個隱形的 **Cast Node**，完全不需要使用者手動處理，也不需要 node 開發者自己寫轉型程式碼。

```
使用者看到的 DAG：
  src（output: i16） ──▶ math-op（input: f32）

Deploy Pipeline 實際產出的 DAG：
  src（output: i16） ──▶ [cast: i16→f32] ──▶ math-op（input: f32）

flow.wasm fusion 後：
  i16_to_f32 的轉換直接 inline 在呼叫鏈中，沒有額外開銷
```

### 2.2 數值族轉換規則

所有數值型別之間都可以自動互轉，依「是否可能損失精度」分為兩類：

**無損轉換（Lossless）**：值域完全被目標型別涵蓋，靜默轉換。

| 來源 | 可無損轉換到 |
|------|------------|
| `i8`  | `i16`, `i32`, `i64`, `f32`, `f64` |
| `u8`  | `u16`, `u32`, `u64`, `i16`, `i32`, `i64`, `f32`, `f64` |
| `i16` | `i32`, `i64`, `f32`, `f64` |
| `u16` | `u32`, `u64`, `i32`, `i64`, `f32`, `f64` |
| `i32` | `i64`, `f64` |
| `u32` | `u64`, `i64`, `f64` |
| `f32` | `f64` |

**有損轉換（Lossy）**：可能截斷或精度損失，Deploy Pipeline 產出 warning，轉換仍然執行。

| 來源 | 有損轉換到 | 風險 |
|------|-----------|------|
| `f64` | `f32` | 精度損失（有效位數從 15 降到 7） |
| `i64` | `f32`, `f64` | 大整數精度損失 |
| `i32` | `f32` | 大整數精度損失 |
| `f32`/`f64` | 任何整數型別 | 小數截斷、overflow |
| 大範圍整數 | 小範圍整數 | overflow 截斷（e.g. i32→i8） |

**不支援的轉換**：`bool`、`short-str`、`blob` 與數值族之間不自動轉換，連接這些型別的 edge 在 Validate Pass 報錯。

### 2.3 Deploy Pipeline 插入 Cast Node 的流程

```
Validate Pass ⑤（型別推導）發現型別不符：
  edge: src(i16) → math-op(f32)
  resolved_type 不同但都在數值族 → 可轉換

  → 在 IR 中插入隱形 CastNode：
    src(i16) → cast_i16_f32 → math-op(f32)

  → 若是有損轉換：
    emit warning: "edge src→math-op: i16→f32 轉換（lossless）"
    或
    emit warning: "edge sensor→calc: f64→f32 可能精度損失"

Fusion Pass：
  cast_i16_f32 的邏輯極簡（單一 WASM instruction），
  直接 inline 進 dispatch wrapper，零額外函式呼叫。
```

### 2.4 Cast Node 的實作（PDK 內建，開發者不需要知道）

```rust
// PDK 內建的數值轉換，每一種都是單一 WASM instruction
fn cast_i16_to_f32(msg: FlowMsg) -> FlowMsg {
    let v = match msg.value { TagValue::I16Val(v) => v, _ => unreachable!() };
    FlowMsg { value: TagValue::F32Val(v as f32), ..msg }
}

fn cast_f64_to_f32(msg: FlowMsg) -> FlowMsg {
    let v = match msg.value { TagValue::F64Val(v) => v, _ => unreachable!() };
    // 有損：精度可能損失，但繼續執行
    FlowMsg { value: TagValue::F32Val(v as f32), ..msg }
}
// ... 共 N*(N-1) 種組合，全部 PDK 內建
```

### 2.5 Node 開發者如何宣告接受的型別

Node 開發者在 `describe()` 中對 input port 宣告 `kind`，有三種選擇：

```rust
// 選項一：宣告具體型別（只接受 f32，其他數值族自動 cast）
InputPortDef { kind: ValueKind::F32Val, .. }
// → Pipeline 會自動插入 cast node，無損轉換靜默，有損輸出 warning

// 選項二：宣告 any（接受所有型別，node 自己用 PDK helper 處理）
InputPortDef { kind: ValueKind::Any, .. }
// → 不插入 cast node，node 的 process() 收到原始 tag-value
// → 適合通用 node（logger、filter、forward）

// 選項三：宣告 numeric（PDK 提供的輔助 kind，表示接受任何數值族）
// 在 WIT 層等同於 any，但 Pipeline 知道這個 port 只期望數值
// → Pipeline 對非數值族（bool/str/blob）連接報錯，數值族自動 cast
InputPortDef { kind: ValueKind::Numeric, .. }  // PDK 擴充
```

---

## 3. WIT 規格參考

完整 WIT 定義請參考 `pdk/wit/iiot-flow.wit`，以下列出 Node 開發者最常用的部分。

### 3.1 flow-node World（Node 開發者實作的介面）

```wit
world flow-node {
    export node-spec-iface;
    export proto-codec;   // source/sink 實作，其他 node 提供空實作
}
```

### 3.2 node-spec-iface（必須實作的三個函式）

```wit
interface node-spec-iface {
    use types.{flow-msg, node-output};
    use node-descriptor.{node-spec};

    /// 回傳 node 的靜態描述，GUI 和 Pipeline 呼叫
    describe: func() -> node-spec;

    /// 初始化：props 是使用者設定值（JSON），wiring 是連線資訊（JSON）
    init: func(props: string, wiring: string) -> result<_, string>;

    /// 核心邏輯：input-port 指定來自哪個 upstream port
    process: func(input-port: u32, msgs: list<flow-msg>) -> result<node-output, string>;

    /// Source 專用，其他 node 回傳空 output
    tick: func() -> result<node-output, string>;
}
```

### 3.3 node-spec 結構速查

```wit
record node-spec {
    name:          string,        // 唯一識別，建議 "組織:名稱" e.g. "myco:temp-filter"
    version:       string,        // semver e.g. "1.0.0"
    kind:          node-kind,     // source/sink/sink-end/transform/mux/demux/merge/join
    inputs:        list<input-port-def>,
    outputs:       list<output-port-def>,
    join-strategy: join-strategy, // any / all / all-or-initial
    props:         list<prop-def>,
    label:         string,        // GUI 顯示名稱
    description:   string,
    icon:          string,        // icon 名稱
    category:      string,        // GUI 分類
    color:         string,        // hex 顏色 e.g. "#4A90D9"
}
```

### 3.4 value-kind 速查

| kind | 說明 | 自動 cast 來源 |
|------|------|--------------|
| `bool-val` | 布林 | 無 |
| `i8-val` ~ `u64-val` | 有號/無號整數 | 數值族 |
| `f32-val` / `f64-val` | 浮點數 | 數值族 |
| `short-str` | 字串 | 無 |
| `blob` | 二進位 | 無 |
| `any` | 不限型別 | 不插入 cast |
| `numeric` | 任意數值（PDK 擴充） | 數值族 cast，非數值報錯 |

---

## 4. CLI 工具

### 4.1 建立新 Node 專案

```bash
# Rust node
iiot-flow new my-filter --lang rust
# 產生：
# my-filter/
# ├── Cargo.toml
# ├── wit/iiot-flow.wit      ← 從 PDK 複製
# └── src/lib.rs             ← 含 describe/init/process 骨架

# C node
iiot-flow new my-filter --lang c
# 產生：
# my-filter/
# ├── CMakeLists.txt
# ├── include/iiot_flow.h    ← wit-bindgen-c 產生
# └── src/my_filter.c        ← 含函式骨架

# C++ node
iiot-flow new my-filter --lang cpp
# 產生同 C，額外含 include/iiot_flow_pdk.hpp
```

### 4.2 驗證 Node

```bash
# 驗證 node.wasm 是否符合 WIT 規格
iiot-flow node validate ./my-filter.wasm

# 輸出範例（通過）：
# ✅ WIT interface: flow-node world 實作完整
# ✅ describe(): node-spec 結構合法
# ✅ props: 3 個欄位，型別合法
# ✅ ports: 2 input, 1 output，無衝突

# 輸出範例（失敗）：
# ❌ describe(): outputs[0].kind = "f32-val" 但 inputs[0].kind = "any"
#    → output 宣告具體型別時，建議 input 也宣告具體型別以啟用自動 cast
# ⚠️  props["threshold"].default-value 不是合法的 f64 JSON 值
```

### 4.3 本地執行測試

```bash
# 執行 node 的測試套件（詳見第 8 章）
iiot-flow node test ./my-filter.wasm

# 單獨執行某個測試
iiot-flow node test ./my-filter.wasm --case "basic_f32"

# 輸出：
# running 3 tests
# test basic_f32     ... ok (1.2ms)
# test edge_quality  ... ok (0.8ms)
# test auto_cast_i16 ... ok (0.9ms)
# test result: ok. 3 passed; 0 failed
```

### 4.4 檢查 Auto Cast 相容性

```bash
# 查詢兩個 node 的 port 是否相容（含 auto cast 資訊）
iiot-flow compat my-source.wasm:port-0 my-filter.wasm:port-0

# 輸出範例：
# source  output port-0: i16-val
# filter  input  port-0: f32-val
# → ✅ 相容（lossless cast：i16 → f32）
#   Pipeline 將自動插入 cast node

# 輸出範例（有損）：
# source  output port-0: f64-val
# filter  input  port-0: f32-val
# → ⚠️  相容（lossy cast：f64 → f32，精度可能損失）
#   Pipeline 將自動插入 cast node 並輸出 warning
```

### 4.5 發佈 Node

```bash
# 登入 Registry
iiot-flow login

# 發佈
iiot-flow node publish ./my-filter.wasm \
    --name "myco:temp-filter"           \
    --version "1.0.0"

# 輸出：
# 🔍 Validating...  ✅
# 📦 Uploading...   ✅
# 🎉 Published: myco:temp-filter@1.0.0
#    Registry URL: https://registry.iiot-flow.io/nodes/myco/temp-filter
```

---

## 5. Rust 開發指引

### 5.1 Cargo.toml 設定

```toml
[package]
name = "my-math-node"
version = "1.0.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
iiot-flow-pdk = "1.0.0"   # PDK crate（含 wit-bindgen + 輔助工具）
serde_json = "1.0"

[profile.release]
opt-level = "s"   # 最小化 wasm 大小
lto = true
```

### 5.2 完整範例：四則運算 Node

```rust
use iiot_flow_pdk::prelude::*;   // 匯入 PDK 所有常用型別與 macro

// PDK 提供 node_impl! macro，自動處理 WASM export 樣板
node_impl!(MathOpNode);

/// 節點狀態（init 後持有）
struct MathOpNode {
    operator:   char,
    output_tag: u32,
    // all-or-initial 策略：各 port 的當前值
    val_a: f64,
    val_b: f64,
}

impl FlowNode for MathOpNode {

    // ── 靜態描述 ──────────────────────────────────────────
    fn describe() -> NodeSpec {
        NodeSpec::builder()
            .name("iiot:math-op")
            .version("1.0.0")
            .kind(NodeKind::Transform)
            .label("四則運算")
            .category("math")
            .color("#4A90D9")
            .description("對兩個輸入值執行 +、-、×、÷ 運算")
            // Input ports：宣告 numeric，接受任意數值族（自動 cast）
            .input(InputPort::new(0, "a").numeric().initial("0.0"))
            .input(InputPort::new(1, "b").numeric().initial("0.0"))
            // Output port：f64（運算結果統一用 f64 輸出）
            .output(OutputPort::new(0, "result").kind(ValueKind::F64Val))
            .join_strategy(JoinStrategy::AllOrInitial)
            // Properties
            .prop(Prop::select("operator", "運算子")
                .choices(["+", "-", "*", "÷"])
                .default("+")
                .required(true))
            .prop(Prop::u32("output-tag", "輸出 Tag ID").default(0).required(true))
            .prop(Prop::f64("initial-a", "a 的初始值").default(0.0).required(false))
            .prop(Prop::f64("initial-b", "b 的初始值").default(0.0).required(false))
            .build()
    }

    // ── 初始化 ────────────────────────────────────────────
    fn init(props: &Props, _wiring: &Wiring) -> Result<Self, String> {
        let operator = props.get_str("operator")
            .and_then(|s| s.chars().next())
            .ok_or("missing operator")?;

        Ok(MathOpNode {
            operator,
            output_tag: props.get_u32("output-tag").unwrap_or(0),
            val_a:      props.get_f64("initial-a").unwrap_or(0.0),
            val_b:      props.get_f64("initial-b").unwrap_or(0.0),
        })
    }

    // ── 核心邏輯 ──────────────────────────────────────────
    fn process(&mut self, input_port: u32, msgs: &[FlowMsg]) -> Result<NodeOutput, String> {
        let mut output = NodeOutput::new();

        for msg in msgs {
            // PDK helper：as_f64() 接受任意數值族，自動轉型
            // （能到這裡代表 cast node 已處理好型別，或 port 宣告 numeric）
            let v = msg.value.as_f64()
                .ok_or_else(|| format!("port {} 收到非數值型別", input_port))?;

            // 更新對應 port 的值
            match input_port {
                0 => self.val_a = v,
                1 => self.val_b = v,
                _ => return Err(format!("未知的 input port: {}", input_port)),
            }

            // 計算
            let result = match self.operator {
                '+' => self.val_a + self.val_b,
                '-' => self.val_a - self.val_b,
                '*' => self.val_a * self.val_b,
                '÷' => {
                    if self.val_b == 0.0 {
                        return Err("除以零".to_string());
                    }
                    self.val_a / self.val_b
                }
                _ => return Err(format!("未知的運算子: {}", self.operator)),
            };

            // 組成輸出 msg（PDK helper：from_msg 保留 timestamp / quality）
            output.push(0, FlowMsg::from_msg(&msg)
                .tag_id(self.output_tag)
                .value(TagValue::F64Val(result)));
        }

        Ok(output)
    }

    // Transform node 不實作 tick
    fn tick(&mut self) -> Result<NodeOutput, String> {
        Ok(NodeOutput::empty())
    }
}
```

### 5.3 完整範例：移動平均 Node

```rust
use iiot_flow_pdk::prelude::*;

node_impl!(MovingAvgNode);

struct MovingAvgNode {
    window: usize,
    output_tag: u32,
    buffer: std::collections::VecDeque<f64>,
}

impl FlowNode for MovingAvgNode {

    fn describe() -> NodeSpec {
        NodeSpec::builder()
            .name("iiot:moving-avg")
            .version("1.0.0")
            .kind(NodeKind::Transform)
            .label("移動平均")
            .category("math")
            .color("#27AE60")
            .input(InputPort::new(0, "in").numeric())
            .output(OutputPort::new(0, "avg").kind(ValueKind::F64Val))
            .join_strategy(JoinStrategy::Any)
            .prop(Prop::u32("window", "窗口大小（樣本數）").default(10).required(true))
            .prop(Prop::u32("output-tag", "輸出 Tag ID").default(0).required(true))
            .build()
    }

    fn init(props: &Props, _wiring: &Wiring) -> Result<Self, String> {
        let window = props.get_u32("window").unwrap_or(10) as usize;
        if window == 0 { return Err("window 必須 > 0".to_string()); }
        Ok(MovingAvgNode {
            window,
            output_tag: props.get_u32("output-tag").unwrap_or(0),
            buffer: std::collections::VecDeque::with_capacity(window),
        })
    }

    fn process(&mut self, _port: u32, msgs: &[FlowMsg]) -> Result<NodeOutput, String> {
        let mut output = NodeOutput::new();

        for msg in msgs {
            let v = msg.value.as_f64().ok_or("非數值型別")?;

            if self.buffer.len() == self.window {
                self.buffer.pop_front();
            }
            self.buffer.push_back(v);

            let avg = self.buffer.iter().sum::<f64>() / self.buffer.len() as f64;

            output.push(0, FlowMsg::from_msg(msg)
                .tag_id(self.output_tag)
                .value(TagValue::F64Val(avg)));
        }

        Ok(output)
    }

    fn tick(&mut self) -> Result<NodeOutput, String> { Ok(NodeOutput::empty()) }
}
```

### 5.4 完整範例：閾值過濾 Node（多 output port）

```rust
use iiot_flow_pdk::prelude::*;

node_impl!(ThresholdNode);

struct ThresholdNode {
    threshold: f64,
}

impl FlowNode for ThresholdNode {

    fn describe() -> NodeSpec {
        NodeSpec::builder()
            .name("iiot:threshold")
            .version("1.0.0")
            .kind(NodeKind::Transform)
            .label("閾值過濾")
            .category("logic")
            .color("#E67E22")
            .input(InputPort::new(0, "in").numeric())
            .output(OutputPort::new(0, "above").kind(ValueKind::Numeric))  // 高於閾值
            .output(OutputPort::new(1, "below").kind(ValueKind::Numeric))  // 低於/等於閾值
            .join_strategy(JoinStrategy::Any)
            .prop(Prop::f64("threshold", "閾值").default(0.0).required(true))
            .build()
    }

    fn init(props: &Props, _wiring: &Wiring) -> Result<Self, String> {
        Ok(ThresholdNode {
            threshold: props.get_f64("threshold").unwrap_or(0.0),
        })
    }

    fn process(&mut self, _port: u32, msgs: &[FlowMsg]) -> Result<NodeOutput, String> {
        let mut output = NodeOutput::new();

        for msg in msgs {
            let v = msg.value.as_f64().ok_or("非數值型別")?;
            // 依閾值送到不同 output port
            if v > self.threshold {
                output.push(0, msg.clone());  // port-0: above
            } else {
                output.push(1, msg.clone());  // port-1: below
            }
        }

        Ok(output)
    }

    fn tick(&mut self) -> Result<NodeOutput, String> { Ok(NodeOutput::empty()) }
}
```

### 5.5 編譯與打包

```bash
# 編譯為 WASM
cargo build --target wasm32-unknown-unknown --release

# 打包成 WASM Component（必要步驟）
wasm-tools component new \
    target/wasm32-unknown-unknown/release/my_math_node.wasm \
    --adapt wasi_snapshot_preview1.reactor.wasm \
    -o my-math-node.wasm

# 驗證
iiot-flow node validate my-math-node.wasm
```

---

## 6. C 開發指引

### 6.1 專案設定

```bash
# 產生骨架
iiot-flow new my-filter --lang c

# 目錄結構
my-filter/
├── CMakeLists.txt
├── include/
│   ├── iiot_flow.h          ← wit-bindgen-c 自動產生，不要手動修改
│   └── iiot_flow_pdk.h      ← PDK 輔助函式
└── src/
    └── my_filter.c
```

### 6.2 CMakeLists.txt

```cmake
cmake_minimum_required(VERSION 3.20)
project(my-filter C)

set(CMAKE_C_STANDARD 11)

# WASM target
set(CMAKE_SYSTEM_NAME Generic)
set(CMAKE_C_COMPILER clang)
set(CMAKE_C_FLAGS "--target=wasm32-unknown-unknown -nostdlib -Wl,--no-entry -Wl,--export-all")

add_library(my-filter SHARED src/my_filter.c)
target_include_directories(my-filter PRIVATE include)
set_target_properties(my-filter PROPERTIES SUFFIX ".wasm")
```

### 6.3 完整範例：閾值過濾 Node（C）

```c
#include "iiot_flow.h"
#include "iiot_flow_pdk.h"
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

// ── 節點狀態 ──────────────────────────────────────────────
static double g_threshold = 0.0;

// ── describe()：靜態描述 ──────────────────────────────────
void exports_iiot_flow_node_spec_iface_describe(
    exports_iiot_flow_node_descriptor_node_spec_t *ret)
{
    // 使用 PDK C helper 簡化結構填充
    iiot_pdk_node_spec_init(ret);

    iiot_pdk_set_str(&ret->name,        "iiot:threshold");
    iiot_pdk_set_str(&ret->version,     "1.0.0");
    iiot_pdk_set_str(&ret->label,       "閾值過濾");
    iiot_pdk_set_str(&ret->category,    "logic");
    iiot_pdk_set_str(&ret->color,       "#E67E22");
    iiot_pdk_set_str(&ret->description, "依閾值將輸入分到 above / below 兩個 output port");
    ret->kind = IIOT_NODE_KIND_TRANSFORM;
    ret->join_strategy = IIOT_JOIN_STRATEGY_ANY;

    // Input port（numeric：接受任意數值族，自動 cast）
    iiot_pdk_add_input_port(ret, 0, "in", IIOT_VALUE_KIND_NUMERIC, IIOT_PORT_ROLE_DATA);

    // Output ports
    iiot_pdk_add_output_port(ret, 0, "above", IIOT_VALUE_KIND_NUMERIC);
    iiot_pdk_add_output_port(ret, 1, "below", IIOT_VALUE_KIND_NUMERIC);

    // Props
    iiot_pdk_add_prop_f64(ret, "threshold", "閾值", "0.0", true);
}

// ── init()：讀取 props ────────────────────────────────────
bool exports_iiot_flow_node_spec_iface_init(
    iiot_flow_types_string_t *props_json,
    iiot_flow_types_string_t *wiring_json,
    iiot_flow_types_string_t *error)
{
    // PDK JSON helper
    iiot_pdk_json_t *props = iiot_pdk_json_parse(props_json->ptr, props_json->len);
    if (!props) {
        iiot_pdk_set_error(error, "invalid props JSON");
        return false;
    }

    g_threshold = iiot_pdk_json_get_f64(props, "threshold", 0.0);
    iiot_pdk_json_free(props);
    return true;
}

// ── process()：核心邏輯 ───────────────────────────────────
bool exports_iiot_flow_node_spec_iface_process(
    uint32_t input_port,
    iiot_flow_types_list_flow_msg_t *msgs,
    exports_iiot_flow_node_spec_iface_node_output_t *ret,
    iiot_flow_types_string_t *error)
{
    iiot_pdk_output_init(ret);

    for (size_t i = 0; i < msgs->len; i++) {
        iiot_flow_types_flow_msg_t *msg = &msgs->ptr[i];

        // PDK helper：as_f64 接受任意數值族
        double v;
        if (!iiot_pdk_value_as_f64(&msg->value, &v)) {
            iiot_pdk_set_error(error, "non-numeric value");
            return false;
        }

        // 依閾值分到不同 output port
        uint32_t out_port = (v > g_threshold) ? 0 : 1;
        iiot_pdk_output_push(ret, out_port, msg);
    }

    return true;
}

// ── tick()：transform node 不實作 ─────────────────────────
bool exports_iiot_flow_node_spec_iface_tick(
    exports_iiot_flow_node_spec_iface_node_output_t *ret,
    iiot_flow_types_string_t *error)
{
    iiot_pdk_output_init(ret);
    ret->outputs.len = 0;
    return true;
}
```

### 6.4 編譯

```bash
mkdir build && cd build
cmake .. -DCMAKE_BUILD_TYPE=Release
make

# 打包成 WASM Component
wasm-tools component new my-filter.wasm \
    --adapt wasi_snapshot_preview1.reactor.wasm \
    -o my-filter-component.wasm

iiot-flow node validate my-filter-component.wasm
```

---

## 7. C++ 開發指引

C++ 使用與 C 相同的 wit-bindgen-c 產生的 binding，但 PDK 提供了 C++ wrapper header 讓程式碼更簡潔。

### 7.1 C++ PDK Wrapper

```cpp
// include/iiot_flow_pdk.hpp
#pragma once
#include "iiot_flow.h"
#include <string>
#include <vector>
#include <optional>

namespace iiot {

// TagValue 的 C++ 包裝，提供 as_f64() 等輔助方法
struct Value {
    iiot_flow_types_tag_value_t raw;

    std::optional<double> as_f64() const;
    std::optional<int64_t> as_i64() const;
    std::optional<std::string> as_str() const;

    static Value f64(double v);
    static Value f32(float v);
    static Value i32(int32_t v);
};

// FlowMsg 的 C++ 包裝
struct Msg {
    iiot_flow_types_flow_msg_t raw;

    uint32_t tag_id() const { return raw.tag_id; }
    uint64_t timestamp() const { return raw.timestamp; }
    uint8_t  quality() const { return raw.quality; }
    Value    value() const { return Value{raw.value}; }

    // 建立輸出 msg（繼承 timestamp / quality）
    static Msg from(const Msg& src, uint32_t tag_id, Value value);
};

// NodeOutput 的 C++ 包裝
struct Output {
    exports_iiot_flow_node_spec_iface_node_output_t raw;
    Output();
    void push(uint32_t port_id, const Msg& msg);
};

// NodeSpec builder（C++ fluent API）
class NodeSpecBuilder { /* ... */ };

} // namespace iiot
```

### 7.2 完整範例：移動平均 Node（C++）

```cpp
#include "iiot_flow_pdk.hpp"
#include <deque>
#include <numeric>

// ── 節點狀態 ──────────────────────────────────────────────
static struct {
    size_t window    = 10;
    uint32_t out_tag = 0;
    std::deque<double> buffer;
} g_state;

// ── describe() ────────────────────────────────────────────
extern "C" void exports_iiot_flow_node_spec_iface_describe(
    exports_iiot_flow_node_descriptor_node_spec_t *ret)
{
    iiot::NodeSpecBuilder builder;
    builder
        .name("iiot:moving-avg-cpp")
        .version("1.0.0")
        .kind(IIOT_NODE_KIND_TRANSFORM)
        .label("移動平均 (C++)")
        .category("math")
        .color("#27AE60")
        .input(0, "in",  IIOT_VALUE_KIND_NUMERIC)
        .output(0, "avg", IIOT_VALUE_KIND_F64_VAL)
        .join_any()
        .prop_u32("window",     "窗口大小", 10,  true)
        .prop_u32("output-tag", "輸出 Tag", 0,   true)
        .build(ret);
}

// ── init() ────────────────────────────────────────────────
extern "C" bool exports_iiot_flow_node_spec_iface_init(
    iiot_flow_types_string_t *props_json,
    iiot_flow_types_string_t *,
    iiot_flow_types_string_t *error)
{
    auto props = iiot::Props::parse(props_json);
    g_state.window  = props.get_u32("window", 10);
    g_state.out_tag = props.get_u32("output-tag", 0);
    if (g_state.window == 0) {
        iiot::set_error(error, "window 必須 > 0");
        return false;
    }
    g_state.buffer.clear();
    return true;
}

// ── process() ─────────────────────────────────────────────
extern "C" bool exports_iiot_flow_node_spec_iface_process(
    uint32_t,
    iiot_flow_types_list_flow_msg_t *msgs,
    exports_iiot_flow_node_spec_iface_node_output_t *ret,
    iiot_flow_types_string_t *error)
{
    iiot::Output output;

    for (size_t i = 0; i < msgs->len; ++i) {
        iiot::Msg msg{msgs->ptr[i]};
        auto v = msg.value().as_f64();
        if (!v) { iiot::set_error(error, "非數值"); return false; }

        if (g_state.buffer.size() == g_state.window)
            g_state.buffer.pop_front();
        g_state.buffer.push_back(*v);

        double avg = std::accumulate(g_state.buffer.begin(),
                                     g_state.buffer.end(), 0.0)
                     / static_cast<double>(g_state.buffer.size());

        output.push(0, iiot::Msg::from(msg, g_state.out_tag, iiot::Value::f64(avg)));
    }

    *ret = output.raw;
    return true;
}

extern "C" bool exports_iiot_flow_node_spec_iface_tick(
    exports_iiot_flow_node_spec_iface_node_output_t *ret,
    iiot_flow_types_string_t *)
{
    iiot::Output out; *ret = out.raw; return true;
}
```

---

## 8. 測試框架

PDK 提供一個輕量的本地測試框架，讓開發者在不需要完整 Runtime 的情況下驗證 node 行為。

### 8.1 Rust 測試（使用 PDK test 模組）

```rust
// src/lib.rs 底部，或獨立的 tests/ 目錄

#[cfg(test)]
mod tests {
    use iiot_flow_pdk::test::*;
    use super::*;   // 引入 MathOpNode

    // ── 基本功能測試 ─────────────────────────────────────
    #[test]
    fn test_add_basic() {
        // 建立 node 實例（模擬 init）
        let mut node = NodeRunner::new::<MathOpNode>(props! {
            "operator"   => "+",
            "output-tag" => 201u32,
            "initial-a"  => 0.0f64,
            "initial-b"  => 0.0f64,
        });

        // 送入 port-0（a = 10.0）
        node.process(0, msgs![f32: (tag=101, val=10.0)]);

        // 送入 port-1（b = 5.0）→ all-or-initial，兩個都有了，觸發計算
        let output = node.process(1, msgs![f32: (tag=102, val=5.0)]);

        // 驗證輸出
        assert_output!(output, port=0, count=1);
        assert_value!(output[0][0], f64: 15.0);
        assert_tag!(output[0][0], 201);
    }

    // ── Auto Cast 測試 ────────────────────────────────────
    // 驗證即使上游送來 i16，node 也能正確處理（cast 已在 pipeline 發生）
    #[test]
    fn test_input_after_cast() {
        let mut node = NodeRunner::new::<MathOpNode>(props! {
            "operator"   => "*",
            "output-tag" => 201u32,
            "initial-a"  => 2.0f64,
            "initial-b"  => 0.0f64,
        });

        // 模擬 cast node 已將 i16 轉為 f64（node 宣告 numeric）
        // 直接送 f64 值（cast node 的輸出）
        let output = node.process(1, msgs![f64: (tag=102, val=3.0)]);

        assert_value!(output[0][0], f64: 6.0);
    }

    // ── 邊界條件測試 ──────────────────────────────────────
    #[test]
    fn test_divide_by_zero() {
        let mut node = NodeRunner::new::<MathOpNode>(props! {
            "operator"   => "÷",
            "output-tag" => 201u32,
            "initial-a"  => 10.0f64,
            "initial-b"  => 0.0f64,
        });

        // 送入 b = 0，應該回傳 Err
        let result = node.process_raw(1, msgs![f64: (tag=102, val=0.0)]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("除以零"));
    }

    // ── quality 傳遞測試 ──────────────────────────────────
    #[test]
    fn test_quality_propagation() {
        let mut node = NodeRunner::new::<MathOpNode>(props! {
            "operator"   => "+",
            "output-tag" => 201u32,
            "initial-a"  => 0.0f64,
            "initial-b"  => 0.0f64,
        });

        node.process(0, msgs![f64: (tag=101, val=5.0, quality=0)]);
        // 送入低品質資料（quality != 0 表示異常）
        let output = node.process(1, msgs![f64: (tag=102, val=3.0, quality=0x80)]);

        // 輸出的 quality 應該是最差的那個
        assert_quality!(output[0][0], 0x80);
    }

    // ── describe() 驗證 ───────────────────────────────────
    #[test]
    fn test_describe_valid() {
        let spec = MathOpNode::describe();
        assert_eq!(spec.inputs.len(), 2);
        assert_eq!(spec.outputs.len(), 1);
        assert_eq!(spec.join_strategy, JoinStrategy::AllOrInitial);
        // 確認 props 有 operator 欄位
        assert!(spec.props.iter().any(|p| p.key == "operator"));
    }
}
```

### 8.2 C 測試（使用 PDK C 測試輔助函式）

```c
// tests/test_threshold.c
#include "iiot_flow_pdk_test.h"
#include <assert.h>

void test_above_threshold() {
    // 初始化 node
    iiot_test_props_t *props = iiot_test_props_new();
    iiot_test_props_set_f64(props, "threshold", 50.0);
    assert(iiot_test_init(props, NULL) == true);

    // 建立測試訊息（f32 值 75.0）
    iiot_test_msgs_t *msgs = iiot_test_msgs_new();
    iiot_test_msgs_add_f32(msgs, /*tag=*/101, /*val=*/75.0f, /*quality=*/0);

    // 執行 process
    iiot_test_output_t *out = iiot_test_process(0, msgs);

    // 驗證輸出在 port-0（above）
    assert(iiot_test_output_port_count(out, 0) == 1);
    assert(iiot_test_output_port_count(out, 1) == 0);
    assert(iiot_test_output_value_f64(out, 0, 0) == 75.0);

    iiot_test_output_free(out);
    iiot_test_msgs_free(msgs);
    iiot_test_props_free(props);
    printf("test_above_threshold: PASS\n");
}

void test_below_threshold() {
    iiot_test_props_t *props = iiot_test_props_new();
    iiot_test_props_set_f64(props, "threshold", 50.0);
    iiot_test_init(props, NULL);

    iiot_test_msgs_t *msgs = iiot_test_msgs_new();
    iiot_test_msgs_add_f32(msgs, 101, 30.0f, 0);

    iiot_test_output_t *out = iiot_test_process(0, msgs);

    // 值 30.0 <= 50.0，應送到 port-1（below）
    assert(iiot_test_output_port_count(out, 0) == 0);
    assert(iiot_test_output_port_count(out, 1) == 1);

    iiot_test_output_free(out);
    iiot_test_msgs_free(msgs);
    iiot_test_props_free(props);
    printf("test_below_threshold: PASS\n");
}

int main() {
    test_above_threshold();
    test_below_threshold();
    printf("All tests passed.\n");
    return 0;
}
```

### 8.3 CLI 執行測試

```bash
# Rust（標準 cargo test）
cargo test

# C（編譯並執行測試）
iiot-flow node test ./my-filter.wasm

# 詳細模式（顯示每個 msg 的輸入輸出）
iiot-flow node test ./my-filter.wasm --verbose

# 輸出：
# [SEND]  port=0  tag=101  value=f32(75.0)  quality=0
# [RECV]  port=0  tag=101  value=f32(75.0)  quality=0  ✅
# [RECV]  port=1  (empty)                              ✅
```

---

## 9. 最佳實踐

### 9.1 describe() 的設計原則

**宣告最精確的型別，讓 Pipeline 幫你做 cast：**

```rust
// ❌ 避免：全部宣告 any，失去自動 cast 和型別檢查
.input(InputPort::new(0, "in").kind(ValueKind::Any))

// ✅ 建議：宣告 numeric，接受所有數值族，非數值在 Pipeline 報錯
.input(InputPort::new(0, "in").numeric())

// ✅ 更精確：如果只接受 f32，直接宣告，讓 Pipeline 自動 cast 其他數值族
.input(InputPort::new(0, "in").kind(ValueKind::F32Val))
```

**output 型別要固定，不要回傳 any：**

```rust
// ❌ 避免：output 宣告 any，下游 node 無法推導型別
.output(OutputPort::new(0, "result").kind(ValueKind::Any))

// ✅ 建議：明確宣告輸出型別
.output(OutputPort::new(0, "result").kind(ValueKind::F64Val))
```

### 9.2 process() 的設計原則

**保持 quality 傳遞語意：**

```rust
// ✅ 使用 FlowMsg::from_msg() 保留 timestamp 和 quality
output.push(0, FlowMsg::from_msg(msg).tag_id(out_tag).value(...));

// ❌ 避免：手動建構 msg 忘記複製 quality
output.push(0, FlowMsg { tag_id: out_tag, value: ..., quality: 0, .. });
// 如果上游送來 quality=0x80（感測器異常），下游應該也要知道
```

**多輸入 node 要正確維護狀態：**

```rust
// ✅ all-or-initial 語意：每次 process 只更新對應 port 的值，然後計算
fn process(&mut self, port: u32, msgs: &[FlowMsg]) -> Result<NodeOutput, String> {
    for msg in msgs {
        match port {
            0 => self.val_a = msg.value.as_f64().unwrap(),
            1 => self.val_b = msg.value.as_f64().unwrap(),
            _ => {}
        }
        // 每次任一 port 更新都計算輸出
        let result = self.val_a + self.val_b;
        output.push(0, ...);
    }
}
```

**不要在 process() 做阻塞 I/O：**

```rust
// ❌ 嚴禁：process() 裡做阻塞操作（WASM 環境不支援，且會卡住整個 DAG）
fn process(&mut self, ...) {
    let data = std::fs::read("config.json").unwrap();  // ❌
    let resp = http_client.get("...").send().unwrap();  // ❌
}

// ✅ 所有外部 I/O 透過 init() 時的 props 預先載入，或透過 host function
```

### 9.3 Props 設計原則

```rust
// ✅ 提供合理的 default，降低使用者設定負擔
.prop(Prop::u32("window", "窗口大小").default(10).required(false))

// ✅ required=true 的 prop 一定要有 default，否則 GUI 無法正確顯示
.prop(Prop::select("operator", "運算子")
    .choices(["+", "-", "*", "÷"])
    .default("+")       // 必填，但給預設值
    .required(true))

// ✅ 複雜設定用 prop-json，但要在 description 說明格式
.prop(Prop::json("rules", "路由規則")
    .default("[]")
    .description(r#"格式：[{"port": 0, "expr": "value > 80"}]"#))
```

### 9.4 Node 命名規範

```
格式：  <組織或作者>:<功能名稱>
範例：  iiot:math-op        （官方 PDK node）
        myco:temp-filter    （公司自定義）
        community:modbus-rtu（社群貢獻）

版本：  使用 semver，breaking change 升 major
        1.0.0 → 1.0.1  （bug fix）
        1.0.0 → 1.1.0  （新增 prop 但向後相容）
        1.0.0 → 2.0.0  （改變 port 數量或型別）
```

---

## 10. Node Registry 發佈

### 10.1 發佈前檢查清單

```bash
# 1. 執行所有測試
cargo test          # Rust
iiot-flow node test ./my-node.wasm   # 所有語言

# 2. 驗證 WIT 規格符合
iiot-flow node validate ./my-node.wasm

# 3. 檢查 describe() 的完整性
iiot-flow node info ./my-node.wasm
# 輸出：
# Name:        iiot:moving-avg
# Version:     1.0.0
# Kind:        transform
# Inputs:      1 (in: numeric)
# Outputs:     1 (avg: f64-val)
# Join:        any
# Props:       window(u32, default=10), output-tag(u32, default=0)
# Category:    math
# Size:        48.3 KB

# 4. 確認 wasm 大小合理（建議 < 200KB）
ls -lh ./my-node.wasm
```

### 10.2 發佈流程

```bash
# 登入（使用 API token）
iiot-flow login --token <your-token>

# 發佈
iiot-flow node publish ./my-node.wasm

# 指定版本（若 Cargo.toml 版本不符）
iiot-flow node publish ./my-node.wasm --version 1.2.0

# 發佈後可在 Registry 查詢
iiot-flow node search "moving-avg"
# iiot:moving-avg@1.0.0  移動平均  math  48.3KB
```

### 10.3 版本更新策略

```bash
# patch（bug fix，不改 describe() 的 port/prop 定義）
iiot-flow node publish ./my-node.wasm --version 1.0.1

# minor（新增 prop 並提供 default，向後相容）
iiot-flow node publish ./my-node.wasm --version 1.1.0

# major（改變 port 數量、型別或移除 prop，破壞性變更）
# 舊版本繼續存在 Registry，使用中的 flow.json 不受影響
iiot-flow node publish ./my-node.wasm --version 2.0.0
```

---

