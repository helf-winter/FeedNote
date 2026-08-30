<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { CalendarClock, Check, LoaderCircle, X } from "lucide-vue-next";
import {
  commitCapture,
  discardCapture,
  getCapturePreview,
  resolvePlanTime,
  type PlanItem,
  type SelectionSnapshot,
} from "../api";

const snapshot = ref<SelectionSnapshot | null>(null);
const plan = ref<PlanItem | null>(null);
const answer = ref("");
const busy = ref(false);
const error = ref("");
const resultMessage = ref("");
let unlisten: UnlistenFn | undefined;
let closeTimer: number | undefined;

onMounted(async () => {
  snapshot.value = await getCapturePreview();
  unlisten = await listen<SelectionSnapshot>("capture-prepared", (event) => {
    snapshot.value = event.payload;
    plan.value = null;
    answer.value = "";
    error.value = "";
    resultMessage.value = "";
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
}

function clearCloseTimer(): void {
  if (closeTimer !== undefined) {
    window.clearTimeout(closeTimer);
    closeTimer = undefined;
  }
}
</script>

<template>
  <section class="capture-popover" aria-label="选区投喂">
    <header>
      <div class="surface-mark"><CalendarClock :size="17" /></div>
      <strong>{{ resultMessage ? "处理完成" : plan ? "安排时间" : "投喂选区" }}</strong>
      <button class="icon-button" type="button" title="关闭" aria-label="关闭" @click="reject">
        <X :size="17" />
      </button>
    </header>

    <div v-if="resultMessage" class="capture-success">
      <span><Check :size="20" /></span>
      <strong>{{ resultMessage }}</strong>
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

    <template v-else>
      <blockquote>{{ snapshot?.selectedText || "正在读取选区..." }}</blockquote>
      <div class="feed-actions">
        <button class="reject-button" type="button" :disabled="busy" @click="reject">不喂</button>
        <button class="feed-button" type="button" :disabled="busy || !snapshot" @click="feed">
          <LoaderCircle v-if="busy" class="spin" :size="15" />
          <span v-else>喂</span>
        </button>
      </div>
    </template>
    <p v-if="error" class="surface-error">{{ error }}</p>
  </section>
</template>
