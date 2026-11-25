<script lang="ts">
  import { bytes } from "../format";
  import { contentState, deleteContent, instancesState, refreshContent } from "../store.svelte";

  const kinds = [
    { kind: "mods", label: "Mods" },
    { kind: "resourcepacks", label: "资源包" },
    { kind: "shaderpacks", label: "光影" },
    { kind: "saves", label: "存档" },
    { kind: "screenshots", label: "截图" },
  ];

  function setKind(kind: string) {
    contentState.kind = kind;
    refreshContent();
  }
</script>

<section class="page">
  <div class="page-head rise" style="--i:0">
    <h2>内容管理</h2>
    <p class="page-sub">当前实例或全局目录下的 mods / 资源包 / 光影 / 存档</p>
  </div>

  <div class="chip-row rise" style="--i:1">
    {#each kinds as k (k.kind)}
      <button class="chip" class:active={contentState.kind === k.kind} onclick={() => setKind(k.kind)}>
        {k.label}
      </button>
    {/each}
  </div>

  <div class="kv rise" style="--i:2">
    <span class="kv-key">作用域</span>
    <span class="kv-val">
      {instancesState.selectedId ? `实例 ${instancesState.selectedId}` : "全局 .minecraft"}
    </span>
  </div>

  <div class="version-list rise" style="--i:3">
    {#if contentState.error}
      <div class="list-note">加载失败：{contentState.error}</div>
    {:else if contentState.list === null}
      <div class="list-note">加载中…</div>
    {:else if contentState.list.length === 0}
      <div class="list-note">暂无内容（{contentState.kind}）</div>
    {:else}
      {#each contentState.list as item (item.name)}
        <div class="v-row">
          <span class="avatar avatar-sm">{item.isDir ? "D" : "F"}</span>
          <span class="v-name" style="min-width:auto">{item.name}</span>
          <span class="v-badges">
            {bytes(item.sizeBytes)}
            {#if item.modifiedAt}
              · {new Date(item.modifiedAt * 1000).toLocaleDateString()}
            {/if}
          </span>
          <span class="v-actions">
            <button class="btn btn-sm btn-danger" onclick={() => deleteContent(item.name)}>删除</button>
          </span>
        </div>
      {/each}
    {/if}
  </div>
</section>
