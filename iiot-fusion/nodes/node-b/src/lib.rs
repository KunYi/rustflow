// nodes/node-b/src/lib.rs
// Node B：品質過濾 + 閾值警報

wit_bindgen::generate!({
    world: "flow-node",
    path: "../../wit",
});

use exports::iiot::flow::meta::Guest as MetaGuest;
use exports::iiot::flow::node::Guest as NodeGuest;
use iiot::flow::types::{FlowMsg, NodeOutput, TagValue, ValueKind};

const HIGH_ALARM:  f64 = 104.0; // °F
const LOW_ALARM:   f64 =  32.0; // °F

struct NodeB;

impl MetaGuest for NodeB {
    fn accepted_input_types() -> Vec<ValueKind> { vec![ValueKind::F64Val] }
    fn output_type() -> ValueKind { ValueKind::F64Val }
    fn name()    -> String { "node-b:quality-filter".to_string() }
    fn version() -> String { "0.1.0".to_string() }
}

impl NodeGuest for NodeB {
    fn process(msg: FlowMsg) -> NodeOutput {
        // bad quality → 丟棄
        if msg.quality >= 2 { return NodeOutput { msgs: vec![] }; }

        let val = match msg.value {
            TagValue::F64Val(v) => v,
            _ => return NodeOutput { msgs: vec![] },
        };

        // 超出工程量程 → quality 降為 uncertain
        let quality = if val > HIGH_ALARM || val < LOW_ALARM { 1 } else { msg.quality };

        NodeOutput { msgs: vec![FlowMsg {
            tag_id:    msg.tag_id,
            msg_id:    msg.msg_id,
            value:     TagValue::F64Val(val),
            timestamp: msg.timestamp,
            quality,
        }]}
    }

    fn process_raw(_: u32, _: u32, _: Vec<u8>) -> NodeOutput { NodeOutput { msgs: vec![] } }
    fn save_state() -> Vec<u8>     { vec![] }
    fn load_state(_state: Vec<u8>) {}
}

export!(NodeB);
