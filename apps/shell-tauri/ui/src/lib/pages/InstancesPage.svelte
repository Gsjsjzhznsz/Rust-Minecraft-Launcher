<script lang="ts">
  import { fade } from "svelte/transition";
  import Icon from "../components/Icon.svelte";
  import { createInstance, instanceAction, instancesState } from "../store.svelte";

  const loaders = ["", "fabric", "quilt", "forge", "neoforge", "optifine"];

  let name = $state("");
  let mc = $state("");
  let loader = $state("");
  let loaderVer = $state("");
  let creating = $state(false);

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
</script>

<section class="page">
  <div class="page-head rise" style="--i:0">
    <h2>游戏实例</h2>
    <p class="page-sub">版本隔离：saves / mods / config 独立，资源与库全局共享（FCL 同款语义）</p>
  </div>

  <div class="card glass rise" style="--i:1">
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
      <div class="list-note glass">还没有实例：在上方创建，或直接使用全局 .minecraft</div>
    {:else}
      {#each instancesState.list as inst (inst.id)}
        <div class="card glass account-card" class:selected={inst.selected}>
          <span class="avatar">{inst.name.slice(0, 1).toUpperCase()}</span>
          <div class="account-info">
            <div class="account-name">
              {inst.name}
              {#if inst.selected}<span class="badge badge-ok">当前</span>{/if}
            </div>
            <div class="account-sub">
              {inst.mcVersion} · {inst.loaderDesc} · Java {inst.javaMajor} · {inst.memoryMb}MB
            </div>
          </div>
          <div class="account-actions">
            {#if !inst.selected}
              <button class="btn btn-sm btn-primary" onclick={() => instanceAction("select", inst.id)}>选用</button>
              <button class="btn btn-sm" onclick={() => instanceAction("unselect", inst.id)}>取消隔离</button>
            {/if}
            <button class="btn btn-sm btn-danger" onclick={() => instanceAction("delete", inst.id)}>删除</button>
          </div>
        </div>
      {/each}
    {/if}
  </div>
</section>
