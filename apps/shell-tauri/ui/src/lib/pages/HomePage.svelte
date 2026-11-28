<script lang="ts">
  import { fade } from "svelte/transition";
  import Icon from "../components/Icon.svelte";
  import ProgressCard from "../components/ProgressCard.svelte";
  import {
    copyExitDetail,
    downloadState,
    launch,
    launchState,
    clearConsole,
    rtState,
    stopGame,
    versionsState,
    envState,
    isSelectedInstalled,
    refreshAssetsStatus,
  } from "../store.svelte";
  import { KIND_LABELS, KIND_ORDER } from "../types";

  /* 选中版本下拉（含恢复上次选择） */
  const selected = $derived(versionsState.selected);
  const installedBadge = $derived(
    !selected
      ? { text: "未选择版本", cls: "dim" }
      : isSelectedInstalled()
        ? { text: "已安装", cls: "ok" }
        : { text: "未安装，可在下载页获取", cls: "warn" },
  );

  function onVersionChange(e: Event) {
    versionsState.selected = (e.target as HTMLSelectElement).value;
    window.localStorage.setItem("quartz.selectedVersion", versionsState.selected);
    refreshAssetsStatus();
  }

  const consoleBox: { box: HTMLDivElement | null } = { box: null };
  $effect(() => {
    if (consoleBox.box) consoleBox.box.scrollTop = consoleBox.box.scrollHeight;
  });
</script>

<section class="page">
  <div class="page-head rise" style="--i:0">
    <h2>主页</h2>
    <p class="page-sub">选择版本，一键启动或下载</p>
  </div>

  <!-- Hero：版本 + 启动 -->
  <div class="card panel panel-accent hero-card rise" style="--i:1">
    <div class="hero-main">
      <div class="hero-select">
        <label class="field-label" for="version-select">游戏版本</label>
        {#if versionsState.list}
          <select id="version-select" value={selected} onchange={onVersionChange}>
            {#each KIND_ORDER as kind (kind)}
              {#if versionsState.list!.filter((v) => v.type === kind).length > 0}
                <optgroup label={KIND_LABELS[kind]}>
                  {#each versionsState.list!.filter((v) => v.type === kind) as v (v.id)}
                    <option value={v.id}>{v.id}</option>
                  {/each}
                </optgroup>
              {/if}
            {/each}
            {#each versionsState.installed as iv (iv)}
              {#if !versionsState.list?.some((v) => v.id === iv)}
                <option value={iv}>{iv}（已安装）</option>
              {/if}
            {/each}
          </select>
        {:else}
          <select disabled>
            {#if versionsState.installed.length > 0}
              {#each versionsState.installed as iv (iv)}
                <option value={iv}>{iv}（已安装）</option>
              {/each}
            {:else}
              <option value="">版本清单未获取，请前往「下载游戏」页</option>
            {/if}
          </select>
        {/if}
      </div>
      <button class="btn-launch" onclick={launch}>
        <Icon name="play" size="lg" />
        <span>启动游戏</span>
      </button>
    </div>
    <div class="hero-meta">
      <span class="badge badge-{installedBadge.cls}">{installedBadge.text}</span>
      <span class="badge badge-{envState.runtimeBadge.cls}">{envState.runtimeBadge.text}</span>
      <span class="hero-hint">M0 启动链路已接入：启动前自动准备 Java 运行时与游戏文件</span>
    </div>
  </div>

  <!-- 游戏下载进度（home/dl 共享状态） -->
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

  <!-- Bento：控制台 + 游戏目录 -->
  <div class="bento">
    <div class="card panel console-card rise" style="--i:2">
      <div class="card-title-row">
        <Icon name="console" />
        <h3>启动控制台</h3>
        <span class="badge badge-{launchState.badgeCls}">{launchState.badgeText}</span>
        <span class="title-spacer"></span>
        {#if launchState.stopVisible}
          <button class="btn btn-sm btn-danger" onclick={stopGame}>停止游戏</button>
        {/if}
        <button class="btn btn-sm btn-ghost" onclick={clearConsole}>清屏</button>
      </div>
      <div class="console-box" bind:this={consoleBox.box}>
        {#each launchState.lines as l}
          <div class="console-line {l.source === 'stderr' ? 'err' : l.source === 'quartz' ? 'sys' : l.source === 'dim' ? 'dim' : ''}">{l.line}</div>
        {/each}
      </div>
      {#if launchState.exitText !== null}
        <div class="exit-detail" transition:fade>
          <div class="exit-detail-head">
            <span class="exit-detail-title">游戏退出详情（含日志尾部）</span>
            <button class="btn btn-sm btn-ghost" onclick={copyExitDetail}>复制</button>
            <button class="btn btn-sm btn-ghost" onclick={() => (launchState.exitText = null)}>关闭</button>
          </div>
          <pre class="exit-detail-text">{launchState.exitText}</pre>
        </div>
      {/if}
      <ProgressCard
        title="Java 运行时"
        stage={rtState.stage}
        percent={rtState.percent}
        current={rtState.current}
        total={rtState.total}
        file={rtState.file}
        visible={rtState.visible}
        status={null}
      />
    </div>

    <div class="card panel rise" style="--i:3">
      <div class="card-title-row">
        <Icon name="folder" />
        <h3>游戏目录</h3>
        <span class="badge badge-{envState.storageBadge.cls}">{envState.storageBadge.text}</span>
      </div>
      <code class="path-line">{envState.gameDir}</code>
      <p class="hint">标准 .minecraft 布局：versions / libraries / assets / saves / mods / resourcepacks / config / logs</p>
    </div>
  </div>
</section>
