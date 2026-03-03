# Fusion Frame ABI - Port Data Access Specification

Version 1.0


## 1. Scope

本規格定義：

- WASM Component Node 內部
- 透過 execute(frame_ptr: u32) -> u32
- 如何存取 Port Data
- 如何讀寫 Primitive 與 Dynamic 型別
- 如何處理 Arena 分配

本規格不涉及：

- Graph runtime
- Scheduling
- Component linking
- DAG lowering

## 2. Memory Model

### 2.1 frame_ptr

frame_ptr 為：

- 指向 WebAssembly linear memory 內的 byte address
- 型別為 u32
- 必須 8-byte aligned

### 2.2 Frame Layout

```code
frame_ptr
│
├── FrameHeader (16 bytes)
├── PortSlot[PORT_COUNT]
└── ArenaRegion
```

## 3. FrameHeader

### 3.1 Layout (16 bytes)

| Offset | Size | Field          | Type |
| ------ | ---- | -------------- | ---- |
| 0      | 4    | magic          | u32  |
| 4      | 4    | version        | u32  |
| 8      | 4    | arena_offset   | u32  |
| 12     | 4    | arena_capacity | u32  |

### 3.2 Constraints

- magic MUST equal 0x4652414D ("FRAM")
- arena_offset MUST be 8-byte aligned
- arena_offset >= 16 + PORT_COUNT * 16

## 4. PortSlot Definition

### 4.1 Layout (16 bytes fixed)

| Offset | Size | Field    | Type |
| ------ | ---- | -------- | ---- |
| 0      | 4    | type_tag | u32  |
| 4      | 4    | reserved | u32  |
| 8      | 8    | payload  | u64  |

### 4.2 PortSlot Address Formula

```code
slot_address(i) = frame_ptr + 16 + i * 16
```

Where:

    i ∈ [0, PORT_COUNT)

## 5. TypeTag Specification

| Value | Meaning |
| ----- | ------- |
| 1     | I32     |
| 2     | I64     |
| 3     | F64     |
| 4     | BOOL    |
| 5     | SLICE   |

## 6. Payload Encoding

### 6.1 I32

```code
payload lower 32 bits = value
upper 32 bits MUST be zero
```

### 6.2 I64

```code
payload = full 64-bit integer
```

### 6.3 F64

```code
payload = IEEE 754 binary representation
```

### 6.4 BOOL

```code
payload lower 32 bits:
    0 = false
    1 = true
upper 32 bits MUST be zero
```

### 6.5 SLICE (String / Blob)

#### 6.5.1 Payload Layout

```code
low  32 bits = ptr (relative to frame_ptr)
high 32 bits = len
```

### 6.5.2 Constraints

- ptr MUST be < arena_capacity
- ptr + len MUST NOT exceed arena_capacity
- ptr MUST be 4-byte aligned
- Data MUST reside inside ArenaRegion

## 7. ArenaRegion

## 7.1 Definition

```code
arena_base = frame_ptr + header.arena_offset
arena_size = header.arena_capacity
```

Arena 是單次 execute() 生命週期的 bump allocator 區域。

## 7.2 Allocation Rules

Node MAY allocate from arena.

Node MUST:

- Not write outside arena bounds
- Not reuse memory after allocation

Node MUST NOT:

- Free memory
- Modify header fields
- Modify arena_offset or capacity

## 7.3 Allocation Model

Recommended bump pointer:

```code
cursor = 0

alloc(size, align):
    aligned = align_up(cursor, align)
    if aligned + size > arena_capacity:
        trap or return error
    cursor = aligned + size
    return aligned
```

Returned offset MUST be relative to frame_ptr.

## 8. Port Access Rules

### 8.1 Reading a Port

Node MUST:
1. Compute slot address
1. Read type_tag
1. Validate expected type
1. Decode payload according to type

If type mismatch:
- MUST trap OR
- MUST return error code (implementation defined)

### 8.2 Writing a Port

Node MUST:

1. Fully overwrite:

   - type_tag
   - payload

2. Reserved field MAY be zeroed

Partial write is undefined behavior.

## 9. Execution Semantics

During a single `execute(frame_ptr)` call:

- Frame memory is stable
- Arena is writable
- Ports may be read multiple times
- Output ports MUST be fully written before return

## 10. Determinism Requirements

Node MUST NOT:

- Access memory outside frame
- Store absolute pointers
- Store wasm memory addresses
- Rely on previous execute state

All state MUST be inside frame.

## 11. Cross-Language Compliance Requirements

An implementation is compliant if:

1. It uses little-endian interpretation
1. It respects 16-byte slot size
1. It respects payload encoding rules
1. It does not assume struct padding
1. It performs bounds checking for SLICE

## 12. Error Handling

Implementation MAY:

- Trap (wasm unreachable)
- Return non-zero error code
- Write error code into reserved slot

Behavior MUST be consistent within the system.

## 13. Future Compatibility

The following are reserved for future versions:

- Additional TypeTag values
- Use of reserved field
- Extended header fields after byte 16

Nodes MUST ignore unknown reserved fields.

## 14. Security Considerations

Node MUST validate:

- SLICE bounds
- TypeTag correctness
- Arena overflow

Failure to validate MAY lead to memory corruption inside frame.

## 15. Example Access Procedure (Language Neutral)

### Read I32

```code
slot = slot_address(i)
assert slot.type_tag == I32
value = lower_32_bits(slot.payload)
```

### Write SLICE

```code
offset = alloc(len, 4)
copy data to arena_base + offset

slot.type_tag = SLICE
slot.payload = (len << 32) | offset
```

## 16. ABI Stability Guarantee

As long as:

- SLOT_SIZE = 16
- HEADER_SIZE = 16
- Payload encoding unchanged

Nodes compiled under this spec remain compatible.
