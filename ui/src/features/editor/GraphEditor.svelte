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
  import { store } from "@/lib/stores/workflowStore.svelte";
  import type {
    RuntimeCapabilities,
    ValidationResponse,
    WorkflowDocument,
    WorkflowEdgeOutcome,
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
    return workflow.edges.map((edge) => ({
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
    // Reading workflow.nodes triggers this effect on any mutation
    void workflow.nodes.length;
    void workflow.entryNodeId;
    void validation;
    void store.nodeStates;
    void store.selection;
    nodes = mergeFlowNodes(
      lastNodes,
      buildFlowNodes(
        workflow,
        validation,
        validationIndex,
        store.nodeStates,
        store.selection.kind === "node" ? store.selection.id : null,
      ),
    );
  });

  $effect(() => {
    void workflow.edges.length;
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
    const outgoing = workflow.edges.filter((e) => e.from === connection.source);
    const sourceNode = workflow.nodes.find((node) => node.id === connection.source);
    let outcome: WorkflowEdgeOutcome = "success";

    if (sourceNode?.type === "collector" && outgoing.some((edge) => edge.outcome === "success")) {
      store.setError("Collector nodes can only have one success edge.");
      return;
    }

    if (sourceNode?.type === "split" || sourceNode?.type === "collector") {
      outcome = "success";
    } else if (sourceNode?.type === "task") {
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

  const supportedNodeTypes = $derived(
    capabilities?.supportedNodeTypes ?? ["task", "approval", "split", "collector"],
  );
</script>

<div class="graphEditor">
  <SvelteFlow
    bind:nodes
    bind:edges
    fitView
    onnodedragstop={handleNodeDragStop}
    onpaneclick={handlePaneClick}
    onnodeclick={handleNodeClick}
    onedgeclick={handleEdgeClick}
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
        <span class="canvasToolbar__meta">{workflow.nodes.length} nodes</span>
      </div>
    </Panel>
  </SvelteFlow>
</div>
