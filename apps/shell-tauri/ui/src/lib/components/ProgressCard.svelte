<script lang="ts">
  import { fade } from "svelte/transition";
  import { bytes } from "../format";

  let {
    title,
    stage,
    percent,
    current,
    total,
    file,
    visible,
    status,
  }: {
    title: string;
    stage: string;
    percent: number;
    current: number;
    total: number;
    file: string;
    visible: boolean;
    status: { kind: "ok" | "err"; text: string } | null;
  } = $props();

  const width = $derived(Math.max(0, Math.min(100, percent)).toFixed(1));
  const text = $derived(
    total > 0 ? `${width}%（${bytes(current)} / ${bytes(total)}）` : total === 0 && current > 0 ? String(current) : `${width}%`,
  );
</script>

{#if visible}
  <div class="progress-card glass glass-accent" transition:fade>
    <div class="progress-head">
      <span class="progress-title">{title}</span>
      <span class="progress-stage">{stage}</span>
      <span class="progress-text">{text}</span>
    </div>
    <div class="progress-bar">
      <div class="progress-fill" style="width: {width}%"></div>
    </div>
    {#if file}
      <div class="progress-file">{file}</div>
    {/if}
    {#if status}
      <div class="alert alert-{status.kind}" transition:fade>{status.text}</div>
    {/if}
  </div>
{/if}
