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
let unlisten: UnlistenFn | undefined;

onMounted(async () => {
  snapshot.value = await getCapturePreview();
  unlisten = await listen<SelectionSnapshot>("capture-prepared", (event) => {
    snapshot.value = event.payload;
    plan.value = null;
    answer.value = "";
    error.value = "";
  });
});

onBeforeUnmount(() => unlisten?.());

async function reject(): Promise<void> {
  if (busy.value) return;
  await discardCapture();
  reset();
}

async function feed(): Promise<void> {
  if (busy.value || !snapshot.value) return;
  busy.value = true;
  error.value = "";
  try {
    const result = await commitCapture();
    plan.value = result.plan;
    if (!result.needsClarification) reset();
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
    plan.value = result.plan;
    answer.value = "";
    if (!result.needsClarification) reset();
  } catch (reason) {
    error.value = String(reason);
  } finally {
    busy.value = false;
  }
}

function reset(): void {
  snapshot.value = null;
  plan.value = null;
  answer.value = "";
  error.value = "";
}
</script>

<template>
  <section class="capture-popover" aria-label="选区投喂">
    <header>
      <div class="surface-mark"><CalendarClock :size="17" /></div>
      <strong>{{ plan ? "安排时间" : "投喂选区" }}</strong>
      <button class="icon-button" type="button" title="关闭" aria-label="关闭" @click="reject">
        <X :size="17" />
      </button>
    </header>

    <template v-if="plan?.status === 'needs_clarification'">
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
