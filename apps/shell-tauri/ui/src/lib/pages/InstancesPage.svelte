<script lang="ts">
  import { fade } from "svelte/transition";
  import Icon from "../components/Icon.svelte";
  import {
    contentState,
    createInstance,
    deleteContent,
    duplicateInstance,
    instanceAction,
    instancesState,
    refreshContent,
    renameInstance,
    setInstanceMemory,
    setInstanceRenderer,
    toggleContent,
    envState,
  } from "../store.svelte";
  import { bytes } from "../format";

  /* 实例页 = FCL ManageUI（版本设置/模组/存档/资源包/光影）∩ ZL2 版本详情
     （概览/配置/Mod/存档/资源包/光影/截图）同构：实例列表 + 内容分区并入，
     不再单独设一级「内容」页。 */
  type Tab = "instances" | "mods" | "resourcepacks" | "shaderpacks" | "saves" | "screenshots";
  let tab = $state<Tab>("instances");
  const tabs: { id: Tab; label: string }[] = [
    { id: "instances", label: "实例" },
    { id: "mods", label: "模组" },
    { id: "resourcepacks", label: "资源包" },
    { id: "shaderpacks", label: "光影" },
    { id: "saves", label: "存档" },
    { id: "screenshots", label: "截图" },
  ];

  const loaders = ["", "fabric", "quilt", "forge", "neoforge", "optifine"];

  let name = $state("");
  let mc = $state("");
  let loader = $state("");
  let loaderVer = $state("");
  let creating = $state(false);

  /* 展开的实例设置（内存/渲染器按实例覆盖，FCL VersionSettingPage 同款） */
  let expandedId = $state<string | null>(null);
  let memDraft = $state<Record<string, number>>({});
  let renamingId = $state<string | null>(null);
  let renameDraft = $state("");

  async function submit() {
    creating = true;
    const ok = await createInstance(name.trim(), mc.trim(), loader, loaderVer.trim());
    if (ok) {
      name = "";
      mc = "";
      loaderVer = "";
    }
    creating = false;
  }

  function switchTab(t: Tab) {
    tab = t;
    if (t !== "instances") {
      contentState.kind = t;
      void refreshContent();
    }
  }

  function armRename(inst: { id: string; name: string }) {
    renamingId = renamingId === inst.id ? null : inst.id;
    renameDraft = inst.name;
  }
  function submitRename(inst: { id: string }) {
    void renameInstance(inst.id, renameDraft);
    renamingId = null;
  }
  function toggleExpand(inst: { id: string; memoryMb: number }) {
    if (expandedId === inst.id) {
      expandedId = null;
    } else {
      expandedId = inst.id;
      memDraft[inst.id] = inst.memoryMb;
    }
  }
</script>

<section class="page">
  <div class="page-head rise" style="--i:0">
    <h2>实例管理</h2>
    <p class="page-sub">版本隔离 + 模组 / 资源包 / 光影 / 存档 / 截图集中管理（FCL/ZL2 同款布局）</p>
  </div>

  <div class="chip-row rise" style="--i:0">
    {#each tabs as t (t.id)}
      <button class="chip" class:active={tab === t.id} onclick={() => switchTab(t.id)}>
        {t.label}
      </button>
    {/each}
  </div>

  {#if tab === "instances"}
    <div class="card panel rise" style="--i:1">
      <div class="card-title-row">
        <Icon name="grid" />
        <h3>新建实例</h3>
      </div>
      <div class="add-row">
        <input placeholder="实例名称，如 生存服整合" autocomplete="off" bind:value={name} />
      </div>
      <div class="add-row" style="margin-top:10px">
        <input placeholder="MC 版本，如 1.20.4" autocomplete="off" bind:value={mc} />
      </div>
      <div class="add-row" style="margin-top:10px">
        <select bind:value={loader} style="flex:1">
          <option value="">无（原版）</option>
          {#each loaders.slice(1) as l (l)}
            <option value={l}>{l.charAt(0).toUpperCase() + l.slice(1)}</option>
          {/each}
        </select>
        <input placeholder="loader 版本（可留空自动选最新稳定）" autocomplete="off" bind:value={loaderVer} />
      </div>
      <button class="btn btn-primary" style="margin-top:14px" disabled={creating} onclick={submit}>
        创建实例
      </button>
      {#if instancesState.formError}
        <div class="alert alert-error" transition:fade>{instancesState.formError}</div>
      {/if}
    </div>

    <div class="account-list rise" style="--i:2">
      {#if instancesState.list.length === 0}
        <div class="list-note panel">还没有实例：在上方创建，或直接使用全局 .minecraft</div>
      {:else}
        {#each instancesState.list as inst (inst.id)}
          <div class="card panel account-card" class:selected={inst.selected}>
            <span class="avatar">{inst.name.slice(0, 1).toUpperCase()}</span>
            <div class="account-info">
              {#if renamingId === inst.id}
                <div class="add-row" style="margin-top:0">
                  <input bind:value={renameDraft} onkeydown={(e) => e.key === "Enter" && submitRename(inst)} />
                  <button class="btn btn-sm btn-primary" onclick={() => submitRename(inst)}>保存</button>
                  <button class="btn btn-sm btn-ghost" onclick={() => (renamingId = null)}>取消</button>
                </div>
              {:else}
                <div class="account-name">
                  {inst.name}
                  {#if inst.selected}<span class="badge badge-ok">当前</span>{/if}
                  {#if inst.renderer}<span class="badge badge-dim">渲染层覆盖：{inst.renderer}</span>{/if}
                </div>
                <div class="account-sub">
                  {inst.mcVersion} · {inst.loaderDesc} · Java {inst.javaMajor} · {inst.memoryMb}MB
                </div>
              {/if}
            </div>
            <div class="account-actions">
              {#if !inst.selected}
                <button class="btn btn-sm btn-primary" onclick={() => instanceAction("select", inst.id)}>选用</button>
                <button class="btn btn-sm" onclick={() => instanceAction("unselect", inst.id)}>取消隔离</button>
              {/if}
              <button class="btn btn-sm btn-ghost" onclick={() => toggleExpand(inst)}>
                {expandedId === inst.id ? "收起设置" : "设置"}
              </button>
              <button class="btn btn-sm btn-ghost" onclick={() => armRename(inst)}>重命名</button>
              <button class="btn btn-sm btn-ghost" onclick={() => duplicateInstance(inst.id)}>复制</button>
              <button class="btn btn-sm btn-danger" onclick={() => instanceAction("delete", inst.id)}>删除</button>
            </div>
            {#if expandedId === inst.id}
              <div class="instance-settings" in:fade={{ duration: 140 }}>
                <div class="kv">
                  <span class="kv-key">内存分配</span>
                  <span class="kv-val">{memDraft[inst.id] ?? inst.memoryMb} MB</span>
                </div>
                <input
                  type="range"
                  min="512"
                  max="4096"
                  step="256"
                  value={memDraft[inst.id] ?? inst.memoryMb}
                  oninput={(e) => (memDraft[inst.id] = Number((e.target as HTMLInputElement).value))}
                />
                <button
                  class="btn btn-sm btn-primary"
                  style="margin-top:8px"
                  onclick={() => setInstanceMemory(inst.id, memDraft[inst.id] ?? inst.memoryMb)}
                >
                  保存内存
                </button>
                <div class="kv" style="margin-top:10px">
                  <span class="kv-key">渲染层（按实例覆盖）</span>
                  <select
                    style="flex:1"
                    value={inst.renderer ?? ""}
                    onchange={(e) =>
                      setInstanceRenderer(inst.id, (e.target as HTMLSelectElement).value || null)}
                  >
                    <option value="">跟随全局</option>
                    {#each envState.renderers.filter((r) => r.installed) as r (r.id)}
                      <option value={r.id}>{r.name}</option>
                    {/each}
                  </select>
                </div>
                <p class="hint">实例设置优先于全局设置（FCL/ZL2 版本覆盖语义）；渲染层需已安装才可选用</p>
              </div>
            {/if}
          </div>
        {/each}
      {/if}
    </div>
  {:else}
    <!-- 内容分区（模组/资源包/光影/存档/截图）：列表 + 启用禁用 + 删除 -->
    <div class="kv rise" style="--i:1">
      <span class="kv-key">作用域</span>
      <span class="kv-val">
        {instancesState.selectedId ? `实例 ${instancesState.selectedId}` : "全局 .minecraft"}
      </span>
      <button class="btn btn-sm btn-ghost" style="margin-left:auto" onclick={() => instanceAction("unselect", "")}>
        切回全局
      </button>
    </div>

    <div class="version-list rise" style="--i:2">
      {#if contentState.error}
        <div class="list-note">加载失败：{contentState.error}</div>
      {:else if contentState.list === null}
        <div class="list-note">加载中…</div>
      {:else if contentState.list.length === 0}
        <div class="list-note">暂无内容（{tab === "mods" ? "mods" : tab}）—— 可在「下载」页获取</div>
      {:else}
        {#each contentState.list as item (item.name)}
          <div class="v-row">
            <span class="avatar avatar-sm">{item.isDir ? "D" : "F"}</span>
            <span class="v-name" style="min-width:auto" class:disabled-item={item.disabled}>{item.name}</span>
            <span class="v-badges">
              {bytes(item.sizeBytes)}
              {#if item.modifiedAt}
                · {new Date(item.modifiedAt * 1000).toLocaleDateString()}
              {/if}
              {#if item.disabled}
                <span class="badge badge-warn">已禁用</span>
              {/if}
            </span>
            <span class="v-actions">
              {#if tab === "mods" && !item.isDir}
                <button class="btn btn-sm" onclick={() => toggleContent(item.name)}>
                  {item.disabled ? "启用" : "禁用"}
                </button>
              {/if}
              <button class="btn btn-sm btn-danger" onclick={() => deleteContent(item.name)}>删除</button>
            </span>
          </div>
        {/each}
      {/if}
    </div>
    {#if tab === "mods"}
      <p class="hint rise" style="--i:3">
        禁用 = 文件改名加 .disabled 后缀（FCL/ZL2 同款语义），文件保留、随时可启用。
      </p>
    {/if}
  {/if}
</section>
