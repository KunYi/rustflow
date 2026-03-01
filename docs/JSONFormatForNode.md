# Node JSON Format Design

This document defines the updated JSON format for representing a Node in the Fusion system. Each Node is a modular execution unit that can be composed into a Directed Acyclic Graph (DAG) for workflow orchestration. The format supports automatic ID generation (UUID for internal uniqueness), user-friendly labels, visual layout properties (position, width, height), and multiple connection points (handles/ports) with enhanced flexibility.

Key updates based on feedback:

- **Handles with Offset:** In addition to `position` (direction: top/bottom/left/right), each handle now includes an optional `offset` object for fine-grained positioning. This allows offsets in x/y coordinates (pixels or percentages) relative to the Node's edge, reducing design limitations and enabling more custom layouts (e.g., multiple handles on the same side without overlap).

- **Custom Node Support:** Nodes can be user-defined (type: "custom"). Users can specify their own WASM file path or URL in the Node edit mode. The JSON accommodates this by allowing flexible `config` and `handles` arrays, which can be edited in the GUI. In edit mode, users can upload/specify WASM, define custom handles (including offsets), and adjust other properties. This integrates with the GUI's edit dialog, where fields like WASM path, handles (as editable JSON or form), and offsets are configurable.

## JSON Structure Overview

- **Root Object:** Contains an array of `nodes` and an array of `edges` (for DAG composition, though this focuses on the Node itself).

- **Node Object:** Represents a single Node with properties for identification, configuration, visualization, and customizability.

- **Validation:** Use JSON Schema for validation (provided below).

- **Key Features:**
    - **ID:** Automatically generated UUID for uniqueness.

    - **Label:** Human-readable name (e.g., "data_load_1"),auto-generated or editable.

    - **Handles:** Array for multiple input/output ports, now with optional offset for precise positioning.

    - **Layout:** Includes position, width, and height for GUI rendering.

    - **Custom Nodes:** Type can be "custom"; wasm can be a user-specified path/URL; edit mode in GUI supports dynamic configuration.

## Example JSON (Single Node Focus)

For a complete DAG, wrap in a root with nodes and edges. Here's a standalone Node example with offsets and custom support:

```json
{
  "id": "uuid-1234-5678-9012-3456",  // Auto-generated UUID (internal unique ID)
  "type": "custom",  // Node type (e.g., 'data_load', 'process', or 'custom' for user-defined)
  "data": {
    "label": "custom_node_1",  // Display label (user-visible, auto-generated as type_increment or editable)
    "wasm": "user_uploaded.wasm",  // Path or URL to the WASM component file (user-specified in edit mode)
    "config": {  // Node-specific configuration (arbitrary object, editable in GUI)
      "param1": "value1",
      "param2": 42
    },
    "handles": [  // Array of connection points (ports), editable in GUI
      {
        "id": "input1",  // Port ID (unique within Node)
        "type": "target",  // 'source' (output) or 'target' (input)
        "position": "left",  // 'top', 'bottom', 'left', 'right'
        "offset": {  // Optional offset for precise positioning (relative to edge)
          "x": 0,  // Horizontal offset (pixels; positive right/down)
          "y": 20  // Vertical offset (pixels; positive down/right)
        }
      },
      {
        "id": "output_success",  // Example for branching
        "type": "source",
        "position": "right",
        "offset": { "x": 0, "y": -10 }  // Negative for up/left adjustment
      }
    ]
  },
  "position": {  // GUI position
    "x": 100,
    "y": 100
  },
  "width": 200,  // Node width in pixels (editable in GUI)
  "height": 150  // Node height in pixels (editable in GUI)
}
```

### Full DAG Example (Multiple Nodes with Custom and Offsets)

```json
{
  "nodes": [
    {
      "id": "uuid-1234-5678-9012-3456",
      "type": "data_load",
      "data": {
        "label": "data_load_1",
        "wasm": "load.wasm",
        "config": { "file": "input.csv" },
        "handles": [
          {
            "id": "output",
            "type": "source",
            "position": "right",
            "offset": { "x": 0, "y": 0 }  // Default zero offset
          }
        ]
      },
      "position": { "x": 100, "y": 100 },
      "width": 200,
      "height": 100
    },
    {
      "id": "uuid-abcd-efgh-ijkl-mnop",
      "type": "custom",
      "data": {
        "label": "custom_process_1",
        "wasm": "user_defined.wasm",  // User-specified in edit mode
        "config": { "transform": "filter", "custom_param": "user_value" },
        "handles": [
          {
            "id": "input",
            "type": "target",
            "position": "left",
            "offset": { "x": 0, "y": 30 }  // Offset to avoid overlap
          },
          {
            "id": "success",
            "type": "source",
            "position": "right",
            "offset": { "x": 0, "y": 0 }
          },
          {
            "id": "error",
            "type": "source",
            "position": "bottom",
            "offset": { "x": 50, "y": 0 }  // Centered offset
          }
        ]
      },
      "position": { "x": 300, "y": 100 },
      "width": 250,
      "height": 150
    }
  ],
  "edges": [
    {
      "id": "edge-1",  // Auto-generated edge ID
      "source": "uuid-1234-5678-9012-3456",
      "target": "uuid-abcd-efgh-ijkl-mnop",
      "sourceHandle": "output",
      "targetHandle": "input",
      "condition": "always"  // Optional condition for branching
    }
  ]
}
```

### JSON Schema for Validation

Use this schema to validate the JSON structure:

```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "type": "object",
  "properties": {
    "nodes": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "id": { "type": "string", "description": "Auto-generated UUID for internal uniqueness" },
          "type": { "type": "string", "description": "Node type (e.g., 'data_load' or 'custom')" },
          "data": {
            "type": "object",
            "properties": {
              "label": { "type": "string", "description": "User-visible label (e.g., 'custom_node_1')" },
              "wasm": { "type": "string", "description": "Path or URL to WASM component (user-editable for custom nodes)" },
              "config": { "type": "object", "description": "Arbitrary configuration object (editable in GUI)" },
              "handles": {
                "type": "array",
                "items": {
                  "type": "object",
                  "properties": {
                    "id": { "type": "string" },
                    "type": { "type": "string", "enum": ["source", "target"] },
                    "position": { "type": "string", "enum": ["top", "bottom", "left", "right"] },
                    "offset": {
                      "type": "object",
                      "properties": {
                        "x": { "type": "number", "description": "Horizontal offset (pixels)" },
                        "y": { "type": "number", "description": "Vertical offset (pixels)" }
                      },
                      "description": "Optional offset for precise port positioning"
                    }
                  },
                  "required": ["id", "type", "position"]
                }
              }
            },
            "required": ["label", "wasm", "handles"]
          },
          "position": {
            "type": "object",
            "properties": { "x": { "type": "number" }, "y": { "type": "number" } },
            "required": ["x", "y"]
          },
          "width": { "type": "number", "minimum": 50 },
          "height": { "type": "number", "minimum": 50 }
        },
        "required": ["id", "type", "data", "position", "width", "height"]
      }
    },
    "edges": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "id": { "type": "string" },
          "source": { "type": "string" },
          "target": { "type": "string" },
          "sourceHandle": { "type": "string" },
          "targetHandle": { "type": "string" },
          "condition": { "type": "string" }
        },
        "required": ["source", "target"]
      }
    }
  },
  "required": ["nodes", "edges"]
}
```

## Usage Notes

- **Generation Rules:**

   - **ID:** Use UUID (e.g., via `crypto.randomUUID()` in JavaScript) for collision-free uniqueness. Do not allow manual editing.

   - **Label:** Auto-generate as `${type}_${increment}` (track per-type counters). Users can edit in GUI for custom nodes.

   - **Handles:** Predefine defaults based on type, but fully editable in GUI edit mode (add/remove, set position/offset). Offsets default to {x: 0, y: 0}; support pixel-based for precision.

   - **Width/Height:** Defaults to 200x100; adjustable in GUI edit mode.

   - **Custom Node Edit Mode Integration:** In GUI (Svelte), when type is "custom":

       - Allow file upload or URL input for `wasm`.

       - Provide form fields or JSON editor for `handles` (including offset inputs as numbers).

       - Dynamic preview: Update Node rendering in real-time as handles/offsets change.

- **Integration:**

    - Frontend (Svelte GUI): In edit dialog, add inputs for offset (e.g., number fields for x/y per handle). Use `@xyflow/svelte` to position handles based on offsets.

    - Backend (Rust Fusion): Parse offsets if needed for wrapper logic (though primarily for GUI; backend focuses on logical connections via handle IDs).

- **Extensions:** For advanced custom nodes, add optional `style` (CSS overrides) or `validation_rules` in config for GUI-side checks.

This updated format addresses limitations by adding offsets for flexible handle positioning and fully supports user-defined custom nodes with editable WASM and properties.

