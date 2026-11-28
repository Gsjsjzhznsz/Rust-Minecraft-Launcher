<script lang="ts">
  import { fade } from "svelte/transition";
  import ProgressCard from "../components/ProgressCard.svelte";
  import ModrinthPanel from "../components/ModrinthPanel.svelte";
  import Icon from "../components/Icon.svelte";
  import {
    downloadState,
    installLoader,
    loadLoaderVersions,
    loaderState,
    retryLoadVersions,
    startDownload,
    versionsState,
    importModpackFile,
    importState,
    instancesState,
    type ModrinthKind,
  } from "../store.svelte";
  import { KIND_LABELS } from "../types";

  /* 下载页分区 = ZL2（游戏/整合包/模组/资源包/光影…8 分区）∩ FCL（7 Tab）同构；
     内容源 Modrinth（无需密钥的聚合生态源，ZL2/FCL 双源之一）。 */
  type Tab = "game" | ModrinthKind;
  let tab = $state<Tab>("game");
  const tabs: { id: Tab; label: string }[] = [
    { id: "game", label: "游戏" },
    { id: "mod", label: "模组" },
    { id: "resourcepack", label: "资源包" },
    { id: "shaderpack", label: "光影" },
    { id: "modpack", label: "整合包" },
  ];

  const kinds = [
    { kind: "all", label: "全部" },
    { kind: "release", label: "正式版" },
    { kind: "snapshot", label: "快照" },
    { kind: "old_beta", label: "旧测试版" },
    { kind: "old_alpha", label: "旧经典版" },
  ];

  const loaders = ["fabric", "quilt", "forge", "neoforge", "optifine"];

  const filtered = $derived(
    (versionsState.list ?? []).filter(
      (v) =>
        (versionsState.kindFilter === "all" || v.type === versionsState.kindFilter) &&
        (!versionsState.search.trim() ||
          v.id.toLowerCase().includes(versionsState.search.trim().toLowerCase())),
    ),
  );
  const shown = $derived(filtered.slice(0, versionsState.shown));

  const typeBadgeCls = (kind: string): string =>
    ({ release: "ok", snapshot: "warn", old_beta: "purple", old_alpha: "dim" })[kind] ?? "dim";

  function onSearch(e: Event) {
    versionsState.search = (e.target as HTMLInputElement).value;
    versionsState.shown = 50;
  }
  function setKindFilter(kind: string) {
    versionsState.kindFilter = kind;
    versionsState.shown = 50;
  }

  /* 整合包本地导入（ZL2 ImportModpackActivity / FCL ImportActivity 的应用内入口） */
  let fileInput = $state<HTMLInputElement | null>(null);
  function pickModpack() {
    fileInput?.click();
  }
  async function onModpackPicked(e: Event) {
    const input = e.target as HTMLInputElement;
    const file = input.files?.[0];
    if (file) await importModpackFile(file);
    input.value = "";
  }
</script>

<section class="page">
  <div class="page-head rise" style="--i:0">
    <h2>下载</h2>
    <p class="page-sub">游戏 / 模组 / 资源包 / 光影 / 整合包 —— 一键安装直落当前作用域</p>
  </div>

  <div class="chip-row rise" style="--i:0">
    {#each tabs as t (t.id)}
      <button class="chip" class:active={tab === t.id} onclick={() => (tab = t.id)}>
        {t.label}
      </button>
    {/each}
  </div>

  <ProgressCard
    title="下载任务"
    stage={downloadState.stage}
    percent={downloadState.percent}
    current={downloadState.current}
    total={downloadState.total}
    file={downloadState.file}
    visible={downloadState.visible}
    status={downloadState.status}
  />

  {#if tab === "game"}
    <div class="toolbar rise" style="--i:1">
      <div class="search-box">
        <Icon name="search" />
        <input placeholder="搜索版本，如 1.20.4" autocomplete="off" oninput={onSearch} />
      </div>
      <div class="chip-row">
        {#each kinds as k (k.kind)}
          <button
            class="chip"
            class:active={versionsState.kindFilter === k.kind}
            onclick={() => setKindFilter(k.kind)}
          >
            {k.label}
          </button>
        {/each}
      </div>
    </div>

    <div class="version-list rise" style="--i:2">
      {#if !versionsState.list}
        <div class="list-note">
          {versionsState.note}
          <div style="margin-top:12px">
            <button class="btn btn-sm btn-primary" onclick={retryLoadVersions}>重试拉取</button>
          </div>
        </div>
      {:else if filtered.length === 0}
        <div class="list-note">没有匹配的版本</div>
      {:else}
        {#each shown as v (v.id)}
          <div class="v-row">
            <span class="v-name">{v.id}</span>
            <span class="v-badges">
              <span class="badge badge-{typeBadgeCls(v.type)}">{KIND_LABELS[v.type] ?? v.type}</span>
              {#if versionsState.installed.includes(v.id)}
                <span class="badge badge-ok">已安装</span>
              {/if}
            </span>
            <span class="v-actions">
              <button
                class="btn btn-primary btn-sm"
                disabled={versionsState.busy}
                onclick={() => startDownload(v.id)}
              >
                {versionsState.installed.includes(v.id) ? "重新下载" : "下载"}
              </button>
            </span>
          </div>
        {/each}
      {/if}
    </div>

    {#if versionsState.list && versionsState.shown < filtered.length}
      <div class="load-more-row">
        <button
          class="btn btn-ghost"
          onclick={() => (versionsState.shown += 50)}
        >
          加载更多（{versionsState.shown} / {filtered.length}）
        </button>
      </div>
    {/if}

    <!-- Modloader 安装（ZL2 附加内容页 / FCL 组件安装同款能力） -->
    <div class="card panel rise" style="--i:3">
      <div class="card-title-row">
        <Icon name="note" />
        <h3>安装 Modloader</h3>
      </div>
      <p class="page-sub">基版本未装时自动先装原版；进度在下方实时展示</p>
      <div class="chip-row" style="margin-top:10px">
        {#each loaders as l (l)}
          <button
            class="chip"
            class:active={loaderState.kind === l}
            onclick={() => (loaderState.kind = l)}
          >
            {l.charAt(0).toUpperCase() + l.slice(1)}
          </button>
        {/each}
      </div>
      <div class="add-row" style="margin-top:12px">
        <input
          placeholder="MC 版本，如 1.20.4"
          autocomplete="off"
          bind:value={loaderState.mcInput}
          onkeydown={(e) => e.key === "Enter" && loadLoaderVersions()}
        />
        <button class="btn btn-primary" onclick={loadLoaderVersions}>查版本</button>
      </div>
      {#if loaderState.versions.length > 0}
        <div class="version-list" style="margin-top:12px;max-height:240px;overflow-y:auto">
          {#each loaderState.versions.slice(0, 40) as v (v.version)}
            <div class="v-row">
              <span class="v-name">{v.version}</span>
              <span class="v-badges">
                {#if v.stable}
                  <span class="badge badge-ok">稳定版</span>
                {:else}
                  <span class="badge badge-warn">测试版</span>
                {/if}
              </span>
              <span class="v-actions">
                <button class="btn btn-primary btn-sm" onclick={() => installLoader(v.version)}>安装</button>
              </span>
            </div>
          {/each}
          {#if loaderState.versions.length > 40}
            <div class="list-note">仅显示前 40 条（共 {loaderState.versions.length} 条）</div>
          {/if}
        </div>
      {:else if loaderState.error === ""}
        <div class="list-note">选择 loader 类型并输入 MC 版本后查询</div>
      {/if}
      {#if loaderState.error}
        <div class="alert alert-error">{loaderState.error}</div>
      {/if}
    </div>

    <ProgressCard
      title="Modloader"
      stage={loaderState.stage}
      percent={loaderState.percent}
      current={0}
      total={0}
      file={loaderState.file}
      visible={loaderState.visible}
      status={loaderState.status}
    />
  {:else if tab === "modpack"}
    <!-- 整合包分区：Modrinth 在线下载 + 本地导入（下载完成自动建实例装依赖） -->
    <div class="card panel rise" style="--i:1">
      <div class="card-title-row">
        <Icon name="folder" />
        <h3>本地导入整合包</h3>
      </div>
      <p class="page-sub">
        选择 .mrpack / .zip 文件：自动解析 → 建实例 → 链式装基版本与加载器 → 下载 overrides 与全部模组。
        APK 里也可以直接在文件管理器点开整合包文件自动进入导入（ZL2/FCL 同款文件关联）。
      </p>
      <input
        bind:this={fileInput}
        type="file"
        accept=".mrpack,.zip,application/zip,application/octet-stream"
        style="display:none"
        onchange={onModpackPicked}
      />
      <button class="btn btn-primary" style="margin-top:10px" disabled={importState.busy} onclick={pickModpack}>
        {importState.busy ? `导入中（${importState.fileName}）…` : "选择整合包文件"}
      </button>
    </div>
    <ModrinthPanel kind="modpack" />
    <ProgressCard
      title="整合包导入"
      stage={importState.progress.stage}
      percent={importState.progress.percent}
      current={0}
      total={0}
      file={importState.progress.file}
      visible={importState.progress.visible || importState.progress.status !== null}
      status={importState.progress.status}
    />
  {:else}
    <ModrinthPanel kind={tab} />
  {/if}
</section>
