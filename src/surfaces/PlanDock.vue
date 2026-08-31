<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { canAcceptTextDrop, droppedPlainText } from "../dropText";
import {
  AlertCircle,
  CalendarClock,
  Check,
  Droplet,
  ExternalLink,
  Link2,
  LoaderCircle,
  NotebookText,
  PanelRightClose,
} from "lucide-vue-next";
import {
  listPlans,
  openExternalLink,
  openMainWindow,
  prepareDragCapture,
  resolvePlanTime,
  setPlanDone,
  togglePlanDock,
  type PlanItem,
} from "../api";

const DOCK_OPACITY_KEY = "feednote.plan-dock.opacity";

const expanded = ref(false);
const plans = ref<PlanItem[]>([]);
const loading = ref(false);
const error = ref("");
const answers = reactive<Record<string, string>>({});
const dockOpacity = ref(loadDockOpacity());
const textDropActive = ref(false);
const dropMessage = ref("");
const dockStyle = computed(() => ({
  "--dock-opacity": String(dockOpacity.value / 100),
}));
let unlisten: UnlistenFn | undefined;
let collapsedPointer: { id: number; x: number; y: number } | undefined;
let dragDepth = 0;
let dropMessageTimer: number | undefined;

function loadDockOpacity(): number {
  const stored = Number.parseInt(localStorage.getItem(DOCK_OPACITY_KEY) ?? "", 10);
  return Number.isFinite(stored) ? Math.min(100, Math.max(45, stored)) : 91;
}

function saveDockOpacity(): void {
  localStorage.setItem(DOCK_OPACITY_KEY, String(dockOpacity.value));
}

onMounted(async () => {
  await refresh();
  unlisten = await listen("plans-changed", refresh);
});

onBeforeUnmount(() => {
  unlisten?.();
  if (dropMessageTimer) window.clearTimeout(dropMessageTimer);
});

function enterTextDrop(event: DragEvent): void {
  event.preventDefault();
  dragDepth += 1;
  textDropActive.value = canAcceptTextDrop(event.dataTransfer);
  if (event.dataTransfer) event.dataTransfer.dropEffect = textDropActive.value ? "copy" : "none";
}

function overTextDrop(event: DragEvent): void {
  event.preventDefault();
  textDropActive.value = canAcceptTextDrop(event.dataTransfer);
  if (event.dataTransfer) event.dataTransfer.dropEffect = textDropActive.value ? "copy" : "none";
}

function leaveTextDrop(): void {
  dragDepth = Math.max(0, dragDepth - 1);
  if (dragDepth === 0) textDropActive.value = false;
}

async function receiveTextDrop(event: DragEvent): Promise<void> {
  event.preventDefault();
  dragDepth = 0;
  textDropActive.value = false;
  if (!canAcceptTextDrop(event.dataTransfer)) {
    showDropMessage("只支持拖入文字");
    return;
  }
  const text = droppedPlainText(event.dataTransfer);
  if (!text) {
    showDropMessage("只支持拖入文字");
    return;
  }
  try {
    await prepareDragCapture(text);
  } catch (reason) {
    showDropMessage(String(reason));
  }
}

function showDropMessage(message: string): void {
  dropMessage.value = message;
  if (dropMessageTimer) window.clearTimeout(dropMessageTimer);
  dropMessageTimer = window.setTimeout(() => {
    dropMessage.value = "";
  }, 2400);
}

async function toggle(): Promise<void> {
  expanded.value = await togglePlanDock();
  if (expanded.value) await refresh();
}

async function refresh(): Promise<void> {
  plans.value = await listPlans();
}

async function complete(plan: PlanItem): Promise<void> {
  await setPlanDone(plan.id, true);
  await refresh();
}

async function startDockDrag(event: MouseEvent): Promise<void> {
  if (event.button !== 0 || (event.target as HTMLElement).closest("button, input")) return;
  await getCurrentWindow().startDragging();
}

function prepareCollapsedDockDrag(event: PointerEvent): void {
  if (event.button !== 0) return;
  collapsedPointer = { id: event.pointerId, x: event.screenX, y: event.screenY };
  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
}

async function continueCollapsedDockDrag(event: PointerEvent): Promise<void> {
  if (!collapsedPointer || event.pointerId !== collapsedPointer.id) return;
  const distance = Math.hypot(
    event.screenX - collapsedPointer.x,
    event.screenY - collapsedPointer.y,
  );
  if (distance < 6) return;

  collapsedPointer = undefined;
  const target = event.currentTarget as HTMLElement;
  if (target.hasPointerCapture(event.pointerId)) target.releasePointerCapture(event.pointerId);
  await getCurrentWindow().startDragging();
}

async function finishCollapsedDockPointer(event: PointerEvent): Promise<void> {
  if (collapsedPointer?.id !== event.pointerId) return;
  collapsedPointer = undefined;
  const target = event.currentTarget as HTMLElement;
  if (target.hasPointerCapture(event.pointerId)) target.releasePointerCapture(event.pointerId);
  await toggle();
}

function cancelCollapsedDockPointer(event: PointerEvent): void {
  if (collapsedPointer?.id === event.pointerId) collapsedPointer = undefined;
}

async function openLink(plan: PlanItem): Promise<void> {
  const url = planLink(plan);
  if (!url) return;
  try {
    await openExternalLink(url);
  } catch (reason) {
    error.value = String(reason);
  }
}

function planLink(plan: PlanItem): string | undefined {
  const extracted = plan.details.match(/https?:\/\/[^\s<>"']+/i)?.[0];
  const candidate = (plan.linkUrl || extracted)?.replace(/[),.;，。；）]+$/, "");
  if (!candidate) return undefined;
  try {
    const url = new URL(candidate);
    return ["http:", "https:"].includes(url.protocol) ? url.toString() : undefined;
  } catch {
    return undefined;
  }
}

function linkLabel(plan: PlanItem): string {
  const url = planLink(plan);
  if (!url) return "";
  const parsed = new URL(url);
  const label = `${parsed.hostname}${parsed.pathname === "/" ? "" : parsed.pathname}`;
  return label.length > 42 ? `${label.slice(0, 39)}...` : label;
}

function compactContent(plan: PlanItem): string {
  if (plan.content?.trim()) return plan.content.trim();
  const knownType = plan.details.match(/AI\s*面|笔试|面试|电话面|视频面|会议|答辩|考试|复试/i)?.[0];
  if (knownType) return knownType.replace(/\s+/g, "");
  const sentence = plan.details.split(/[。！？!\n]/)[0].trim();
  return sentence.length > 36 ? `${sentence.slice(0, 33)}...` : sentence;
}

async function answer(plan: PlanItem): Promise<void> {
  const value = answers[plan.id]?.trim();
  if (!value || loading.value) return;
  loading.value = true;
  error.value = "";
  try {
    await resolvePlanTime(plan.id, value);
    answers[plan.id] = "";
    await refresh();
  } catch (reason) {
    error.value = String(reason);
  } finally {
    loading.value = false;
  }
}

function formatTime(timestamp?: number): string {
  if (!timestamp) return "等待安排时间";
  return new Intl.DateTimeFormat("zh-CN", {
    month: "numeric",
    day: "numeric",
    weekday: "short",
    hour: "2-digit",
    minute: "2-digit",
  }).format(timestamp);
}
</script>

<template>
  <button
    v-if="!expanded"
    class="dock-tab"
    :class="{ 'text-drop-active': textDropActive }"
    :title="dropMessage || 'FeedNote'"
    type="button"
    aria-label="展开计划"
    @pointerdown="prepareCollapsedDockDrag"
    @pointermove="continueCollapsedDockDrag"
    @pointerup="finishCollapsedDockPointer"
    @pointercancel="cancelCollapsedDockPointer"
    @keydown.enter.prevent="toggle"
    @keydown.space.prevent="toggle"
    @dragenter="enterTextDrop"
    @dragover="overTextDrop"
    @dragleave="leaveTextDrop"
    @drop="receiveTextDrop"
  >
    <span class="dock-logo"><CalendarClock :size="20" /></span>
    <span v-if="plans.length" class="plan-count">{{ Math.min(plans.length, 99) }}</span>
  </button>

  <aside
    v-else
    class="plan-dock-panel"
    :class="{ 'text-drop-active': textDropActive }"
    aria-label="桌面计划"
    :style="dockStyle"
    @dragenter="enterTextDrop"
    @dragover="overTextDrop"
    @dragleave="leaveTextDrop"
    @drop="receiveTextDrop"
  >
    <header @mousedown="startDockDrag">
      <div class="dock-heading">
        <span class="dock-heading-icon"><CalendarClock :size="17" /></span>
        <div class="dock-heading-content">
          <div class="dock-title-row">
            <span>桌面计划</span>
            <strong>{{ plans.length }}</strong>
          </div>
          <label class="dock-opacity-control" title="调节面板透明度" @mousedown.stop>
            <Droplet :size="11" aria-hidden="true" />
            <input
              v-model.number="dockOpacity"
              type="range"
              min="45"
              max="100"
              step="1"
              aria-label="面板透明度"
              @input="saveDockOpacity"
            />
            <output>{{ dockOpacity }}%</output>
          </label>
        </div>
      </div>
      <nav>
        <button type="button" aria-label="打开记忆库" @click="openMainWindow">
          <NotebookText :size="17" />
        </button>
        <button type="button" aria-label="收起" @click="toggle">
          <PanelRightClose :size="18" />
        </button>
      </nav>
    </header>

    <div class="plan-list">
      <div v-if="!plans.length" class="empty-plans">
        <span><CalendarClock :size="25" /></span>
        <p>暂无待办计划</p>
      </div>
      <article v-for="plan in plans" :key="plan.id" class="plan-card">
        <button
          class="complete-plan"
          type="button"
          title="标记完成"
          aria-label="标记完成"
          @click="complete(plan)"
        >
          <Check :size="14" />
        </button>
        <div class="plan-card-body">
          <time :class="{ pending: !plan.scheduledAt }">{{ formatTime(plan.scheduledAt) }}</time>
          <h2>{{ plan.title }}</h2>
          <div class="plan-fields">
            <div class="plan-field">
              <span>内容</span>
              <p>{{ compactContent(plan) }}</p>
            </div>
            <div v-if="planLink(plan)" class="plan-field">
              <span>链接</span>
              <button class="plan-link" type="button" @click="openLink(plan)">
                <Link2 :size="12" />
                <span>{{ linkLabel(plan) }}</span>
                <ExternalLink :size="12" />
              </button>
            </div>
            <div v-if="plan.notes" class="plan-field plan-notes">
              <span>注意</span>
              <p><AlertCircle :size="12" />{{ plan.notes }}</p>
            </div>
          </div>
          <form
            v-if="plan.status === 'needs_clarification'"
            class="dock-time-answer"
            @submit.prevent="answer(plan)"
          >
            <label :for="`answer-${plan.id}`">{{ plan.clarificationQuestion }}</label>
            <div>
              <input
                :id="`answer-${plan.id}`"
                v-model="answers[plan.id]"
                maxlength="500"
                placeholder="补充具体日期和时间"
              />
              <button type="submit" title="确认时间" aria-label="确认时间" :disabled="loading">
                <LoaderCircle v-if="loading" class="spin" :size="15" />
                <Check v-else :size="15" />
              </button>
            </div>
          </form>
        </div>
      </article>
    </div>
    <p v-if="dropMessage || error" class="dock-error">{{ dropMessage || error }}</p>
  </aside>
</template>
