<script lang="ts">
  import { THEMES, setTheme, themeState } from "../theme.svelte";
</script>

<div class="theme-dots" role="group" aria-label="设计语言快捷切换">
  <span class="td-label">设计语言</span>
  {#each THEMES as t (t.id)}
    <button
      class="td-dot"
      class:active={themeState.id === t.id}
      style="--dot-bg: linear-gradient(135deg, {t.dot[0]} 50%, {t.dot[1]} 50%)"
      title="{t.name} · {t.en}"
      aria-label="切换到{t.name}"
      aria-pressed={themeState.id === t.id}
      onclick={() => setTheme(t.id)}
    ></button>
  {/each}
</div>
