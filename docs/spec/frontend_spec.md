# FusionFlow Frontend Specification

## Overview

The frontend is a web-based GUI built with Svelte, providing a visual editor for DAG creation, similar to Node-RED. It supports drag-and-drop Node addition, Edge connections, dynamic rendering of handles and styles, and real-time updates from the backend via WebSocket. All Nodes use a single customizable component for rendering, with Protobuf for data exchange.

## Key Features

- DAG Visualization: Interactive canvas for Nodes and Edges using @xyflow/svelte.
- Node Editing: Dynamic adjustment of handles, styles, and configurations via dialogs.
- Drag-and-Drop: From sidebar to canvas for adding Nodes.
- Right-Click Deletion: Users can right-click (mouse context menu) on a Node or Handle to select and delete it. This triggers a context menu with option like "Delete", updating the stores ($nodes, $edges) accordingly.
- Real-Time Updates: WebSocket for backend-driven changes (e.g., new handles or styles).
- Export/Import: Serialize DAG to Protobuf and send to backend; load from Protobuf.
- UI Components: shadcn-svelte for buttons, dialogs, accordions; Tailwind CSS for styling.

## Technology StackFramework:

- Svelte 5.x (with SvelteKit for routing if needed).
- Libraries:
    - @xyflow/svelte 1.5.1: For flow canvas, Nodes, Edges.
    - Protobuf: Static code generation from .proto using protoc or Buf (e.g., protoc-gen-es for TypeScript/JavaScript code). No runtime dependency like protobufjs; directly import generated classes for encode/decode.
    - shadcn-svelte: UI components (Button, Dialog, Accordion, Input, Textarea).
    - Tailwind CSS 4.1.x: Styling with dark mode support.
    - Advanced (Optional): Auto-layout button for DAG arrangement (using @dagrejs/dagre), implemented in future iterations to allow users to trigger hierarchical positioning without overriding manual adjustments.
- Build Tool: Vite 7.x.
- Package Manager: pnpm 10.x.
- Communication: HTTP POST for initial DAG submission (Protobuf binary); WebSocket for updates.

## Components Structure

- App.svelte: Main layout with sidebar (Node library) and canvas (SvelteFlow). Handles WebSocket, drag-and-drop, layout application, and Protobuf export.
- CustomNode.svelte: Single Node renderer. Dynamically applies handles (with offsets) and styles from `data.style` (e.g., background-color, border-radius). Uses <Handle> for connections.
- Sidebar: Accordion-based Node selector with draggable items (basic/custom Nodes). Includes search input and tools (layout buttons, export).
- Edit Dialog: shadcn Dialog for Node editing (label, WASM path, width/height, config/handles/style as JSON inputs).
- Protobuf Integration: Use protoc or Buf to generate static JS/TS code from .proto (e.g., `import { Dag } from './proto/fusion_pb.js';`). Encode/decode using generated classes (e.g., `Dag.encode(message).finish()` for binary).

## Data Flow

- Canvas starts with empty nodes/edges stores.
- User drags Node from sidebar → Adds to $nodes store at drop position → Renders on canvas.
- Connect Edges → Updates $edges.
- Edit Node → Opens dialog, updates data (handles/style).
- Export → Builds Protobuf Dag from stores using generated classes, sends binary to backend.
- Backend Update → WebSocket receives NodeUpdate binary, decodes with generated classes, updates specific Node's data (re-renders handles/style dynamically).
- Advanced Layout (Future): User clicks "Auto Layout" button → Triggers dagre to compute and apply positions without overriding manual drags.


## Performance & Accessibility

- Optimize for large DAGs: Use fitView for zooming; lazy-load Protobuf generated code.
- Accessibility: ARIA labels on Nodes/Edges; keyboard navigation via @xyflow.
- Dark Mode: Tailwind classes (e.g., dark:bg-gray-800).
- Error Handling: Validate Protobuf with generated verify methods before send; toast notifications for failures.

## DeploymentBuild:
- Build: `pnpm build` → Static assets for hosting. Include Protobuf generation in build script (e.g., via Vite plugin or pre-build hook).
- Testing: Vitest for unit tests; Playwright for E2E (focus on drag-drop, WebSocket).

Example
```typescript
<script lang="ts">
  import { writable } from 'svelte/store';
  import {
    SvelteFlow,
    Background,
    Controls,
    MiniMap,
    Panel,
    Position,
    ConnectionLineType,
    DnDProvider,
    useDrag,
    type Node,
    type Edge
  } from '@xyflow/svelte';
  import '@xyflow/svelte/dist/style.css';
  import CustomNode from './CustomNode.svelte';
  import { Button } from '$lib/components/ui/button';  // shadcn
  import { Accordion, AccordionContent, AccordionItem, AccordionTrigger } from '$lib/components/ui/accordion';
  import { Input } from '$lib/components/ui/input';
  import { Dialog, DialogContent, DialogHeader, DialogTitle } from '$lib/components/ui/dialog';
  import { Label } from '$lib/components/ui/label';
  import { Textarea } from '$lib/components/ui/textarea';
  import { Dag } from './proto/fusion_pb';  // 假設靜態生成 Protobuf code

  // 初始為空，讓畫布從空白開始
  let nodes = writable<Node[]>([]);
  let edges = writable<Edge[]>([]);
  let nodeCounts: Record<string, number> = {};

  let showEditDialog = false;
  let selectedNode: Node | null = null;

  let ws: WebSocket;  // WebSocket 監聽後端更新
  // 假設靜態 Protobuf code 已生成，無需 protobufjs load

  onMount(() => {
    // WebSocket 連接
    ws = new WebSocket('ws://your-backend-url/ws');
    ws.onmessage = (event) => {
      const buffer = new Uint8Array(event.data);
      const update = NodeUpdate.decode(buffer);  // 使用生成 code
      nodes.update(n => n.map(node => {
        if (node.id === update.node_id) {
          node.data = update.updated_data;
        }
        return node;
      }));
    };
  });

  // 拖拽 createDraggableNode（如前）

  // 當拖拽掉落到畫布時添加 Node
  function onDrop(event) {
    const type = event.detail.dataTransfer.getData('application/svelteflow');
    if (!type) return;

    const position = event.detail.getXY();  // 掉落位置
    if (!nodeCounts[type]) nodeCounts[type] = 0;
    nodeCounts[type] += 1;
    const label = `${type}_${nodeCounts[type]}`;
    const id = crypto.randomUUID();
    const defaultHandles = getDefaultHandles(type);

    nodes.update(n => [...n, {
      id,
      type: 'custom',
      data: { label, wasm: `${type}.wasm`, config: {}, handles: defaultHandles },
      position,
      width: 200,
      height: 100
    }]);
  }

  // getDefaultHandles（如前）

  // 匯出 Protobuf
  async function exportProtobuf() {
    const dagData = {
      // ... 從 stores 建構
    };
    const message = Dag.create(dagData);  // 使用生成 code
    const buffer = Dag.encode(message).finish();

    await fetch('/api/dag', {
      method: 'POST',
      body: buffer,
      headers: { 'Content-Type': 'application/octet-stream' }
    });
  }

  // Node 點擊與保存（如前）

  function onConnect(event) {
    edges.update(e => [...e, event.detail]);
  }
</script>

<div class="flex h-screen">
  <!-- 側邊欄：Node 選擇，支援拖拽 -->
  <aside class="w-80 p-4 bg-gray-50 border-r overflow-y-auto dark:bg-gray-800">
    <h2 class="text-xl mb-4 font-bold text-gray-800 dark:text-gray-200">Node Library</h2>

    <!-- 搜尋 -->
    <Input placeholder="Search nodes..." class="mb-4" />

    <!-- Accordion -->
    <Accordion>
      <AccordionItem value="basic">
        <AccordionTrigger>Basic Nodes</AccordionTrigger>
        <AccordionContent>
          <div class="draggable-node p-2 bg-blue-100 rounded cursor-grab mb-2 hover:bg-blue-200" {...createDraggableNode('data_load')}>
            Data Load Node
          </div>
          <div class="draggable-node p-2 bg-blue-100 rounded cursor-grab mb-2 hover:bg-blue-200" {...createDraggableNode('process')}>
            Process Node
          </div>
          <div class="draggable-node p-2 bg-blue-100 rounded cursor-grab hover:bg-blue-200" {...createDraggableNode('output')}>
            Output Node
          </div>
        </AccordionContent>
      </AccordionItem>

      <AccordionItem value="custom">
        <AccordionTrigger>Custom Nodes</AccordionTrigger>
        <AccordionContent>
          <div class="draggable-node p-2 bg-green-100 rounded cursor-grab mb-2 hover:bg-green-200" {...createDraggableNode('custom')}>
            Custom Node
          </div>
          <Input type="file" accept=".wasm" class="mt-2" on:change={(e) => console.log('Upload:', e.target.files[0])} />
        </AccordionContent>
      </AccordionItem>
    </Accordion>

    <!-- 工具（移除 layout 按鈕，作為進階） -->
    <div class="mt-6 space-y-2">
      <Button class="w-full" on:click={exportProtobuf}>Export Protobuf</Button>
    </div>
  </aside>

  <!-- 畫布：支援掉落 -->
  <main class="flex-1">
    <DnDProvider>
      <SvelteFlow
        bind:nodes={$nodes}
        bind:edges={$edges}
        fitView
        on:nodeclick={onNodeClick}
        on:connect={onConnect}
        on:paneDrop={onDrop}
        connectionLineType={ConnectionLineType.SmoothStep}
        defaultEdgeOptions={{ type: 'smoothstep', animated: true }}
      >
        <Background variant="dots" />
        <Controls />
        <MiniMap />
        <slot:node types={{ custom: CustomNode }} />
      </SvelteFlow>
    </DnDProvider>
  </main>
</div>

<!-- Dialog 編輯（如前） -->
<Dialog bind:open={showEditDialog}>
  <!-- 同前內容 -->
</Dialog>
```



