<script lang="ts">
  import { THEMES, setTheme, themeState, toggleMotion } from "../theme.svelte";
  import { toast } from "../store.svelte";
  import Icon from "./Icon.svelte";

  /* 图鉴卡片：悬停只高亮，点击才应用全站主题（对齐 Style Atlas 交互约定） */
  const tempos = [
    { name: "fast", desc: "按钮、收藏、开关", time: "90–150ms" },
    { name: "nav", desc: "导航与选择", time: "120–220ms" },
    { name: "surface", desc: "容器展开、弹窗", time: "160–300ms" },
    { name: "status", desc: "成功反馈、通知", time: "100–200ms" },
  ];
</script>

<div class="tp-grid">
  {#each THEMES as t (t.id)}
    <button
      class="tp-card"
      class:active={themeState.id === t.id}
      onclick={() => setTheme(t.id)}
      aria-pressed={themeState.id === t.id}
    >
      {#if themeState.id === t.id}
        <span class="badge badge-ok">使用中</span>
      {/if}
      <!-- 预览小窗：scoped data-theme，主题 Token 在子树内生效 -->
      <div class="tp-preview" data-theme={t.id}>
        <div class="tp-bar"><i></i><i></i><i></i></div>
        <div class="tp-title">Quartz</div>
        <div class="tp-line"></div>
        <div class="tp-line short"></div>
        <span class="tp-btn">启动</span>
      </div>
      <div class="tp-info">
        <span class="tp-name">{t.name}<span class="tp-en">{t.en}</span></span>
        <span class="tp-tagline">{t.tagline}</span>
        <span class="tp-tags">
          {#each t.tags as tag (tag)}
            <span class="tp-tag">{tag}</span>
          {/each}
        </span>
      </div>
    </button>
  {/each}
</div>

<!-- 动效实验室：四档节奏 + 总开关 + 即时体验 -->
<div class="motion-row">
  <button
    class="motion-switch"
    class:on={themeState.motion}
    onclick={toggleMotion}
    aria-pressed={themeState.motion}
  >
    <span class="knob"></span>
    <span>{themeState.motion ? "动效已启用" : "动效已减少"}</span>
  </button>
  <button class="btn btn-sm" onclick={() => toast("即时反馈 · ease-out", "ok")}>
    <Icon name="play" />
    体验反馈动效
  </button>
</div>
<div class="motion-tempo">
  {#each tempos as tp (tp.name)}
    <span class="mt-name">{tp.name}</span>
    <span class="mt-desc">{tp.desc}</span>
    <span class="mt-time">{tp.time}</span>
  {/each}
</div>
<p class="motion-note">
  动效只解释行为，不做装饰：仅动 transform / opacity，短路径更快、容器更大更慢；
  系统级 prefers-reduced-motion 与上面的开关相互独立、任一生效即全关。
</p>
