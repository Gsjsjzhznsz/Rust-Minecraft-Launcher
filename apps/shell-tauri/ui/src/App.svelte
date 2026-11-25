<script lang="ts">
  import { onMount } from "svelte";
  import AuroraBackground from "./lib/components/AuroraBackground.svelte";
  import Toasts from "./lib/components/Toasts.svelte";
  import DiagBanner from "./lib/components/DiagBanner.svelte";
  import LoginScreen from "./lib/components/LoginScreen.svelte";
  import Shell from "./lib/components/Shell.svelte";
  import { boot, refreshPermission, session } from "./lib/store.svelte";

  onMount(async () => {
    await boot();
    // 授权后回到应用时自动重检权限（覆盖从系统设置返回的场景）
    document.addEventListener("visibilitychange", () => {
      if (document.visibilityState === "visible") void refreshPermission();
    });
    window.addEventListener("focus", () => void refreshPermission());
  });
</script>

<AuroraBackground />
<DiagBanner />

{#if session.entered}
  <Shell />
{:else}
  <LoginScreen />
{/if}

<Toasts />
