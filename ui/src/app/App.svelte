<script lang="ts">
  import { QueryClient } from "@tanstack/svelte-query";
  import { QueryClientProvider } from "@tanstack/svelte-query";
  import AppShell from "./AppShell.svelte";

  const queryClient = new QueryClient();
</script>

<QueryClientProvider client={queryClient}>
  <AppShell />
</QueryClientProvider>
