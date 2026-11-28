<script lang="ts">
  import { fade } from "svelte/transition";
  import Icon from "../components/Icon.svelte";
  import ProgressCard from "../components/ProgressCard.svelte";
  import {
    modrinthState,
    modrinthSearch,
    modrinthOpen,
    modrinthCloseDetail,
    modrinthInstall,
    instancesState,
    type ModrinthKind,
  } from "../store.svelte";
  import { bytes } from "../format";
  import type { SearchHit, ProjectVersion } from "../types";

  /** 单个 Modrinth 分区面板：搜索 / 筛选 / 结果 / 详情版本列表 / 一键安装。
   *  ZL2 搜索页（SearchModScreen）与 FCL DownloadPage 同款操作逻辑：
   *  关键词 → 平台/加载器/游戏版本筛选 → 详情 → 选版本一键安装（直落当前作用域）。 */
  let { kind }: { kind: ModrinthKind } = $props();

  const kindMeta: Record<ModrinthKind, { label: string; dir: string }> = {
    mod: { label: "模组", dir: "mods/" },
    resourcepack: { label: "资源包", dir: "resourcepacks/" },
    shaderpack: { label: "光影", dir: "shaderpacks/" },
    modpack: { label: "整合包", dir: "自动创建实例" },
  };

  const loaderChips = ["fabric", "forge", "quilt", "neoforge"];
  const sortChips: { id: string; label: string }[] = [
    { id: "relevance", label: "相关度" },
    { id: "downloads", label: "下载量" },
    { id: "updated", label: "更新时间" },
    { id: "newest", label: "最新发布" },
  ];

  let queryInput = $state("");

  /* 切到本分区时未加载过则自动搜索（下拉刷新由按钮触发，FCL 同款默认拉首页） */
  $effect(() => {
    if (modrinthState.lastKind !== kind) void modrinthSearch(kind);
  });

  function submitSearch() {
    modrinthState.query = queryInput;
    void modrinthSearch(kind);
  }
  function toggleLoader(l: string) {
    const i = modrinthState.loaders.indexOf(l);
    if (i >= 0) modrinthState.loaders.splice(i, 1);
    else modrinthState.loaders.push(l);
    void modrinthSearch(kind);
  }
  function setSort(id: string) {
    modrinthState.sort = id;
    void modrinthSearch(kind);
  }
  function onGvKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") void modrinthSearch(kind);
  }

  function fmtDl(n: number): string {
    if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
    if (n >= 1_000) return `${(n / 1_000).toFixed(0)}K`;
    return String(n);
  }

  function install(v: ProjectVersion) {
    void modrinthInstall(v.id, kind);
  }
</script>

<div class="toolbar rise" style="--i:1">
  <div class="search-box">
    <Icon name="search" />
    <input
      placeholder={`搜索 Modrinth ${kindMeta[kind].label}，如 sodium`}
      autocomplete="off"
      bind:value={queryInput}
      onkeydown={(e) => e.key === "Enter" && submitSearch()}
    />
    <button class="btn btn-sm btn-primary" onclick={submitSearch}>搜索</button>
  </div>
  <div class="chip-row" style="margin-top:8px">
    {#if kind === "mod"}
      {#each loaderChips as l (l)}
        <button class="chip" class:active={modrinthState.loaders.includes(l)} onclick={() => toggleLoader(l)}>
          {l.charAt(0).toUpperCase() + l.slice(1)}
        </button>
      {/each}
    {/if}
    {#each sortChips as s (s.id)}
      <button class="chip" class:active={modrinthState.sort === s.id} onclick={() => setSort(s.id)}>
        {s.label}
      </button>
    {/each}
    <input
      class="gv-input"
      placeholder="MC 版本，如 1.20.4"
      autocomplete="off"
      bind:value={modrinthState.gameVersions}
      onkeydown={onGvKeydown}
    />
  </div>
</div>

<div class="kv rise" style="--i:1">
  <span class="kv-key">安装作用域</span>
  <span class="kv-val">
    {instancesState.selectedId ? `实例 ${instancesState.selectedId}` : "全局 .minecraft"}
    （{kindMeta[kind].dir}）
  </span>
</div>

<div class="version-list rise" style="--i:2">
  {#if modrinthState.loading}
    <div class="list-note">搜索中…</div>
  {:else if modrinthState.error}
    <div class="list-note">搜索失败：{modrinthState.error}</div>
  {:else if (modrinthState.hits ?? []).length === 0}
    <div class="list-note">没有匹配的 {kindMeta[kind].label}</div>
  {:else}
    {#each (modrinthState.hits ?? []).slice(0, modrinthState.shown) as h (h.project_id)}
      <div class="v-row" style="align-items:flex-start">
        <span class="avatar avatar-sm">{h.title.slice(0, 1).toUpperCase()}</span>
        <div style="flex:1;min-width:0">
          <div class="v-name" style="min-width:auto">{h.title}</div>
          <div class="hint" style="margin-top:2px">{h.description}</div>
        </div>
        <span class="v-badges">
          <span class="badge badge-dim">↓ {fmtDl(h.downloads)}</span>
          {#if h.display_categories[0]}
            <span class="badge badge-dim">{h.display_categories[0]}</span>
          {/if}
        </span>
        <span class="v-actions">
          <button class="btn btn-sm btn-primary" onclick={() => modrinthOpen(h, kind)}>版本列表</button>
        </span>
      </div>
    {/each}
    {#if modrinthState.shown < (modrinthState.hits ?? []).length}
      <div class="load-more-row">
        <button class="btn btn-ghost" onclick={() => (modrinthState.shown += 20)}>
          加载更多（{Math.min(modrinthState.shown, modrinthState.hits?.length ?? 0)} / {modrinthState.hits?.length ?? 0}）
        </button>
      </div>
    {/if}
  {/if}
</div>

{#if modrinthState.detail}
  <div class="card panel" in:fade={{ duration: 160 }}>
    <div class="card-title-row">
      <Icon name="cube" />
      <h3>{modrinthState.detail.hit.title} · 版本列表</h3>
      <span class="title-spacer"></span>
      <button class="btn btn-sm btn-ghost" onclick={modrinthCloseDetail}>关闭</button>
    </div>
    {#if modrinthState.detailLoading}
      <div class="list-note">版本加载中…</div>
    {:else}
      {#each modrinthState.detail.versions as v (v.id)}
        <div class="v-row">
          <span class="v-name" style="min-width:auto">{v.version_number}</span>
          <span class="v-badges">
            {#if v.featured}<span class="badge badge-ok">推荐</span>{/if}
            <span class="badge badge-dim">{v.game_versions.slice(0, 2).join(" / ")}</span>
            <span class="badge badge-dim">{v.loaders.join(" / ")}</span>
            {#if v.dependencies.some((d) => d.dependency_type === "required")}
              <span class="badge badge-warn">{v.dependencies.filter((d) => d.dependency_type === "required").length} 个依赖</span>
            {/if}
            <span class="badge badge-dim">{bytes(v.files[0]?.size ?? 0)}</span>
          </span>
          <span class="v-actions">
            <button class="btn btn-sm btn-primary" onclick={() => install(v)}>
              一键安装
            </button>
          </span>
        </div>
      {:else}
        <div class="list-note">没有匹配当前筛选的版本</div>
      {/each}
    {/if}
    <p class="hint">一键安装：自动解析并下载全部必需依赖，文件名已存在则跳过（不覆盖你的文件）</p>
  </div>
{/if}

<ProgressCard
  title={`Modrinth ${kindMeta[kind].label}`}
  stage={modrinthState.progress.stage}
  percent={modrinthState.progress.percent}
  current={0}
  total={0}
  file={modrinthState.progress.file}
  visible={modrinthState.progress.visible}
  status={modrinthState.progress.status}
/>
