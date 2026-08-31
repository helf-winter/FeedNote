<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { CalendarClock, Check, LockKeyhole, LoaderCircle, Undo2, X } from "lucide-vue-next";
import {
  commitCapture,
  discardCapture,
  getCapturePreview,
  getVaultStatus,
  initializeVault,
  resolvePlanTime,
  stashCapture,
  undoSecretStash,
  unlockVault,
  type PlanItem,
  type SelectionSnapshot,
  type VaultStatus,
} from "../api";

const snapshot = ref<SelectionSnapshot | null>(null);
const plan = ref<PlanItem | null>(null);
const answer = ref("");
const busy = ref(false);
const error = ref("");
const resultMessage = ref("");
const vaultStatus = ref<VaultStatus>({ initialized: false, unlocked: false, secretCount: 0 });
const vaultAuthVisible = ref(false);
const masterPassword = ref("");
const confirmPassword = ref("");
const stashedSecretId = ref("");
const activeAction = ref<"feed" | "stash" | "">("");
let unlisten: UnlistenFn | undefined;
let closeTimer: number | undefined;

onMounted(async () => {
  snapshot.value = await getCapturePreview();
  vaultStatus.value = await getVaultStatus();
  unlisten = await listen<SelectionSnapshot>("capture-prepared", (event) => {
    snapshot.value = event.payload;
    plan.value = null;
    answer.value = "";
    error.value = "";
    resultMessage.value = "";
    vaultAuthVisible.value = false;
    masterPassword.value = "";
    confirmPassword.value = "";
    stashedSecretId.value = "";
  });
});

onBeforeUnmount(() => {
  unlisten?.();
  if (closeTimer) window.clearTimeout(closeTimer);
});

async function reject(): Promise<void> {
  if (busy.value) return;
  clearCloseTimer();
  await discardCapture();
  reset();
}

async function feed(): Promise<void> {
  if (busy.value || !snapshot.value) return;
  busy.value = true;
  activeAction.value = "feed";
  error.value = "";
  try {
    const result = await commitCapture();
    plan.value = result.plan ?? null;
    if (!result.needsClarification) {
      resultMessage.value = result.message;
      closeTimer = window.setTimeout(async () => {
        await discardCapture();
        reset();
      }, 1400);
    }
  } catch (reason) {
    error.value = String(reason);
  } finally {
    busy.value = false;
    activeAction.value = "";
  }
}

async function stash(): Promise<void> {
  if (busy.value || !snapshot.value) return;
  if (!vaultStatus.value.unlocked) {
    vaultAuthVisible.value = true;
    return;
  }
  await stashNow();
}

async function authorizeVaultAndStash(): Promise<void> {
  if (!masterPassword.value || busy.value) return;
  if (!vaultStatus.value.initialized && masterPassword.value !== confirmPassword.value) {
    error.value = "两次主密码不一致";
    return;
  }
  busy.value = true;
  activeAction.value = "stash";
  error.value = "";
  try {
    vaultStatus.value = vaultStatus.value.initialized
      ? await unlockVault(masterPassword.value)
      : await initializeVault(masterPassword.value);
    masterPassword.value = "";
    confirmPassword.value = "";
    vaultAuthVisible.value = false;
  } catch (reason) {
    error.value = String(reason);
    busy.value = false;
    activeAction.value = "";
    return;
  }
  busy.value = false;
  activeAction.value = "";
  await stashNow();
}

async function stashNow(): Promise<void> {
  busy.value = true;
  activeAction.value = "stash";
  error.value = "";
  try {
    const result = await stashCapture();
    stashedSecretId.value = result.secretId;
    resultMessage.value = result.message;
    snapshot.value = null;
    closeTimer = window.setTimeout(async () => {
      await discardCapture();
      reset();
    }, 7600);
  } catch (reason) {
    error.value = String(reason);
  } finally {
    busy.value = false;
    activeAction.value = "";
  }
}

async function undoStash(): Promise<void> {
  if (!stashedSecretId.value || busy.value) return;
  busy.value = true;
  try {
    await undoSecretStash(stashedSecretId.value);
    resultMessage.value = "已撤销";
    stashedSecretId.value = "";
    closeTimer = window.setTimeout(async () => {
      await discardCapture();
      reset();
    }, 800);
  } catch (reason) {
    error.value = String(reason);
  } finally {
    busy.value = false;
  }
}

async function submitTime(): Promise<void> {
  if (busy.value || !plan.value || !answer.value.trim()) return;
  busy.value = true;
  error.value = "";
  try {
    const result = await resolvePlanTime(plan.value.id, answer.value.trim());
    plan.value = result.plan ?? null;
    answer.value = "";
    if (!result.needsClarification) {
      resultMessage.value = result.message;
      closeTimer = window.setTimeout(reset, 1000);
    }
  } catch (reason) {
    error.value = String(reason);
  } finally {
    busy.value = false;
  }
}

function reset(): void {
  clearCloseTimer();
  snapshot.value = null;
  plan.value = null;
  answer.value = "";
  error.value = "";
  resultMessage.value = "";
  vaultAuthVisible.value = false;
  masterPassword.value = "";
  confirmPassword.value = "";
  stashedSecretId.value = "";
}

function clearCloseTimer(): void {
  if (closeTimer !== undefined) {
    window.clearTimeout(closeTimer);
    closeTimer = undefined;
  }
}
</script>

<template>
  <section class="capture-popover" aria-label="处理选区">
    <header>
      <div class="surface-mark"><CalendarClock :size="17" /></div>
      <strong>{{ resultMessage ? "处理完成" : plan ? "安排时间" : "处理选区" }}</strong>
      <button class="icon-button" type="button" title="关闭" aria-label="关闭" @click="reject">
        <X :size="17" />
      </button>
    </header>

    <div v-if="resultMessage" class="capture-success">
      <span><Check :size="20" /></span>
      <strong>{{ resultMessage }}</strong>
      <button v-if="stashedSecretId" class="undo-stash" type="button" :disabled="busy" @click="undoStash">
        <Undo2 :size="13" />撤销
      </button>
    </div>

    <template v-else-if="plan?.status === 'needs_clarification'">
      <p class="question">{{ plan.clarificationQuestion }}</p>
      <form class="time-answer" @submit.prevent="submitTime">
        <input v-model="answer" maxlength="500" autofocus placeholder="例如：明天下午 3 点" />
        <button type="submit" title="确认时间" aria-label="确认时间" :disabled="busy || !answer.trim()">
          <LoaderCircle v-if="busy" class="spin" :size="16" />
          <Check v-else :size="16" />
        </button>
      </form>
    </template>

    <form v-else-if="vaultAuthVisible" class="vault-auth" @submit.prevent="authorizeVaultAndStash">
      <p>{{ vaultStatus.initialized ? "输入主密码以解锁秘密备忘录" : "设置独立主密码，丢失后无法从本机恢复" }}</p>
      <input v-model="masterPassword" type="password" minlength="6" maxlength="256" autofocus autocomplete="off" placeholder="主密码（至少 6 个字符）" />
      <input v-if="!vaultStatus.initialized" v-model="confirmPassword" type="password" minlength="6" maxlength="256" autocomplete="off" placeholder="再次输入主密码" />
      <button class="feed-button" type="submit" :disabled="busy || masterPassword.length < 6">
        <LoaderCircle v-if="busy" class="spin" :size="15" />
        <span v-else>{{ vaultStatus.initialized ? "解锁并藏入" : "创建并藏入" }}</span>
      </button>
    </form>

    <template v-else>
      <blockquote>{{ snapshot?.selectedText || "正在读取选区..." }}</blockquote>
      <div class="feed-actions">
        <button class="stash-button" type="button" :disabled="busy || !snapshot" @click="stash">
          <LoaderCircle v-if="busy && activeAction === 'stash'" class="spin" :size="15" />
          <template v-else><LockKeyhole :size="14" /><span>藏</span></template>
        </button>
        <button class="feed-button" type="button" :disabled="busy || !snapshot" @click="feed">
          <LoaderCircle v-if="busy && activeAction === 'feed'" class="spin" :size="15" />
          <span v-else>喂</span>
        </button>
      </div>
    </template>
    <p v-if="error" class="surface-error">{{ error }}</p>
  </section>
</template>
