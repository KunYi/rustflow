// host/src/main.rs
// IIoT Flow Fusion Host
//
// 用兩個 bindgen! 分別對應兩個 WIT world：
//   FlowNode          ← flow-node          (Source / Node A/B/C)
//   FlowNodeWithHost  ← flow-node-with-host (Sink)
//
// Host Function (host-api) 透過 Linker::func_wrap 手動掛載
use wasmtime::error::{ Context, Result };
// use anyhow::bail;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;
use wasmtime::component::{bindgen, Component, Linker, ResourceTable };
use wasmtime::{ bail, Config, Engine, Store };
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiView, WasiCtxView};

// ── bindings for flow-node world (Source, Node A/B/C) ────────────────────────
bindgen!({
    world: "flow-node",
    path:  "../wit",
});

// ── bindings for flow-node-with-host world (Sink) ────────────────────────────
mod sink_bindings {
    wasmtime::component::bindgen!({
        world: "flow-node-with-host",
        path:  "../wit",
        with: {
            "iiot:flow/types@0.1.0": crate::iiot::flow::types,
        }
    });
}

mod fused_bindings {
    wasmtime::component::bindgen!({
        world: "fused-pipeline",
        path:  "../wit",
        // with: {
        //     "iiot:flow/types@0.1.0": crate::iiot::flow::types,
        // }
    });
}

use iiot::flow::types::{FlowMsg, TagValue, ValueKind};

// ════════════════════════════════════════════════════════════════════════════
// Tag Registry
// ════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct TagMeta {
    pub name:          String,
    pub unit:          String,
    pub mqtt_topic:    String,
    pub historian_tag: String,
    pub alarm_group:   String,
    pub eng_low:       f64,
    pub eng_high:      f64,
}

pub struct TagRegistry {
    tags:       HashMap<u32, TagMeta>,
    name_to_id: HashMap<String, u32>,
    next_id:    u32,
}

impl TagRegistry {
    pub fn new() -> Self {
        TagRegistry { tags: HashMap::new(), name_to_id: HashMap::new(), next_id: 1 }
    }

    pub fn get_or_create(&mut self, name: &str, meta: TagMeta) -> u32 {
        if let Some(&id) = self.name_to_id.get(name) { return id; }
        let id = self.next_id;
        self.next_id += 1;
        self.name_to_id.insert(name.to_string(), id);
        self.tags.insert(id, meta);
        id
    }

    pub fn get_attr(&self, tag_id: u32, key: &str) -> Option<String> {
        let m = self.tags.get(&tag_id)?;
        match key {
            "name"          => Some(m.name.clone()),
            "unit"          => Some(m.unit.clone()),
            "mqtt_topic"    => Some(m.mqtt_topic.clone()),
            "historian_tag" => Some(m.historian_tag.clone()),
            "alarm_group"   => Some(m.alarm_group.clone()),
            _               => None,
        }
    }

    pub fn get_eng_range(&self, tag_id: u32) -> Option<(f64, f64)> {
        let m = self.tags.get(&tag_id)?;
        Some((m.eng_low, m.eng_high))
    }
}

// ════════════════════════════════════════════════════════════════════════════
// msg_id Allocator
// ════════════════════════════════════════════════════════════════════════════

static MSG_COUNTER: AtomicU32 = AtomicU32::new(1);

fn next_msg_id() -> u32 {
    loop {
        let id = MSG_COUNTER.fetch_add(1, Ordering::Relaxed);
        if id != 0 { return id; }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Store State
// ════════════════════════════════════════════════════════════════════════════

pub struct HostState {
    wasi:     WasiCtx,
    table:    ResourceTable,
    registry: Arc<RwLock<TagRegistry>>,
}

impl WasiView for HostState {
    fn ctx(&mut self) -> WasiCtxView<'_> { WasiCtxView { ctx: &mut self.wasi, table: &mut self.table } }
}

fn make_store(engine: &Engine, registry: Arc<RwLock<TagRegistry>>) -> Store<HostState> {
    let wasi = WasiCtxBuilder::new().inherit_stdio().build();
    let table: ResourceTable = ResourceTable::new();
    Store::new(engine, HostState { wasi, table, registry })
}

// ════════════════════════════════════════════════════════════════════════════
// Deploy 型別檢查
// ════════════════════════════════════════════════════════════════════════════

fn type_check(
    from: &str, from_out: ValueKind,
    to:   &str, to_in:    &[ValueKind],
) -> Result<()> {
    let ok = to_in.iter().any(|k| *k == ValueKind::Any || *k == from_out)
          || from_out == ValueKind::Any;
    if !ok {
        bail!("❌ 型別不符  {} ({:?}) → {} ({:?})", from, from_out, to, to_in);
    }
    println!("  ✅ {:>30} → {:<30} ({:?})", from, to, from_out);
    Ok(())
}

// ════════════════════════════════════════════════════════════════════════════
// 通用 Node 包裝（flow-node world）
// ════════════════════════════════════════════════════════════════════════════

struct Node {
    store:    Store<HostState>,
    bindings: FlowNode,
    pub name: String,
}

impl Node {
    fn load(engine: &Engine, linker: &Linker<HostState>,
            registry: Arc<RwLock<TagRegistry>>, path: &str) -> Result<Self> {
        let component = Component::from_file(engine, path)
            .with_context(|| format!("載入失敗：{path}"))?;
        let mut store = make_store(engine, registry);
        let bindings = FlowNode::instantiate(&mut store, &component, linker)?;
        let name = bindings.iiot_flow_meta().call_name(&mut store)?;
        Ok(Node { store, bindings, name })
    }

    fn meta_input_types(&mut self) -> Result<Vec<ValueKind>> {
        Ok(self.bindings.iiot_flow_meta().call_accepted_input_types(&mut self.store)?)
    }
    fn meta_output_type(&mut self) -> Result<ValueKind> {
        Ok(self.bindings.iiot_flow_meta().call_output_type(&mut self.store)?)
    }
    fn process(&mut self, msg: &FlowMsg) -> Result<Vec<FlowMsg>> {
        Ok(self.bindings.iiot_flow_node().call_process(&mut self.store, msg)?.msgs)
    }
    fn process_raw(&mut self, tag_id: u32, msg_id: u32, raw: &[u8]) -> Result<Vec<FlowMsg>> {
        Ok(self.bindings.iiot_flow_node()
            .call_process_raw(&mut self.store, tag_id, msg_id, raw)?.msgs)
    }
    fn save_state(&mut self) -> Result<Vec<u8>> {
        Ok(self.bindings.iiot_flow_node().call_save_state(&mut self.store)?)
    }
    fn load_state(&mut self, s: Vec<u8>) -> Result<()> {
        Ok(self.bindings.iiot_flow_node().call_load_state(&mut self.store, &s)?)
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Sink Node 包裝（flow-node-with-host world）
// ════════════════════════════════════════════════════════════════════════════

struct SinkNode {
    store:    Store<HostState>,
    bindings: sink_bindings::FlowNodeWithHost,
    pub name: String,
}

impl SinkNode {
    fn load(engine: &Engine, _linker: &Linker<HostState>,
            registry: Arc<RwLock<TagRegistry>>, path: &str) -> Result<Self> {
        let component = Component::from_file(engine, path)
            .with_context(|| format!("載入失敗：{path}"))?;
        let mut store = make_store(engine, registry);

        // Sink 用自己的 linker（包含 host-api）
        let mut sink_linker: Linker<HostState> = Linker::new(engine);
        wasmtime_wasi::p2::add_to_linker_sync(&mut sink_linker)?;
        add_host_api_to_linker(&mut sink_linker)?;

        let bindings = sink_bindings::FlowNodeWithHost::instantiate(
            &mut store, &component, &sink_linker)?;
        let name = bindings.iiot_flow_meta().call_name(&mut store)?;
        Ok(SinkNode { store, bindings, name })
    }

    fn meta_input_types(&mut self) -> Result<Vec<ValueKind>> {
        Ok(self.bindings.iiot_flow_meta().call_accepted_input_types(&mut self.store)?)
    }
    fn meta_output_type(&mut self) -> Result<ValueKind> {
        Ok(self.bindings.iiot_flow_meta().call_output_type(&mut self.store)?)
    }
    fn process(&mut self, msg: &FlowMsg) -> Result<()> {
        self.bindings.iiot_flow_node().call_process(&mut self.store, msg)?;
        Ok(())
    }
}

// ════════════════════════════════════════════════════════════════
// Fused Pipeline 包裝（Fusion + AOT 後使用）
// ════════════════════════════════════════════════════════════════

struct FusedPipeline {
    store:    Store<HostState>,
    bindings: fused_bindings::FusedPipeline,
    pub name: String,
}

impl FusedPipeline {
    fn load(engine: &Engine,
            registry: Arc<RwLock<TagRegistry>>,
            path: &str) -> Result<Self> {
        let component = if path.ends_with(".cwasm") {
            // AOT 預編譯版本：直接 mmap，無 JIT 開銷
            unsafe { Component::deserialize_file(engine, path) }
                .with_context(|| format!("載入 AOT 失敗：{path}"))?
        } else {
            // 一般 .wasm：JIT 編譯
            Component::from_file(engine, path)
                .with_context(|| format!("載入失敗：{path}"))?
        };

        let mut store = make_store(engine, Arc::clone(&registry));

        let mut fused_linker: Linker<HostState> = Linker::new(engine);
        wasmtime_wasi::p2::add_to_linker_sync(&mut fused_linker)?;
        add_host_api_to_linker(&mut fused_linker)?;

        let bindings = fused_bindings::FusedPipeline::instantiate(
            &mut store, &component, &fused_linker)?;
        let name = bindings.pipeline().call_name(&mut store)?;

        Ok(FusedPipeline { store, bindings, name })
    }

    // 核心：一次呼叫跑完整個 Pipeline
    fn run(&mut self, tag_id: u32, msg_id: u32, raw: &[u8]) -> Result<Vec<u8>> {
        Ok(self.bindings.pipeline()
            .call_run(&mut self.store, tag_id, msg_id, raw)?)
    }

    fn save_states(&mut self) -> Result<Vec<u8>> {
        Ok(self.bindings.pipeline().call_save_states(&mut self.store)?)
    }

    fn load_states(&mut self, states: Vec<u8>) -> Result<()> {
        Ok(self.bindings.pipeline().call_load_states(&mut self.store, &states)?)
    }
}

// ── 掛載 host-api Host Functions ─────────────────────────────────────────────
fn add_host_api_to_linker(linker: &mut Linker<HostState>) -> Result<()> {
    let mut root = linker.instance("iiot:flow/host-api@0.1.0")?;

    root.func_wrap("get-tag-attr", |ctx, (tag_id, key): (u32, String)| {
        let reg = ctx.data().registry.read().unwrap();
        Ok((reg.get_attr(tag_id, &key),))
    })?;

    root.func_wrap("get-eng-range", |ctx, (tag_id,): (u32,)| {
        let reg = ctx.data().registry.read().unwrap();
        Ok((reg.get_eng_range(tag_id),))
    })?;

    root.func_wrap("log-debug", |_ctx, (node_name, msg): (String, String)| {
        eprintln!("[WASM:{}] {}", node_name, msg);
        Ok(())
    })?;

    Ok(())
}

// ════════════════════════════════════════════════════════════════════════════
// Protobuf（Host 端）
// ════════════════════════════════════════════════════════════════════════════

mod proto {
    #[derive(prost::Message, Clone)]
    pub struct TagUpdate {
        #[prost(string,  tag = "1")] pub tag_id_str: String,
        #[prost(uint64,  tag = "2")] pub timestamp:  u64,
        #[prost(uint32,  tag = "3")] pub quality:    u32,
        #[prost(string,  tag = "4")] pub unit:       String,
        #[prost(double, optional, tag = "14")] pub f64_val: Option<f64>,
    }

    #[derive(prost::Message)]
    pub struct FlowResult {
        #[prost(uint32, tag = "1")] pub tag_id:     u32,
        #[prost(string, tag = "2")] pub tag_name:   String,
        #[prost(string, tag = "3")] pub mqtt_topic: String,
        #[prost(uint32, tag = "4")] pub msg_id:     u32,
        #[prost(double, tag = "5")] pub value:      f64,
        #[prost(uint64, tag = "6")] pub timestamp:  u64,
        #[prost(uint32, tag = "7")] pub quality:    u32,
    }
}

// ════════════════════════════════════════════════════════════════════════════
// 主程式
// ════════════════════════════════════════════════════════════════════════════

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let dir = if args.len() > 1 { args[1].as_str() } else { "." };

    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║  IIoT Flow Fusion Host                                       ║");
    println!("║  Source → NodeA → NodeB → NodeC → Sink                      ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    // ── Engine ───────────────────────────────────────────────────────────────
    let mut config = Config::new();
    config.wasm_component_model(true);
    let engine = Engine::new(&config)?;

    // 通用 linker（給 Source / Node A/B/C）
    let mut linker: Linker<HostState> = Linker::new(&engine);
    wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;

    // ── Tag Registry ─────────────────────────────────────────────────────────
    let registry = Arc::new(RwLock::new(TagRegistry::new()));
    {
        let mut reg = registry.write().unwrap();
        reg.get_or_create("plant1.motor3.temp", TagMeta {
            name:          "Motor 3 Temperature".to_string(),
            unit:          "°C".to_string(),
            mqtt_topic:    "factory/plant1/motor3/temperature".to_string(),
            historian_tag: "PI:plant1_motor3_temp".to_string(),
            alarm_group:   "critical".to_string(),
            eng_low: 0.0, eng_high: 150.0,
        });
    }

    // ── Step 1：載入 Nodes ───────────────────────────────────────────────────
    println!("▶ Step 1：載入 WASM Nodes...");
    let t0 = Instant::now();

    let mk = |wasm: &str| -> Result<Node> {
        Node::load(&engine, &linker, Arc::clone(&registry), &format!("{dir}/{wasm}"))
    };

    let mut source = mk("source_node.wasm")?;
    let mut node_a = mk("node_a.wasm")?;
    let mut node_b = mk("node_b.wasm")?;
    let mut node_c = mk("node_c.wasm")?;
    let mut sink   = SinkNode::load(
        &engine, &linker, Arc::clone(&registry), &format!("{dir}/sink_node.wasm"))?;

    println!("  {} │ {} │ {} │ {} │ {}",
        source.name, node_a.name, node_b.name, node_c.name, sink.name);
    println!("  ✅ 完成 ({:.1}ms)\n", t0.elapsed().as_secs_f64() * 1000.0);

    // ── Step 2：Deploy 型別檢查 ──────────────────────────────────────────────
    println!("▶ Step 2：Deploy 型別檢查...");

    let src_out  = source.meta_output_type()?;
    let a_in     = node_a.meta_input_types()?;
    let a_out    = node_a.meta_output_type()?;
    let b_in     = node_b.meta_input_types()?;
    let b_out    = node_b.meta_output_type()?;
    let c_in     = node_c.meta_input_types()?;
    let c_out    = node_c.meta_output_type()?;
    let sink_in  = sink.meta_input_types()?;

    type_check(&source.name, src_out, &node_a.name, &a_in)?;
    type_check(&node_a.name, a_out,   &node_b.name, &b_in)?;
    type_check(&node_b.name, b_out,   &node_c.name, &c_in)?;
    type_check(&node_c.name, c_out,   &sink.name,   &sink_in)?;
    println!("  ✅ 型別全部相容，允許 Deploy\n");

    // ── Step 3：模擬 TagUpdate 流 ────────────────────────────────────────────
    println!("▶ Step 3：模擬 Protocol Driver TagUpdate 流...\n");

    let inputs: &[(&str, f64, u32)] = &[
        ("plant1.motor3.temp", 25.0, 0),  // 正常 25°C → 77°F
        ("plant1.motor3.temp", 38.0, 0),  // 高溫 38°C → 100.4°F（超閾值）
        ("plant1.motor3.temp", 20.0, 2),  // quality=bad → filter
        ("plant1.motor3.temp", 22.0, 0),
        ("plant1.motor3.temp", 24.0, 0),
        ("plant1.motor3.temp", 26.0, 0),
        ("plant1.motor3.temp", 28.0, 0),
        ("plant1.motor3.temp", 30.0, 0),
        ("plant1.motor3.temp", 32.0, 0),  // 視窗滿後平均穩定
    ];

    for (tag_name, raw_val, quality) in inputs {
        use prost::Message as _;

        // Host 職責：分配 tag_id + msg_id
        let tag_id = registry.write().unwrap().get_or_create(tag_name, TagMeta {
            name: tag_name.to_string(), unit: "°C".to_string(),
            mqtt_topic: format!("iiot/{}", tag_name.replace('.', "/")),
            historian_tag: String::new(), alarm_group: "default".to_string(),
            eng_low: 0.0, eng_high: 150.0,
        });
        let msg_id = next_msg_id();

        // 組 Protobuf bytes（模擬 nng 進來的資料）
        let tu = proto::TagUpdate {
            tag_id_str: tag_name.to_string(),
            timestamp:  1_700_000_000_000 + msg_id as u64 * 100,
            quality:    *quality,
            unit:       "°C".to_string(),
            f64_val:    Some(*raw_val),
        };
        let mut bytes = Vec::new();
        tu.encode(&mut bytes)?;

        print!("  IN  {:<25} raw={:>5.1}°C  q={}  id={:>3} │ ",
               tag_name, raw_val, quality, msg_id);

        // ── Pipeline ─────────────────────────────────────────────────────
        let msgs = source.process_raw(tag_id, msg_id, &bytes)?;
        if msgs.is_empty() { println!("DROPPED (source)"); continue; }

        let msgs = run_node(&mut node_a, msgs)?;
        if msgs.is_empty() { println!("DROPPED (node-a)"); continue; }

        let msgs = run_node(&mut node_b, msgs)?;
        if msgs.is_empty() { println!("DROPPED (bad quality)"); continue; }

        let msgs = run_node(&mut node_c, msgs)?;
        if msgs.is_empty() { println!("DROPPED (node-c)"); continue; }

        // 取 Node C 的最終 avg 值顯示
        let avg = match msgs[0].value { TagValue::F64Val(v) => v, _ => 0.0 };

        // Sink：查 Registry + encode（副作用，不回傳 FlowMsg）
        sink.process(&msgs[0])?;

        let mqtt = registry.read().unwrap()
            .get_attr(tag_id, "mqtt_topic").unwrap_or_default();
        println!("OUT avg={:>8.4}°F  → {}", avg, mqtt);
    }

    // ── Step 4：Snapshot / Restore ──────────────────────────────────────────
    println!("\n▶ Step 4：Node C Snapshot / Restore...");
    let snap = node_c.save_state()?;
    println!("  Snapshot {} bytes", snap.len());

    let mut node_c2 = mk("node_c.wasm")?;
    node_c2.load_state(snap)?;

    let test_msg = FlowMsg {
        tag_id: 1, msg_id: 9999,
        value: TagValue::F64Val(200.0),
        timestamp: 0, quality: 0,
    };
    let r1 = node_c.process(&test_msg)?;
    let r2 = node_c2.process(&test_msg)?;
    let v1 = match r1[0].value { TagValue::F64Val(v) => v, _ => 0.0 };
    let v2 = match r2[0].value { TagValue::F64Val(v) => v, _ => 0.0 };
    println!("  原始={:.6}  還原={:.6}  差={:.2e}  {}",
        v1, v2, (v1-v2).abs(),
        if (v1-v2).abs() < 1e-10 { "✅" } else { "❌" });

    // ── Step 5：Benchmark ───────────────────────────────────────────────────
    println!("\n▶ Step 5：吞吐量 Benchmark (10萬筆)...");

    use prost::Message as _;
    let mut bench_bytes = Vec::new();
    proto::TagUpdate {
        tag_id_str: "plant1.motor3.temp".to_string(),
        timestamp: 0, quality: 0, unit: "°C".to_string(),
        f64_val: Some(25.0),
    }.encode(&mut bench_bytes)?;

    const N: u64 = 100_000;
    let t = Instant::now();
    for _ in 0..N {
        let mid  = next_msg_id();
        let msgs = source.process_raw(1, mid, &bench_bytes)?;
        let msgs = run_node(&mut node_a, msgs)?;
        let msgs = run_node(&mut node_b, msgs)?;
        let msgs = run_node(&mut node_c, msgs)?;
        sink.process(msgs.first().unwrap())?;
    }
    let el = t.elapsed();
    println!("  吞吐量   = {:.0} msgs/sec", (N as f64) / el.as_secs_f64());
    println!("  平均延遲 = {:.3}µs/msg",    (el.as_micros() as f64) / (N as f64));
    println!("  {}",
        if (el.as_micros() as f64) / (N as f64) < 500.0 { "✅ < 500µs 目標達成" }
        else { "⚠️  超過 500µs（請用 release build）" });

    Ok(())
}

fn run_node(node: &mut Node, msgs: Vec<FlowMsg>) -> Result<Vec<FlowMsg>> {
    let mut out = Vec::new();
    for msg in msgs { out.extend(node.process(&msg)?); }
    Ok(out)
}
