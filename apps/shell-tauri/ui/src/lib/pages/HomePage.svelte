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
    showPage,
    isSelectedInstalled,
    refreshAssetsStatus,
  } from "../store.svelte";

  /* 主页 = ZL2/FCL 同款操作逻辑：展示「当前选中版本」+ 一键启动。
     版本选择只列已安装版本（未安装的可在下载页获取）；切版/下载去下载页。 */
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

  <!-- Hero：当前版本卡 + 启动（ZL2 操作菜单 / FCL 右栏版本卡同款逻辑） -->
  <div class="card panel panel-accent hero-card rise" style="--i:1">
    <div class="hero-main">
      <div class="hero-select">
        <label class="field-label" for="version-select">当前版本</label>
        {#if versionsState.installed.length > 0}
          <select id="version-select" value={selected} onchange={onVersionChange}>
            {#each versionsState.installed as iv (iv)}
              <option value={iv}>{iv}</option>
            {/each}
            {#if selected && !versionsState.installed.includes(selected)}
              <option value={selected}>{selected}（未安装）</option>
            {/if}
          </select>
        {:else}
          <select disabled>
            <option value="">还没有已安装版本</option>
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
      <button class="btn btn-sm btn-ghost" style="margin-left:auto" onclick={() => showPage("download")}>
        获取更多版本 / 内容
      </button>
    </div>
    <p class="hint" style="margin-top:8px">
      启动前自动链：Java 运行时按需下载 → 资源文件缺失自动补全 → 一键进世界
    </p>
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
