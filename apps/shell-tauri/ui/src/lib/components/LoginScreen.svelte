<script lang="ts">
  import { fade, fly } from "svelte/transition";
  import Icon from "./Icon.svelte";
  import { doLogin, loginState, permBanners, requestPermission } from "../store.svelte";

  let username = $state("");

  async function submit() {
    await doLogin(username.trim());
  }
</script>

<section class="screen login-screen">
  <div class="login-card glass glass-accent" in:fly={{ y: 26, duration: 420 }}>
    <div class="login-brand">
      <div class="logo rise" style="--i:0">Q</div>
      <h1 class="login-title rise" style="--i:1">Quartz Launcher</h1>
      <p class="login-sub rise" style="--i:2">Rust 版 Minecraft Java 启动器</p>
    </div>
    <div class="login-form rise" style="--i:3">
      <label class="field-label" for="username">离线用户名</label>
      <input
        id="username"
        maxlength="16"
        placeholder="3-16 位字母 / 数字 / 下划线"
        autocomplete="off"
        bind:value={username}
        onkeydown={(e) => e.key === "Enter" && submit()}
      />
      <button
        id="login-btn"
        class="btn btn-primary btn-block"
        disabled={loginState.pending}
        onclick={submit}
      >
        <Icon name="arrow" />
        进入启动器
      </button>
      {#if loginState.error}
        <div class="alert alert-error" transition:fade>{loginState.error}</div>
      {/if}
      {#if permBanners.login}
        <div class="alert alert-warn" transition:fade>
          未授予完整文件权限：游戏文件将保存在应用私有目录（卸载会丢失）。
          <button class="btn btn-sm btn-primary" style="margin-top:8px" onclick={requestPermission}>
            立即授权
          </button>
        </div>
      {/if}
      <p class="login-note">账号仅写入本地 accounts.json，不连接微软服务，登录后可随时在「账户」页添加或切换。</p>
    </div>
  </div>
</section>
