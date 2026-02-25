// nodes/source-node/src/lib.rs
// Source Node：Protobuf decode → FlowMsg
// world: flow-node（不需要 host-api）

wit_bindgen::generate!({
    world: "flow-node",
    path: "../../wit",
});

use exports::iiot::flow::meta::Guest as MetaGuest;
use exports::iiot::flow::node::Guest as NodeGuest;
use iiot::flow::types::{FlowMsg, NodeOutput, TagValue, ValueKind};

mod proto {
    #[derive(prost::Message, Clone)]
    pub struct TagUpdate {
        #[prost(string,  tag = "1")] pub tag_id_str: String,
        #[prost(uint64,  tag = "2")] pub timestamp:  u64,
        #[prost(uint32,  tag = "3")] pub quality:    u32,
        #[prost(string,  tag = "4")] pub unit:       String,
        #[prost(bool,    optional, tag = "10")] pub bool_val: Option<bool>,
        #[prost(int32,   optional, tag = "11")] pub i32_val:  Option<i32>,
        #[prost(uint32,  optional, tag = "12")] pub u32_val:  Option<u32>,
        #[prost(float,   optional, tag = "13")] pub f32_val:  Option<f32>,
        #[prost(double,  optional, tag = "14")] pub f64_val:  Option<f64>,
        #[prost(string,  optional, tag = "15")] pub str_val:  Option<String>,
        #[prost(bytes = "vec", optional, tag = "16")] pub blob_val: Option<Vec<u8>>,
    }
}

struct SourceNode;

impl MetaGuest for SourceNode {
    fn accepted_input_types() -> Vec<ValueKind> { vec![] }
    fn output_type() -> ValueKind { ValueKind::Any }
    fn name()    -> String { "source-node:protobuf-tag-update".to_string() }
    fn version() -> String { "0.1.0".to_string() }
}

impl NodeGuest for SourceNode {
    fn process(_msg: FlowMsg) -> NodeOutput {
        NodeOutput { msgs: vec![] }
    }

    fn process_raw(tag_id: u32, msg_id: u32, raw_bytes: Vec<u8>) -> NodeOutput {
        use prost::Message;
        let tu = match proto::TagUpdate::decode(raw_bytes.as_slice()) {
            Ok(v)  => v,
            Err(_) => return NodeOutput { msgs: vec![] },
        };

        let value = if let Some(v) = tu.bool_val  { TagValue::BoolVal(v)   }
        else if let Some(v) = tu.i32_val           { TagValue::I32Val(v)    }
        else if let Some(v) = tu.u32_val           { TagValue::U32Val(v)    }
        else if let Some(v) = tu.f32_val           { TagValue::F32Val(v)    }
        else if let Some(v) = tu.f64_val           { TagValue::F64Val(v)    }
        else if let Some(v) = tu.str_val           { TagValue::ShortStr(v)  }
        else if let Some(v) = tu.blob_val          { TagValue::Blob(v)      }
        else { return NodeOutput { msgs: vec![] }; };

        NodeOutput { msgs: vec![FlowMsg {
            tag_id, msg_id, value,
            timestamp: tu.timestamp,
            quality:   tu.quality as u8,
        }]}
    }

    fn save_state() -> Vec<u8>     { vec![] }
    fn load_state(_state: Vec<u8>) {}
}

export!(SourceNode);
