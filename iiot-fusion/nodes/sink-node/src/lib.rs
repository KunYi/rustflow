// nodes/sink-node/src/lib.rs
// Sink Node：FlowMsg → Protobuf encode
// world: flow-node-with-host（需要 import host-api 查 Tag Registry）

wit_bindgen::generate!({
    world: "flow-node-with-host",
    path: "../../wit",
});

use exports::iiot::flow::meta::Guest as MetaGuest;
use exports::iiot::flow::node::Guest as NodeGuest;
use iiot::flow::types::{FlowMsg, NodeOutput, TagValue, ValueKind};
use iiot::flow::host_api;

mod proto {
    #[derive(prost::Message)]
    pub struct FlowResult {
        #[prost(uint32, tag = "1")] pub tag_id:    u32,
        #[prost(string, tag = "2")] pub tag_name:  String,
        #[prost(string, tag = "3")] pub mqtt_topic: String,
        #[prost(uint32, tag = "4")] pub msg_id:    u32,
        #[prost(double, tag = "5")] pub value:     f64,
        #[prost(uint64, tag = "6")] pub timestamp: u64,
        #[prost(uint32, tag = "7")] pub quality:   u32,
        #[prost(string, tag = "8")] pub flow_id:   String,
    }
}

// 輸出緩衝區：process() 後 Host 呼叫 take_output_ptr/len 取走
static mut OUTPUT_BUF: Vec<u8> = Vec::new();

struct SinkNode;

impl MetaGuest for SinkNode {
    fn accepted_input_types() -> Vec<ValueKind> {
        vec![
            ValueKind::BoolVal, ValueKind::I32Val, ValueKind::U32Val,
            ValueKind::F32Val,  ValueKind::F64Val, ValueKind::ShortStr,
        ]
    }
    fn output_type() -> ValueKind { ValueKind::Any }
    fn name()    -> String { "sink-node:protobuf-flow-result".to_string() }
    fn version() -> String { "0.1.0".to_string() }
}

impl NodeGuest for SinkNode {
    fn process(msg: FlowMsg) -> NodeOutput {
        use prost::Message;

        // 唯一需要 Host Function 的地方
        let tag_name   = host_api::get_tag_attr(msg.tag_id, "name")
            .unwrap_or_else(|| format!("tag_{}", msg.tag_id));
        let mqtt_topic = host_api::get_tag_attr(msg.tag_id, "mqtt_topic")
            .unwrap_or_else(|| format!("iiot/tag/{}", msg.tag_id));

        let value_f64: f64 = match msg.value {
            TagValue::BoolVal(v) => if v { 1.0 } else { 0.0 },
            TagValue::I32Val(v)  => v as f64,
            TagValue::U32Val(v)  => v as f64,
            TagValue::F32Val(v)  => v as f64,
            TagValue::F64Val(v)  => v,
            _ => return NodeOutput { msgs: vec![] },
        };

        let result = proto::FlowResult {
            tag_id:    msg.tag_id,
            tag_name,
            mqtt_topic,
            msg_id:    msg.msg_id,
            value:     value_f64,
            timestamp: msg.timestamp,
            quality:   msg.quality as u32,
            flow_id:   "flow-temp-pipeline-v1".to_string(),
        };

        let mut buf = Vec::with_capacity(result.encoded_len());
        result.encode(&mut buf).ok();
        unsafe { OUTPUT_BUF = buf; }

        NodeOutput { msgs: vec![] }
    }

    fn process_raw(_: u32, _: u32, _: Vec<u8>) -> NodeOutput { NodeOutput { msgs: vec![] } }
    fn save_state() -> Vec<u8>     { vec![] }
    fn load_state(_state: Vec<u8>) {}
}

#[no_mangle]
pub extern "C" fn take_output_ptr() -> u32 {
    unsafe { (&raw const OUTPUT_BUF).as_ref().unwrap().as_ptr() as u32 }
}
#[no_mangle]
pub extern "C" fn take_output_len() -> u32 {
    unsafe { (&raw const OUTPUT_BUF).as_ref().unwrap().len() as u32 }
}
#[no_mangle]
pub extern "C" fn clear_output() {
    unsafe { (&raw mut OUTPUT_BUF).as_mut().unwrap().clear(); }
}

export!(SinkNode);
