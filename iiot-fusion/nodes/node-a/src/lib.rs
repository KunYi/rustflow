// nodes/node-a/src/lib.rs
// Node A：單位換算 °C → °F，只接受 f32/f64

wit_bindgen::generate!({
    world: "flow-node",
    path: "../../wit",
});

use exports::iiot::flow::meta::Guest as MetaGuest;
use exports::iiot::flow::node::Guest as NodeGuest;
use iiot::flow::types::{FlowMsg, NodeOutput, TagValue, ValueKind};

struct NodeA;

impl MetaGuest for NodeA {
    fn accepted_input_types() -> Vec<ValueKind> {
        vec![ValueKind::F32Val, ValueKind::F64Val]
    }
    fn output_type() -> ValueKind { ValueKind::F64Val }
    fn name()    -> String { "node-a:unit-converter".to_string() }
    fn version() -> String { "0.1.0".to_string() }
}

impl NodeGuest for NodeA {
    fn process(msg: FlowMsg) -> NodeOutput {
        let raw: f64 = match msg.value {
            TagValue::F32Val(v) => v as f64,
            TagValue::F64Val(v) => v,
            _ => return NodeOutput { msgs: vec![] },
        };
        // °C → °F
        let converted = raw * 9.0 / 5.0 + 32.0;
        NodeOutput { msgs: vec![FlowMsg {
            tag_id:    msg.tag_id,
            msg_id:    msg.msg_id,
            value:     TagValue::F64Val(converted),
            timestamp: msg.timestamp,
            quality:   msg.quality,
        }]}
    }

    fn process_raw(_: u32, _: u32, _: Vec<u8>) -> NodeOutput { NodeOutput { msgs: vec![] } }
    fn save_state() -> Vec<u8>     { vec![] }
    fn load_state(_state: Vec<u8>) {}
}

export!(NodeA);
