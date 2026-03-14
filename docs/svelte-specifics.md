# Svelte 5 + SvelteFlow — Project-Specific Notes

Collected from current project debugging, library docs, and implementation notes for
the Svelte 5 frontend.

---

## 1. SvelteFlow (@xyflow/svelte) Critical Rules

### Use `$state.raw` for nodes and edges — NEVER `$state` or `$derived`

```ts
// CORRECT
let nodes = $state.raw<Node[]>([...]);
let edges = $state.raw<Edge[]>([...]);

// WRONG — deep Proxy breaks SvelteFlow internals
let nodes = $state<Node[]>([...]);
let nodes = $derived<Node[]>([...]);
```

**Why:** SvelteFlow mutates internal properties on node objects (`node.measured.width`,
`node.measured.height`, position tracking). Plain `$state()` wraps arrays in Svelte 5's
deep `Proxy`, which intercepts these mutations and traps them in the reactivity graph
instead of modifying the actual objects. Result: nodes have zero dimensions and are
invisible. `$derived` has the same proxy issue AND is read-only.

### `$state.raw` only fixes the array itself — nested fields must still be plain

This caused a real bug in `GraphEditor.svelte`: the app state showed `1 nodes`, but
SvelteFlow rendered zero `.svelte-flow__node` elements and `onnodeclick` never fired.

**Important:** a `$state.raw<Node[]>` array is only safe if each node object and its nested
object fields are also plain data. If a mapper copies nested values directly from a deep
`$state` source, those nested values can still be Svelte proxies.

```ts
// SAFE: rebuild nested objects as plain values
const canvasPosition = workflow.ui?.canvas?.nodes?.[node.id];

return {
  id: node.id,
  position: canvasPosition
    ? { x: canvasPosition.x, y: canvasPosition.y }
    : { x: 180, y: 180 },
  data: { label: node.name },
  type: "default",
};
```

```ts
// UNSAFE: nested proxy can leak through from deep $state
return {
  id: node.id,
  position: workflow.ui?.canvas?.nodes?.[node.id],
  data: node.data,
};
```

Why this matters: `@xyflow/svelte` stores the original `userNode` internally and later
shallow-copies it when updating dimensions/position. If `position`, `data`, `measured`,
or any other nested object is still proxied, that proxy survives even though the outer
array is `$state.raw`.

**Practical rule:** whenever nodes/edges come from a rune-backed store, rebuild every
nested object/array field that you pass into SvelteFlow. Common examples are `position`,
`data`, and edge marker objects such as `markerEnd`.

**Quick smoke test:** `structuredClone(nodes[0])` should succeed. In development,
SvelteFlow's own `$state.raw` warning uses this same check on the first node/edge.

**References:**
- https://github.com/xyflow/xyflow/issues/5200
- https://github.com/sveltejs/svelte/issues/13915
- https://svelteflow.dev/learn/getting-started/building-a-flow

### Always use `bind:nodes` and `bind:edges`

```svelte
<SvelteFlow bind:nodes bind:edges fitView>
```

Passing `{nodes}` as a read-only prop prevents SvelteFlow from updating internal state.
The `$bindable()` pattern in SvelteFlow's source requires two-way binding.

### Immutable update pattern with `$state.raw`

Since `$state.raw` is non-reactive for mutations, you must reassign the whole array:

```ts
// CORRECT — triggers reactivity
nodes = [...nodes, newNode];
nodes = nodes.map(n => n.id === id ? { ...n, data: newData } : n);

// WRONG — silent no-op
nodes.push(newNode);
nodes[0].data.label = "new";
```

### Syncing external data -> SvelteFlow nodes

Use `$effect` to rebuild and reassign when source data changes:

```ts
let nodes = $state.raw<Node[]>([]);

$effect(() => {
  void workflow.nodes.length;   // dependency tracking
  void workflow.entryNodeId;
  nodes = workflow.nodes.map((node) => {
    const canvasPosition = workflow.ui?.canvas?.nodes?.[node.id];

    return {
      id: node.id,
      position: canvasPosition
        ? { x: canvasPosition.x, y: canvasPosition.y }
        : { x: 180, y: 180 },
      data: { label: node.name },
      type: "default",
    };
  });
});
```

If the mapper depends on reactive props or store values, prefer initializing `nodes`/`edges`
as empty `$state.raw([])` arrays and filling them inside `$effect`. Calling the mapper
inside the `$state.raw(...)` initializer captures only the initial reactive values and
triggers Svelte's `state_referenced_locally` warning.

### Event handler signatures (v1.x / Svelte 5)

SvelteFlow events pass a **single object**, not positional args like React Flow:

```ts
// SvelteFlow (correct)
onnodeclick={({ node }) => selectNode(node.id)}
onedgeclick={({ edge }) => selectEdge(edge.id)}
onnodedragstop={({ targetNode }) => ...}
onpaneclick={() => ...}

// React Flow style (WRONG in SvelteFlow)
onnodeclick={(_event, node) => ...}  // won't work
```

### Do not set `nodeDragThreshold={0}` in this project

We tried this as a workaround for click-vs-drag behavior and it caused a real regression:
task nodes required extra clicks to select, while our custom inspector selection got out of
sync with SvelteFlow's internal selection.

In the current `@xyflow/svelte` build, node click selection in `NodeWrapper.svelte` is gated by
`store.nodeDragThreshold > 0` when `selectNodesOnDrag` is enabled. Setting the threshold to `0`
therefore changes more than drag sensitivity; it also changes the library's click-selection path.

**Project rule:** leave `nodeDragThreshold` at the library default unless there is a verified fix
for the exact interaction issue being addressed, and retest single-click node selection after
any threshold change.

### Node IDs must be strings

Using numeric IDs causes nodes to be in the DOM but invisible. No warning is shown.
Always use `crypto.randomUUID()` or string-typed IDs.

Reference: https://github.com/xyflow/xyflow/issues/5024

### `useSvelteFlow()` must be called inside SvelteFlow context

The `useSvelteFlow()` hook reads from Svelte context and can only be used in components
that are children of `<SvelteFlow>`. It cannot be called in the same component that
renders `<SvelteFlow>`.

### `applyNodeChanges` / `applyEdgeChanges` don't exist

These are React Flow helpers. In SvelteFlow, the internal store handles changes
automatically when using `bind:nodes`. If you need to intercept changes, use
`onnodeschange`/`onedgeschange` for notification only.

---

## 2. Svelte 5 Runes Patterns

### `$state` vs `$state.raw`

| Feature | `$state` | `$state.raw` |
|---------|----------|--------------|
| Deep reactivity | Yes (Proxy-wrapped) | No (reference only) |
| Mutation tracking | Automatic (property-level) | Must reassign whole value |
| Use for | Simple objects, form state | Arrays of complex objects, lib interop |
| SvelteFlow compat | NO | YES, if nested members are plain |

### `@const` must be inside control flow blocks

```svelte
<!-- WRONG — @const directly in <section> or <div> -->
<section>
  {@const value = compute()}
</section>

<!-- CORRECT — @const inside {#if}, {#each}, {#snippet} -->
{#if condition}
  {@const value = compute()}
{/if}
```

If you need computed values outside control flow, use functions or `$derived`.

### `$effect` — no dependency arrays

Unlike React's `useEffect`, Svelte 5's `$effect` auto-tracks dependencies by reading
reactive values inside the callback. No dependency array means no stale-closure bugs
and no infinite-loop-from-unstable-references.

```ts
// Use void reads to explicitly declare dependencies
$effect(() => {
  void someReactiveValue;  // tracked dependency
  doSomething();
});
```

### Render and `$derived` paths must be read-only

This caused the task-node inspector bug in `InspectorPanel.svelte`. A helper called from markup
was lazily initializing local `$state`:

```ts
// WRONG: called from template, mutates $state during render
function getJsonDraft(key: string, value: unknown): string {
  if (jsonDrafts[key] === undefined) {
    jsonDrafts[key] = value ? JSON.stringify(value, null, 2) : "";
  }
  return jsonDrafts[key] ?? "";
}
```

That triggered Svelte's `state_unsafe_mutation` error when selecting task nodes, because those
task-only fields executed the helper during render. Approval nodes did not hit that branch, which
made the bug look node-type-specific.

**Safe pattern:**

```ts
// CORRECT: pure read in template
function formatJsonDraft(key: string, value: unknown): string {
  return jsonDrafts[key] ?? (value ? JSON.stringify(value, null, 2) : "");
}

// write only in effects or event handlers
$effect(() => {
  const _sel = store.selection;
  jsonDrafts = {};
  jsonErrors = {};
});
```

**Project rule:** functions called from template expressions, `@const`, and `$derived.by(...)`
must be pure reads. Initialize or reset local rune state in `$effect`, and mutate it only in
event handlers or other explicit imperative code.

### Class-based stores with `$state`

```ts
class MyStore {
  items = $state<Item[]>([]);           // deep reactive
  count = $derived(this.items.length);  // computed

  add(item: Item) {
    this.items.push(item);  // works because $state tracks mutations
  }
}
export const store = new MyStore();
```

For store properties consumed by SvelteFlow, use `$state.raw` and reassign.

---

## 3. Svelte 5 Component Patterns

### Props syntax

```svelte
<script lang="ts">
  let { workflow, validation = null }: {
    workflow: WorkflowDocument;
    validation: ValidationResponse | null;
  } = $props();
</script>
```

### Event forwarding

Svelte 5 uses `on<event>` props (no colon syntax):

```svelte
<button onclick={() => doThing()}>Click</button>
<input oninput={(e) => handle(e)} />
<svelte:window onkeydown={handleKey} />
```

### QueryClient context (tanstack/svelte-query)

`createQuery`/`createMutation` must be called inside a component wrapped by
`<QueryClientProvider>`. Split into wrapper (App.svelte) and consumer (AppShell.svelte).

---

## 4. Vite / Build Notes

### Svelte plugin config

```js
// ui/vite.config.ts
import { svelte } from "@sveltejs/vite-plugin-svelte";
export default defineConfig({ plugins: [svelte()] });

// ui/svelte.config.js
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";
export default { preprocess: vitePreprocess() };
```

### JSON in Svelte attribute values

Svelte parser treats `{` as expression start inside attributes:

```svelte
<!-- WRONG -->
<textarea placeholder='{"key":"value"}'></textarea>

<!-- CORRECT -->
<textarea placeholder={'{"key":"value"}'}></textarea>
```

### Stale Vite cache

If HMR shows stale errors after fixing imports, clear `node_modules/.vite/` and restart.

---

## 5. Known @xyflow/svelte Notes (current project: 1.1.0)

| Issue | Description | Workaround |
|-------|-------------|------------|
| [#4996](https://github.com/xyflow/xyflow/issues/4996) | `nodeDragThreshold` and click handling interact in surprising ways | Do not use `nodeDragThreshold={0}` in this project; keep the default and retest single-click selection after any change |
| [#5024](https://github.com/xyflow/xyflow/issues/5024) | Numeric node IDs cause invisible nodes | Always use string IDs |
| [#5223](https://github.com/xyflow/xyflow/issues/5223) | Nodes disappear on browser tab switch | Known in v11.10.x |
| [#4120](https://github.com/xyflow/xyflow/issues/4120) | Dynamic handles in `{#each}` don't update | Wrap with `{#key}` block |
