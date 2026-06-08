<script lang="ts">
  import {
    SvelteFlow,
    Background,
    Controls,
    MiniMap,
    Panel,
    MarkerType,
    type Node,
    type Edge,
    type Connection,
  } from "@xyflow/svelte";
  import clsx from "clsx";
  import { buildFlowNodes, buildValidationIndex } from "@/features/editor/flowNodes";
  import SubflowNode from "@/features/editor/SubflowNode.svelte";
  import { store } from "@/lib/stores/workflowStore.svelte";
  import type {
    RuntimeCapabilities,
    ValidationResponse,
    WorkflowDocument,
    WorkflowEdgeOutcome,
    WorkflowNodeType,
  } from "@/lib/types/workflow";

  let {
    workflow,
    validation,
    capabilities,
  }: {
    workflow: WorkflowDocument;
    validation: ValidationResponse | null;
    capabilities: RuntimeCapabilities | undefined;
  } = $props();

  // The canvas always renders the *active* document — root, or a subflow we
  // drilled into. `workflow` (the prop) stays bound to the root for saving.
  let activeWorkflow = $derived(store.activeWorkflow ?? workflow);

  const nodeTypes = { subflow: SubflowNode };

  // Secondary node types live behind a "+ More" menu to keep the toolbar tidy.
  let showMoreNodes = $state(false);

  const PRIMITIVE_TYPES: { type: WorkflowNodeType; label: string }[] = [
    { type: "run_agent", label: "Run Agent" },
    { type: "decide", label: "Decide" },
    { type: "parallel_batch", label: "Batch" },
    { type: "subflow", label: "Subflow" },
    { type: "call", label: "Call" },
    { type: "spawn", label: "Spawn" },
    { type: "send", label: "Send" },
    { type: "wait", label: "Wait" },
    { type: "capture", label: "Capture" },
    { type: "kill", label: "Kill" },
  ];

  function edgeColor(outcome: WorkflowEdgeOutcome): string {
    switch (outcome) {
      case "branch": return "#f97316";
      case "loop_continue":
      case "loop_exit": return "#14b8a6";
      case "reject": return "#ef4444";
      case "success":
      default: return "#64748b";
    }
  }

  function edgeDash(outcome: WorkflowEdgeOutcome): string {
    switch (outcome) {
      case "branch": return "8 4";
      case "loop_continue":
      case "loop_exit": return "3 3";
      case "reject": return "12 4 4 4";
      case "success":
      default: return "";
    }
  }

  let validationIndex = $derived(buildValidationIndex(validation));

  function toFlowEdges(): Edge[] {
    return activeWorkflow.edges.map((edge) => ({
      id: edge.id,
      source: edge.from,
      target: edge.to,
      selected: store.selection.kind === "edge" && store.selection.id === edge.id,
      label: edge.label ?? edge.outcome,
      markerEnd: { type: MarkerType.ArrowClosed, color: edgeColor(edge.outcome) },
      style: `stroke: ${edgeColor(edge.outcome)}; stroke-width: 2;${edgeDash(edge.outcome) ? ` stroke-dasharray: ${edgeDash(edge.outcome)};` : ""}`,
      labelStyle: `fill: ${edgeColor(edge.outcome)}; font-weight: 600;`,
      class: clsx("graphEdge", {
        "graphEdge--selected": store.selection.kind === "edge" && store.selection.id === edge.id,
      }),
    }));
  }

  // SvelteFlow REQUIRES $state.raw — plain $state wraps nodes in deep Proxy,
  // which breaks SvelteFlow's internal node.measured mutations (width/height).
  // See: https://github.com/xyflow/xyflow/issues/5200
  let nodes = $state.raw<Node[]>([]);
  let edges = $state.raw<Edge[]>([]);
  let lastNodes: Node[] = [];
  let lastEdges: Edge[] = [];

  function mergeFlowNodes(previousNodes: Node[], nextNodes: Node[]): Node[] {
    const previousById = new Map(previousNodes.map((node) => [node.id, node]));
    return nextNodes.map((node) => {
      const previous = previousById.get(node.id);
      return previous
        ? { ...previous, ...node, position: node.position, data: node.data, class: node.class, style: node.style }
        : node;
    });
  }

  function mergeFlowEdges(previousEdges: Edge[], nextEdges: Edge[]): Edge[] {
    const previousById = new Map(previousEdges.map((edge) => [edge.id, edge]));
    return nextEdges.map((edge) => {
      const previous = previousById.get(edge.id);
      return previous ? { ...previous, ...edge } : edge;
    });
  }

  $effect(() => {
    lastNodes = nodes;
  });

  $effect(() => {
    lastEdges = edges;
  });

  // Sync workflow store -> SvelteFlow when workflow data changes.
  // With $state.raw we must reassign the whole array (immutable pattern).
  $effect(() => {
    // Reading active nodes triggers this effect on any mutation
    void activeWorkflow.nodes.length;
    void activeWorkflow.entryNodeId;
    void store.drillStack;
    void validation;
    void store.nodeStates;
    void store.selection;
    nodes = mergeFlowNodes(
      lastNodes,
      buildFlowNodes(
        activeWorkflow,
        validation,
        validationIndex,
        store.nodeStates,
        store.selection.kind === "node" ? store.selection.id : null,
      ),
    );
  });

  $effect(() => {
    void activeWorkflow.edges.length;
    void store.drillStack;
    void store.selection;
    edges = mergeFlowEdges(lastEdges, toFlowEdges());
  });

  function handleNodeDragStop({ nodes }: { nodes: Node[] }) {
    for (const node of nodes) {
      store.setNodePosition(node.id, node.position);
    }
  }

  function handleConnect(connection: Connection) {
    if (!connection.source || !connection.target) return;
    const outgoing = activeWorkflow.edges.filter((e) => e.from === connection.source);
    const sourceNode = activeWorkflow.nodes.find((node) => node.id === connection.source);
    let outcome: WorkflowEdgeOutcome = "success";

    if (sourceNode?.type === "collector" && outgoing.some((edge) => edge.outcome === "success")) {
      store.setError("Collector nodes can only have one success edge.");
      return;
    }

    if (sourceNode?.type === "split" || sourceNode?.type === "collector") {
      outcome = "success";
    } else if (sourceNode?.type === "decide") {
      // Decide nodes route exclusively through branch edges.
      outcome = "branch";
    } else if (sourceNode?.type === "task" || sourceNode?.type === "run_agent") {
      outcome = outgoing.some((e) => e.outcome === "success") ? "branch" : "success";
    } else if (sourceNode?.type === "approval") {
      outcome = outgoing.some((e) => e.outcome === "success") ? "reject" : "success";
    }

    store.addEdge({
      from: connection.source,
      to: connection.target,
      outcome,
      label: outcome === "branch" ? "branch" : null,
      branchId: outcome === "branch" ? "branch" : null,
      condition: null,
    });
  }

  function handlePaneClick() {
    store.selectWorkflow();
  }

  function handleNodeClick({ node, event }: { node: Node; event: MouseEvent | TouchEvent }) {
    event.stopPropagation();
    store.selectNode(node.id);
  }

  function handleEdgeClick({ edge, event }: { edge: Edge; event: MouseEvent }) {
    event.stopPropagation();
    store.selectEdge(edge.id);
  }

  function handleSelectionChange({ nodes: selected }: { nodes: Node[]; edges: Edge[] }) {
    store.setMultiSelection(selected.map((n) => n.id));
  }

  function saveAsCompound() {
    const ids = store.multiSelectedNodeIds.length
      ? store.multiSelectedNodeIds
      : store.selection.kind === "node"
        ? [store.selection.id]
        : [];
    if (ids.length === 0) {
      store.setError("Select one or more nodes to save as a compound node.");
      return;
    }
    const name = prompt("Name for the new compound node (saved subflow):");
    if (!name) return;
    const result = store.saveSelectionAsCompound(ids, name);
    if (!result.ok) {
      store.setError(result.reason);
    }
  }

  const supportedNodeTypes = $derived(
    capabilities?.supportedNodeTypes ?? ["task", "approval", "split", "collector"],
  );

  let canSaveCompound = $derived(
    store.multiSelectedNodeIds.length > 0 || store.selection.kind === "node",
  );
</script>

<div class="graphEditor">
  <SvelteFlow
    bind:nodes
    bind:edges
    {nodeTypes}
    fitView
    onnodedragstop={handleNodeDragStop}
    onpaneclick={handlePaneClick}
    onnodeclick={handleNodeClick}
    onedgeclick={handleEdgeClick}
    onselectionchange={handleSelectionChange}
    onconnect={handleConnect}
  >
    <Background patternColor="rgba(148, 163, 184, 0.14)" gap={18} size={1} />
    <MiniMap
      pannable
      zoomable
      style="background-color: rgba(8, 15, 30, 0.88); border: 1px solid rgba(148, 163, 184, 0.16);"
    />
    <Controls />
    <Panel position="top-left">
      <div class="canvasToolbar">
        {#if supportedNodeTypes.includes("task")}
          <button class="button button--ghost" onclick={(event) => {
            event.stopPropagation();
            store.addNode("task");
          }}>
            + Task
          </button>
        {/if}
        {#if supportedNodeTypes.includes("approval")}
          <button class="button button--ghost" onclick={(event) => {
            event.stopPropagation();
            store.addNode("approval");
          }}>
            + Approval
          </button>
        {/if}
        {#if supportedNodeTypes.includes("split")}
          <button class="button button--ghost" onclick={(event) => {
            event.stopPropagation();
            store.addNode("split");
          }}>
            + Split
          </button>
        {/if}
        {#if supportedNodeTypes.includes("collector")}
          <button class="button button--ghost" onclick={(event) => {
            event.stopPropagation();
            store.addNode("collector");
          }}>
            + Collector
          </button>
        {/if}

        {#if PRIMITIVE_TYPES.some((t) => supportedNodeTypes.includes(t.type))}
          <div class="canvasToolbar__more">
            <button class="button button--ghost" onclick={(event) => {
              event.stopPropagation();
              showMoreNodes = !showMoreNodes;
            }}>
              + More ▾
            </button>
            {#if showMoreNodes}
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <div class="canvasToolbar__backdrop" onclick={() => showMoreNodes = false}></div>
              <div class="canvasToolbar__menu">
                {#each PRIMITIVE_TYPES.filter((t) => supportedNodeTypes.includes(t.type)) as item (item.type)}
                  <button class="canvasToolbar__menuItem" onclick={(event) => {
                    event.stopPropagation();
                    store.addNode(item.type);
                    showMoreNodes = false;
                  }}>
                    + {item.label}
                  </button>
                {/each}
              </div>
            {/if}
          </div>
        {/if}

        <button
          class="button button--ghost"
          disabled={!canSaveCompound}
          title="Collapse the selected nodes into a reusable compound node"
          onclick={(event) => { event.stopPropagation(); saveAsCompound(); }}
        >
          ⧉ Save as compound
        </button>

        <span class="canvasToolbar__meta">{activeWorkflow.nodes.length} nodes</span>
      </div>
    </Panel>

    {#if store.isDrilledIn}
      <Panel position="top-center">
        <div class="drillBreadcrumb">
          {#each store.breadcrumb as crumb, index (index)}
            {#if index > 0}<span class="drillBreadcrumb__sep">›</span>{/if}
            <button
              class="drillBreadcrumb__crumb"
              class:drillBreadcrumb__crumb--current={index === store.breadcrumb.length - 1}
              onclick={(event) => { event.stopPropagation(); store.drillToLevel(index); }}
            >
              {index === 0 ? (workflow.name || "root") : crumb}
            </button>
          {/each}
        </div>
      </Panel>
    {/if}
  </SvelteFlow>
</div>

<style>
  .canvasToolbar__more {
    position: relative;
  }

  .canvasToolbar__backdrop {
    position: fixed;
    inset: 0;
    z-index: 20;
  }

  .canvasToolbar__menu {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    z-index: 21;
    display: flex;
    flex-direction: column;
    min-width: 150px;
    padding: 6px;
    gap: 2px;
    background: rgba(8, 15, 30, 0.97);
    border: 1px solid var(--border);
    border-radius: 12px;
    box-shadow: 0 18px 40px rgba(2, 8, 23, 0.5);
  }

  .canvasToolbar__menuItem {
    text-align: left;
    padding: 7px 10px;
    border-radius: 8px;
    background: transparent;
    border: none;
    color: var(--text-bright);
    cursor: pointer;
    font-size: 13px;
  }

  .canvasToolbar__menuItem:hover {
    background: rgba(96, 165, 250, 0.16);
  }

  .drillBreadcrumb {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 14px;
    background: rgba(8, 15, 30, 0.9);
    border: 1px solid rgba(56, 189, 248, 0.4);
    border-radius: 999px;
    backdrop-filter: blur(10px);
    font-size: 13px;
  }

  .drillBreadcrumb__crumb {
    background: transparent;
    border: none;
    color: var(--text-dim);
    cursor: pointer;
    padding: 2px 4px;
    border-radius: 6px;
  }

  .drillBreadcrumb__crumb:hover {
    color: var(--text-bright);
  }

  .drillBreadcrumb__crumb--current {
    color: rgba(125, 211, 252, 0.95);
    font-weight: 600;
    cursor: default;
  }

  .drillBreadcrumb__sep {
    color: var(--text-dim);
    opacity: 0.6;
  }
</style>
