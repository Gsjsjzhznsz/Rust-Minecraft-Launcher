<script lang="ts">
  import { fade } from "svelte/transition";
  import Icon from "../components/Icon.svelte";
  import ProgressCard from "../components/ProgressCard.svelte";
  import {
    copyLogTail,
    downloadLwjgl,
    downloadRenderer,
    envState,
    permBanners,
    refreshAssetsStatus,
    repairAssets,
    requestPermission,
    rendererState,
    saveAzureClientId,
    saveJvmArgs,
    saveMemorySetting,
    settingsState,
    switchRenderer,
    viewLogTail,
  } from "../store.svelte";
  import { bytes } from "../format";
  import ThemePicker from "../components/ThemePicker.svelte";

  let logSelect = $state("");
  let azureInput = $state("");
  let jvmInput = $state("");
  let memSlider = $state(1024);
  let jvmInit = $state(false);

  /* 首次进入把后端值填进本地输入控件 */
  $effect(() => {
    if (settingsState.jvmArgs !== "" && !jvmInit) {
      jvmInput = settingsState.jvmArgs;
      azureInput = settingsState.azureId;
      memSlider = settingsState.memory;
      jvmInit = true;
    }
  });

  const memPercent = $derived(((memSlider - 512) / (4096 - 512)) * 100);
  const selectedLog = $derived(settingsState.logs.find((l) => l.name === logSelect));

  function onMemInput(e: Event) {
    memSlider = Number((e.target as HTMLInputElement).value);
  }
</script>

<section class="page">
  <div class="page-head rise" style="--i:0">
    <h2>设置</h2>
    <p class="page-sub">外观 / 存储与关于</p>
  </div>

  <!-- 外观：多设计语言实时切换 -->
  <div class="card panel rise" style="--i:1">
    <div class="card-title-row">
      <Icon name="layers" />
      <h3>外观 · 设计语言</h3>
      <span class="badge badge-dim">实时切换</span>
    </div>
    <p class="page-sub">
      21 种设计语言（配色取材 Style Atlas 图鉴），分五组：玻璃与光 / 产品语言 / 极简理性 / 质感趣味 / 极客霓虹。
      每种语言是一套完整的 Token 契约 + 组件约束（色彩 / 形状 / 字体 / 动效节奏），
      点击卡片即时应用全站，选择会被记住；侧栏与顶栏的色点可随时快切。主打：玻璃拟态。
    </p>
    <ThemePicker />
  </div>

  <!-- 存储 -->
  <div class="card panel rise" style="--i:2">
    <div class="card-title-row">
      <Icon name="folder" />
      <h3>存储</h3>
      <span class="badge badge-{envState.storageBadge.cls}">{envState.storageBadge.text}</span>
    </div>
    <div class="kv"><span class="kv-key">游戏根目录</span><code class="path-line">{envState.gameDir}</code></div>
    <div class="kv"><span class="kv-key">存储模式</span><span class="kv-val">{envState.storageMode}</span></div>
    <div class="kv"><span class="kv-key">可用空间</span><span class="kv-val">{envState.storageFree}</span></div>
    <div class="kv"><span class="kv-key">完整文件权限</span><span class="kv-val">{envState.permState}</span></div>
    <div class="kv"><span class="kv-key">Java 运行时</span><span class="kv-val">{envState.runtimeText}</span></div>
    <div class="kv"><span class="kv-key">标准布局</span><span class="kv-val">{envState.layoutStatus}</span></div>
    {#if permBanners.settings}
      <div class="alert alert-warn" transition:fade>
        未授予完整文件权限。
        <button class="btn btn-sm btn-primary" style="margin-top:8px" onclick={requestPermission}>
          授权完整文件权限
        </button>
      </div>
    {/if}
  </div>

  <!-- 渲染层 -->
  <div class="card panel rise" style="--i:3">
    <div class="card-title-row">
      <Icon name="cube" />
      <h3>渲染层</h3>
      <span class="badge badge-{envState.rendererBadge.cls}">{envState.rendererBadge.text}</span>
    </div>
    <p class="page-sub">GL 转译层与 JRE、LWJGL 一样全部在线下载，不随 APK 内置</p>
    <div>
      {#each envState.renderers as r (r.id)}
        <div class="kv" style="flex-direction:row;align-items:center;gap:12px">
          <span class="kv-key" style="min-width:110px">{r.name}</span>
          <span class="kv-val" style="flex:1">{r.note} · {r.sizeHint}</span>
          {#if r.active}
            <span class="badge badge-ok">使用中</span>
          {:else if r.installed}
            <span class="badge badge-dim">已安装</span>
          {:else if !r.specAvailable}
            <span class="badge badge-dim">暂无源</span>
          {/if}
          {#if r.specAvailable && !r.installed}
            <button class="btn btn-sm btn-primary" onclick={() => downloadRenderer(r.id)}>下载</button>
          {/if}
          {#if r.installed && !r.active}
            <button class="btn btn-sm btn-ghost" onclick={() => switchRenderer(r.id, r.name)}>切换</button>
          {/if}
        </div>
      {/each}
    </div>
    <div class="kv">
      <span class="kv-key">LWJGL 安卓包</span>
      <span class="kv-val">{envState.lwjglState}</span>
      {#if !envState.lwjglOk}
        <button class="btn btn-sm btn-primary" onclick={downloadLwjgl}>下载</button>
      {/if}
    </div>
    <ProgressCard
      title="渲染层"
      stage={rendererState.stage}
      percent={rendererState.percent}
      current={rendererState.current}
      total={rendererState.total}
      file={rendererState.file}
      visible={rendererState.visible}
      status={null}
    />
  </div>

  <!-- 性能 -->
  <div class="card panel rise" style="--i:4">
    <div class="card-title-row">
      <Icon name="bars" />
      <h3>性能</h3>
    </div>
    <div class="kv"><span class="kv-key">游戏内存分配</span><span class="kv-val">{memSlider} MB</span></div>
    <input
      type="range"
      min="512"
      max="4096"
      step="256"
      value={memSlider}
      style="--mem-p: {memPercent}%"
      oninput={onMemInput}
    />
    <div class="kv"><span class="kv-key">建议</span><span class="kv-val">2GB 内存设备 ≤1024MB；4GB 设备 1536–2048MB</span></div>
    <button class="btn btn-sm btn-primary" onclick={() => saveMemorySetting(memSlider)}>保存内存设置</button>
  </div>

  <!-- 启动参数 -->
  <div class="card panel rise" style="--i:5">
    <div class="card-title-row">
      <Icon name="console" />
      <h3>启动参数</h3>
    </div>
    <p class="page-sub">自定义 JVM 参数（每行一条，追加到启动计划；-Xmx 由内存设置管理，无需重复填写）</p>
    <textarea class="tree" rows="3" placeholder={"-XX:+UseG1GC&#10;-Dfml.ignoreInvalidMinecraftCertificates=true"} bind:value={jvmInput}></textarea>
    <button class="btn btn-sm btn-primary" style="margin-top:10px" onclick={() => saveJvmArgs(jvmInput)}>
      保存 JVM 参数
    </button>
  </div>

  <!-- 微软登录配置 -->
  <div class="card panel rise" style="--i:6">
    <div class="card-title-row">
      <Icon name="user" />
      <h3>微软登录配置</h3>
    </div>
    <p class="page-sub">项目自有 Azure Public Client ID（Public Client 允许公开，不涉密钥）</p>
    <div class="add-row" style="margin-top:10px">
      <input placeholder="Azure client_id（GUID）" autocomplete="off" bind:value={azureInput} />
      <button class="btn btn-sm btn-primary" onclick={() => saveAzureClientId(azureInput.trim())}>保存</button>
    </div>
    <div class="kv"><span class="kv-key">当前状态</span><span class="kv-val">{settingsState.azureState}</span></div>
  </div>

  <!-- 游戏文件维护 -->
  <div class="card panel rise" style="--i:7">
    <div class="card-title-row">
      <Icon name="refresh" />
      <h3>游戏文件维护</h3>
      <span class="badge badge-{settingsState.assetsBadge.cls}">{settingsState.assetsBadge.text}</span>
    </div>
    <p class="page-sub">启动前会自动检测并补全缺失的资源文件；也可在此手动修复</p>
    <div class="kv"><span class="kv-key">资产索引</span><span class="kv-val">{settingsState.assetsIndex}</span></div>
    <div class="kv"><span class="kv-key">对象文件</span><span class="kv-val">{settingsState.assetsObjects}</span></div>
    <button class="btn btn-sm btn-primary" style="margin-top:8px" onclick={repairAssets}>修复资源文件</button>
  </div>

  <!-- 启动日志 -->
  <div class="card panel rise" style="--i:8">
    <div class="card-title-row">
      <Icon name="file" />
      <h3>启动日志</h3>
    </div>
    <p class="page-sub">游戏崩溃时把这里的日志尾部一并发给开发者</p>
    <div class="kv" style="gap:8px">
      <select bind:value={logSelect}>
        {#if settingsState.logs.length === 0}
          <option value="">暂无日志（先启动一次游戏）</option>
        {:else}
          {#each settingsState.logs as l (l.name)}
            <option value={l.name}>{l.name}（{bytes(l.size)}，{new Date(l.modifiedMs).toLocaleString()}）</option>
          {/each}
        {/if}
      </select>
      <div style="display:flex;gap:8px;margin-top:6px">
        <button class="btn btn-sm btn-primary" onclick={() => viewLogTail(logSelect)}>查看尾部</button>
        <button class="btn btn-sm btn-ghost" onclick={() => copyLogTail(logSelect)}>复制</button>
      </div>
    </div>
    {#if settingsState.logText !== null}
      <pre class="tree" style="max-height:240px;overflow:auto">{settingsState.logText}</pre>
    {/if}
  </div>

  <!-- 目录结构 -->
  <div class="card panel rise" style="--i:9">
    <div class="card-title-row">
      <Icon name="file" />
      <h3>标准 .minecraft 目录结构</h3>
    </div>
    <pre class="tree">files/
├── .minecraft/
│   ├── versions/&lt;v&gt;/&lt;v&gt;.jar · &lt;v&gt;.json
│   ├── libraries/&lt;group&gt;/&lt;artifact&gt;/&lt;ver&gt;/
│   ├── assets/indexes/ · objects/&lt;hash[:2]&gt;/
│   ├── saves/  mods/  resourcepacks/  shaderpacks/
│   └── config/  logs/  options.txt
├── accounts.json
└── jres/&lt;jre-id&gt;/</pre>
  </div>

  <!-- 关于 -->
  <div class="card panel rise" style="--i:10">
    <div class="card-title-row">
      <Icon name="check" />
      <h3>关于</h3>
    </div>
    <div class="kv"><span class="kv-key">应用</span><span class="kv-val">{envState.meta?.name ?? "Quartz Launcher"}</span></div>
    <div class="kv"><span class="kv-key">版本</span><span class="kv-val">v{envState.meta?.version ?? "-"}</span></div>
    <div class="kv"><span class="kv-key">平台</span><span class="kv-val">{envState.meta ? `${envState.meta.os} · ${envState.meta.arch}` : "-"}</span></div>
    <div class="kv"><span class="kv-key">下载源</span><span class="kv-val">piston-meta.mojang.com（官方）</span></div>
    <div class="kv"><span class="kv-key">开源许可</span><span class="kv-val">GPL-3.0</span></div>
  </div>
</section>
