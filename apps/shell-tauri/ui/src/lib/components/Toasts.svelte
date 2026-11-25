<script lang="ts">
  import { fly } from "svelte/transition";
  import { toasts } from "../store.svelte";
</script>

<div class="toast-wrap">
  {#each toasts.list as t (t.id)}
    <div class="toast {t.kind}" transition:fly={{ y: 14, duration: 240 }}>{t.msg}</div>
  {/each}
</div>
