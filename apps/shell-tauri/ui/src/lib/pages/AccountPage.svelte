<script lang="ts">
  import { fade } from "svelte/transition";
  import Icon from "../components/Icon.svelte";
  import {
    accountsState,
    addAccount,
    authlibForm,
    authlibLogin,
    downloadAuthlibAgent,
    refreshAuthlib,
    removeAccount,
    startMsLogin,
    switchAccount,
    settingsState,
  } from "../store.svelte";
  import { avatarLetter, avatarStyle } from "../format";

  let newName = $state("");
  let adding = $state(false);

  let authlibServer = $state("");
  let authlibUser = $state("");
  let authlibPass = $state("");

  const confirmArmed = $state<{ uuid: string | null }>({ uuid: null });
  let armTimer: number | undefined;

  async function submitAdd() {
    if (!newName.trim()) {
      accountsState.error = "请输入用户名";
      return;
    }
    adding = true;
    const ok = await addAccount(newName.trim());
    if (ok) newName = "";
    adding = false;
  }

  function armDelete(uuid: string, username: string, btn: HTMLButtonElement) {
    if (confirmArmed.uuid === uuid) {
      void removeAccount(uuid, username);
      confirmArmed.uuid = null;
      return;
    }
    confirmArmed.uuid = uuid;
    btn.textContent = "确认删除？";
    window.clearTimeout(armTimer);
    armTimer = window.setTimeout(() => {
      confirmArmed.uuid = null;
      btn.textContent = "删除";
    }, 3000);
  }

  async function submitAuthlib() {
    await authlibLogin(authlibServer.trim(), authlibUser.trim(), authlibPass);
  }
</script>

<section class="page">
  <div class="page-head rise" style="--i:0">
    <h2>账户</h2>
    <p class="page-sub">离线 / 微软 / authlib 外置登录，登录状态与下载完全解耦</p>
  </div>

  <div class="card glass rise" style="--i:1">
    <div class="card-title-row">
      <Icon name="user" />
      <h3>添加离线账号</h3>
    </div>
    <div class="add-row">
      <input
        maxlength="16"
        placeholder="输入用户名（3-16 位字母 / 数字 / 下划线）"
        autocomplete="off"
        bind:value={newName}
        onkeydown={(e) => e.key === "Enter" && submitAdd()}
      />
      <button class="btn btn-primary" disabled={adding} onclick={submitAdd}>添加</button>
    </div>
    {#if accountsState.error}
      <div class="alert alert-error" transition:fade>{accountsState.error}</div>
    {/if}
  </div>

  <div class="card glass rise" style="--i:2">
    <div class="card-title-row">
      <Icon name="cube" />
      <h3>微软账号（设备码登录）</h3>
    </div>
    <p class="page-sub">需先在设置页配置项目自有 Azure client_id；登录后自动写入会话</p>
    <button class="btn btn-primary" style="margin-top:10px" onclick={startMsLogin}>开始微软登录</button>
    {#if settingsState.msPrompt}
      <div class="alert alert-warn" transition:fade>{settingsState.msPrompt}</div>
    {/if}
    {#if settingsState.msResult}
      <div class="alert alert-{settingsState.msResult.ok ? 'ok' : 'error'}" transition:fade>
        {settingsState.msResult.text}
      </div>
    {/if}
  </div>

  <div class="card glass rise" style="--i:3">
    <div class="card-title-row">
      <Icon name="shield" />
      <h3>外置登录（authlib-injector）</h3>
    </div>
    <p class="page-sub">Yggdrasil 协议；agent jar 首次使用前需下载一次</p>
    <div class="kv">
      <span class="kv-key">Agent</span>
      <span class="kv-val">{settingsState.authlibState}</span>
      <button class="btn btn-sm btn-primary" onclick={downloadAuthlibAgent}>下载 agent</button>
    </div>
    <div class="add-row" style="margin-top:10px">
      <input
        placeholder="认证服务器地址，如 https://littleskin.cn/api/yggdrasil"
        autocomplete="off"
        bind:value={authlibServer}
      />
    </div>
    <div class="add-row" style="margin-top:10px">
      <input placeholder="邮箱或用户名" autocomplete="off" bind:value={authlibUser} />
      <input type="password" placeholder="密码" autocomplete="off" bind:value={authlibPass} />
      <button class="btn btn-primary" onclick={submitAuthlib}>登录</button>
    </div>
    {#if authlibForm.error}
      <div class="alert alert-error" transition:fade>{authlibForm.error}</div>
    {/if}
  </div>

  <div class="account-list rise" style="--i:4">
    {#if accountsState.list.length === 0}
      <div class="list-note glass">还没有账号，先添加一个离线账号</div>
    {:else}
      {#each accountsState.list as a (a.uuid)}
        <div class="card glass account-card" class:selected={a.selected}>
          <span class="avatar" style={avatarStyle(a.uuid)}>{avatarLetter(a.username)}</span>
          <div class="account-info">
            <div class="account-name">
              {a.username}
              {#if a.selected}<span class="badge badge-ok">当前</span>{/if}
            </div>
            <div class="account-uuid">{a.uuid}</div>
          </div>
          <div class="account-actions">
            {#if !a.selected}
              <button class="btn btn-sm" onclick={() => switchAccount(a.uuid, a.username)}>切换</button>
            {/if}
            <button class="btn btn-sm btn-danger" onclick={(e) => armDelete(a.uuid, a.username, e.currentTarget)}>
              删除
            </button>
          </div>
        </div>
      {/each}
    {/if}
  </div>
</section>
