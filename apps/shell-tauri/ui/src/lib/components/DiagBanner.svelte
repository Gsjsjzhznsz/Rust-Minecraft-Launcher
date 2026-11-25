<script lang="ts">
  import { fade } from "svelte/transition";
  import { diag } from "../store.svelte";
</script>

{#if diag.visible}
  <div class="diag-banner" transition:fade>
    <pre class="diag-text">{diag.text}</pre>
    <div class="diag-actions">
      <button class="btn btn-sm" onclick={() => (diag.visible = false)}>知道了</button>
    </div>
  </div>
{/if}
