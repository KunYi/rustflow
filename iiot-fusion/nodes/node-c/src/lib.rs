// nodes/node-c/src/lib.rs
// Node C：滑動視窗平均，展示 save/load state

wit_bindgen::generate!({
    world: "flow-node",
    path: "../../wit",
});

use exports::iiot::flow::meta::Guest as MetaGuest;
use exports::iiot::flow::node::Guest as NodeGuest;
use iiot::flow::types::{FlowMsg, NodeOutput, TagValue, ValueKind};

const WINDOW: usize = 8;

static mut BUF:   [f64; WINDOW] = [0.0; WINDOW];
static mut POS:   usize = 0;
static mut COUNT: usize = 0;
static mut TOTAL: u64   = 0;

struct NodeC;

impl MetaGuest for NodeC {
    fn accepted_input_types() -> Vec<ValueKind> { vec![ValueKind::F64Val] }
    fn output_type() -> ValueKind { ValueKind::F64Val }
    fn name()    -> String { "node-c:sliding-avg".to_string() }
    fn version() -> String { "0.1.0".to_string() }
}

impl NodeGuest for NodeC {
    fn process(msg: FlowMsg) -> NodeOutput {
        let val = match msg.value {
            TagValue::F64Val(v) => v,
            _ => return NodeOutput { msgs: vec![] },
        };

        let avg = unsafe {
            BUF[POS] = val;
            POS   = (POS + 1) % WINDOW;
            if COUNT < WINDOW { COUNT += 1; }
            TOTAL += 1;
            BUF[..COUNT].iter().sum::<f64>() / COUNT as f64
        };

        NodeOutput { msgs: vec![FlowMsg {
            tag_id:    msg.tag_id,
            msg_id:    msg.msg_id,
            value:     TagValue::F64Val(avg),
            timestamp: msg.timestamp,
            quality:   msg.quality,
        }]}
    }

    fn process_raw(_: u32, _: u32, _: Vec<u8>) -> NodeOutput { NodeOutput { msgs: vec![] } }

    fn save_state() -> Vec<u8> {
        let mut b = Vec::with_capacity(WINDOW * 8 + 24);
        unsafe {
            let buf = &*std::ptr::addr_of!(BUF);
            for &v in buf { b.extend_from_slice(&v.to_le_bytes()); }
            b.extend_from_slice(&(POS   as u64).to_le_bytes());
            b.extend_from_slice(&(COUNT as u64).to_le_bytes());
            b.extend_from_slice(&TOTAL.to_le_bytes());
        }
        b
    }

    fn load_state(s: Vec<u8>) {
        if s.len() < WINDOW * 8 + 24 { return; }
        unsafe {
            for i in 0..WINDOW {
                BUF[i] = f64::from_le_bytes(s[i*8..i*8+8].try_into().unwrap());
            }
            let b = WINDOW * 8;
            POS   = u64::from_le_bytes(s[b   ..b+ 8].try_into().unwrap()) as usize;
            COUNT = u64::from_le_bytes(s[b+ 8..b+16].try_into().unwrap()) as usize;
            TOTAL = u64::from_le_bytes(s[b+16..b+24].try_into().unwrap());
        }
    }
}

export!(NodeC);
