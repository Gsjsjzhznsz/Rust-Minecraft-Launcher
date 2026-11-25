<script lang="ts">
  import { fade } from "svelte/transition";
  import Icon from "./Icon.svelte";
  import { showPage, session, permBanners, requestPermission, envState } from "../store.svelte";
  import { avatarLetter, avatarStyle } from "../format";
  import HomePage from "../pages/HomePage.svelte";
  import DownloadPage from "../pages/DownloadPage.svelte";
  import InstancesPage from "../pages/InstancesPage.svelte";
  import ContentPage from "../pages/ContentPage.svelte";
  import AccountPage from "../pages/AccountPage.svelte";
  import SettingsPage from "../pages/SettingsPage.svelte";

  const nav = [
    { page: "home", label: "主页", icon: "home" },
    { page: "download", label: "下载游戏", icon: "download" },
    { page: "instances", label: "实例", icon: "grid" },
    { page: "content", label: "内容", icon: "layers" },
    { page: "account", label: "账户", icon: "user" },
    { page: "settings", label: "设置", icon: "settings" },
  ] as const;

  const bottomNav = [
    { page: "home", label: "主页", icon: "home" },
    { page: "download", label: "下载", icon: "download" },
    { page: "instances", label: "实例", icon: "grid" },
    { page: "content", label: "内容", icon: "layers" },
    { page: "account", label: "账户", icon: "user" },
    { page: "settings", label: "设置", icon: "settings" },
  ] as const;

  function avatarBg(): string {
    return session.account ? avatarStyle(session.account.uuid) : "";
  }
</script>

<section class="screen app-shell">
  <!-- 侧栏（宽屏） -->
  <aside class="sidebar">
    <div class="brand">
      <div class="logo logo-sm">Q</div>
      <div class="brand-name">Quartz</div>
    </div>
    <nav class="nav" id="nav">
      {#each nav as item (item.page)}
        <button
          class="nav-item"
          class:active={session.page === item.page}
          onclick={() => showPage(item.page)}
        >
          <Icon name={item.icon} />
          <span>{item.label}</span>
        </button>
      {/each}
    </nav>
    <div class="sidebar-foot">
      <div class="mini-account">
        <span class="avatar avatar-sm" style={avatarBg()}>
          {session.account ? avatarLetter(session.account.username) : "?"}
        </span>
        <span class="mini-name">{session.account?.username ?? "未登录"}</span>
      </div>
      <div class="version-tag">v{envState.meta?.version ?? "-"}</div>
    </div>
  </aside>

  <!-- 顶栏（窄屏） -->
  <header class="topbar">
    <div class="brand small">
      <div class="logo logo-sm">Q</div>
      <div class="brand-name">Quartz</div>
    </div>
    <button
      id="topbar-account"
      class="mini-account topbar-account-btn"
      onclick={() => showPage("account")}
    >
      <span class="avatar avatar-sm" style={avatarBg()}>
        {session.account ? avatarLetter(session.account.username) : "?"}
      </span>
      <span class="mini-name">{session.account?.username ?? "未登录"}</span>
    </button>
  </header>

  <main class="content">
    {#if permBanners.main}
      <div class="perm-banner" transition:fade>
        <span class="perm-banner-text">
          未授予完整文件权限：游戏文件保存在应用私有目录，卸载应用会丢失。建议授权后使用标准目录（可浏览、卸载不丢）。
        </span>
        <button class="btn btn-sm btn-primary" onclick={requestPermission}>立即授权</button>
      </div>
    {/if}

    {#key session.page}
      <div class="page-wrap" in:fade={{ duration: 200 }}>
        {#if session.page === "home"}
          <HomePage />
        {:else if session.page === "download"}
          <DownloadPage />
        {:else if session.page === "instances"}
          <InstancesPage />
        {:else if session.page === "content"}
          <ContentPage />
        {:else if session.page === "account"}
          <AccountPage />
        {:else if session.page === "settings"}
          <SettingsPage />
        {/if}
      </div>
    {/key}
  </main>

  <!-- 底部导航（窄屏玻璃条） -->
  <nav class="bottom-nav" id="bottom-nav">
    {#each bottomNav as item (item.page)}
      <button
        class="nav-item"
        class:active={session.page === item.page}
        onclick={() => showPage(item.page)}
      >
        <Icon name={item.icon} />
        <span>{item.label}</span>
      </button>
    {/each}
  </nav>
</section>
