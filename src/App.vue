<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from "vue";
import {
  Archive,
  BrainCircuit,
  Check,
  ChevronRight,
  CircleAlert,
  Clock3,
  Cloud,
  Database,
  Download,
  ExternalLink,
  FileText,
  Inbox,
  LoaderCircle,
  Menu,
  NotebookPen,
  Plus,
  RefreshCw,
  Search,
  Send,
  Settings,
  ShieldCheck,
  Smartphone,
  Sparkles,
  Table2,
  Trash2,
  X,
} from "lucide-vue-next";
import {
  checkAi,
  createFeed,
  deleteFeed,
  exportArchive,
  getFeishuSourceStatus,
  getFeishuSyncStatus,
  getMemory,
  getSettings,
  getStats,
  isTauri,
  listFeeds,
  listMemories,
  listReviews,
  processFeed,
  requestDeleteFeed,
  resolveReview,
  openExternalLink,
  syncFeishuNow,
  syncFeishuSourceNow,
  testMobilePush,
  updateSettings,
  type AppSettings,
  type FeedEvent,
  type FeishuSourceStatus,
  type FeishuSyncStatus,
  type MemoryDetail,
  type MemorySummary,
  type Page,
  type ReviewItem,
  type Stats,
} from "./api";

const activePage = ref<Page>("inbox");
const sidebarOpen = ref(false);
const composer = ref("");
const composerElement = ref<HTMLTextAreaElement>();
const feedSearch = ref("");
const memorySearch = ref("");
const memoryType = ref("");
const feeds = ref<FeedEvent[]>([]);
const memories = ref<MemorySummary[]>([]);
const reviews = ref<ReviewItem[]>([]);
const stats = ref<Stats>({ totalFeeds: 0, totalMemories: 0, pendingReviews: 0, pendingProcessing: 0 });
const settings = ref<AppSettings>({
  aiEnabled: true,
  llmEndpoint: "https://open.bigmodel.cn/api/anthropic",
  llmModel: "glm-5.3",
  embeddingEndpoint: "https://open.bigmodel.cn/api/paas/v4",
  embeddingModel: "embedding-3",
  embeddingDimensions: 512,
  mobilePushEnabled: false,
  mobilePushProvider: "ntfy",
  mobileReminderMinutes: 15,
  feishuSyncEnabled: false,
  feishuTaskRemindersEnabled: false,
  feishuSourceEnabled: false,
  feishuSourceUrl: "",
});
const selectedMemory = ref<MemoryDetail>();
const deleteTarget = ref<FeedEvent>();
const saving = ref(false);
const loading = ref(true);
const processingIds = ref(new Set<string>());
const toast = ref<{ message: string; kind: "success" | "error" }>();
const aiCheckState = ref<"idle" | "checking" | "success" | "error">("idle");
const aiCheckMessage = ref("");
const pushCheckState = ref<"idle" | "checking" | "success" | "error">("idle");
const pushCheckMessage = ref("");
const feishuSyncState = ref<"idle" | "syncing" | "success" | "error">("idle");
const feishuSyncMessage = ref("");
const feishuStatus = ref<FeishuSyncStatus>({
  enabled: false,
  configured: false,
  pendingPlans: 0,
  taskRemindersEnabled: false,
  pendingTaskReminders: 0,
  lastError: undefined,
});
const feishuSourceState = ref<"idle" | "syncing" | "success" | "error">("idle");
const feishuSourceMessage = ref("");
const feishuSourceStatus = ref<FeishuSourceStatus>({
  enabled: false,
  configured: false,
  spreadsheetUrl: "",
  totalRows: 0,
  actionableRows: 0,
  trackedRows: 0,
  importedPlans: 0,
});

const pageTitles: Record<Page, { title: string; subtitle: string }> = {
  inbox: { title: "收集箱", subtitle: "原样保存每一次输入，再慢慢理解" },
  memories: { title: "记忆", subtitle: "当前理解，以及它从哪里来" },
  memo: { title: "备忘录", subtitle: "留给更长远的事情" },
  review: { title: "待澄清", subtitle: "只有无法可靠理解的内容才会停在这里" },
  settings: { title: "设置", subtitle: "模型、数据和隐私边界" },
};

const navItems = [
  { id: "inbox" as const, label: "收集箱", icon: Inbox },
  { id: "memories" as const, label: "记忆", icon: BrainCircuit },
  { id: "memo" as const, label: "备忘录", icon: NotebookPen },
  { id: "review" as const, label: "待澄清", icon: CircleAlert },
  { id: "settings" as const, label: "设置", icon: Settings },
];

const memoryTypes = [
  "Knowledge",
  "Project",
  "Decision",
  "Idea",
  "Task",
  "Preference",
  "Person",
  "Experience",
  "Unclassified",
];

const canSubmit = computed(() => composer.value.trim().length > 0 && !saving.value);

onMounted(async () => {
  await refreshAll();
  await refreshFeishuStatus();
  await refreshFeishuSourceStatus();
  window.addEventListener("keydown", (event) => {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
      event.preventDefault();
      activePage.value = "inbox";
      nextTick(() => composerElement.value?.focus());
    }
    if (event.key === "Escape") {
      selectedMemory.value = undefined;
      deleteTarget.value = undefined;
      sidebarOpen.value = false;
    }
  });
});

async function refreshAll(): Promise<void> {
  loading.value = true;
  try {
    const [nextFeeds, nextMemories, nextReviews, nextStats, nextSettings] = await Promise.all([
      listFeeds(feedSearch.value),
      listMemories(memorySearch.value, memoryType.value),
      listReviews(),
      getStats(),
      getSettings(),
    ]);
    feeds.value = nextFeeds;
    memories.value = nextMemories;
    reviews.value = nextReviews;
    stats.value = nextStats;
    settings.value = nextSettings;
  } catch (error) {
    notify(errorMessage(error), "error");
  } finally {
    loading.value = false;
  }
}

async function submitFeed(): Promise<void> {
  if (!canSubmit.value) return;
  saving.value = true;
  try {
    const content = composer.value;
    const result = await createFeed(content);
    composer.value = "";
    notify("已保存原始记录", "success");
    await refreshAll();
    if (settings.value.aiEnabled) void runProcessing(result.feedId, false);
  } catch (error) {
    notify(errorMessage(error), "error");
  } finally {
    saving.value = false;
    nextTick(() => composerElement.value?.focus());
  }
}

async function runProcessing(feedId: string, announce = true): Promise<void> {
  processingIds.value = new Set(processingIds.value).add(feedId);
  try {
    const result = await processFeed(feedId);
    if (announce || result.status === "review") notify(result.message, "success");
    await refreshAll();
  } catch (error) {
    notify(errorMessage(error), "error");
    await refreshAll();
  } finally {
    const next = new Set(processingIds.value);
    next.delete(feedId);
    processingIds.value = next;
  }
}

async function searchFeeds(): Promise<void> {
  feeds.value = await listFeeds(feedSearch.value);
}

async function searchMemories(): Promise<void> {
  memories.value = await listMemories(memorySearch.value, memoryType.value);
}

async function openMemory(memoryId?: string): Promise<void> {
  if (!memoryId) return;
  try {
    selectedMemory.value = await getMemory(memoryId);
  } catch (error) {
    notify(errorMessage(error), "error");
  }
}

async function decideReview(reviewId: string, accept: boolean): Promise<void> {
  try {
    await resolveReview(reviewId, accept);
    notify(accept ? "已按原文保留为未分类记忆" : "已关闭这条澄清", "success");
    await refreshAll();
  } catch (error) {
    notify(errorMessage(error), "error");
  }
}

async function confirmDelete(): Promise<void> {
  if (!deleteTarget.value) return;
  try {
    const confirmationToken = await requestDeleteFeed(deleteTarget.value.id);
    await deleteFeed(deleteTarget.value.id, confirmationToken);
    deleteTarget.value = undefined;
    notify("记录及其派生记忆已永久删除", "success");
    await refreshAll();
  } catch (error) {
    notify(errorMessage(error), "error");
  }
}

async function saveSettings(): Promise<void> {
  try {
    await updateSettings(settings.value);
    notify("设置已保存", "success");
  } catch (error) {
    notify(errorMessage(error), "error");
  }
}

async function testAi(): Promise<void> {
  aiCheckState.value = "checking";
  aiCheckMessage.value = "正在检查 LLM 与 Embedding...";
  try {
    aiCheckMessage.value = await checkAi();
    aiCheckState.value = "success";
  } catch (error) {
    aiCheckMessage.value = errorMessage(error);
    aiCheckState.value = "error";
  }
}

async function checkMobilePush(): Promise<void> {
  pushCheckState.value = "checking";
  pushCheckMessage.value = "正在发送测试提醒...";
  try {
    pushCheckMessage.value = await testMobilePush(settings.value.mobilePushProvider);
    pushCheckState.value = "success";
  } catch (error) {
    pushCheckMessage.value = errorMessage(error);
    pushCheckState.value = "error";
  }
}

async function refreshFeishuStatus(): Promise<void> {
  try {
    feishuStatus.value = await getFeishuSyncStatus();
  } catch {
    feishuStatus.value = {
      enabled: settings.value.feishuSyncEnabled,
      configured: false,
      pendingPlans: 0,
      taskRemindersEnabled: settings.value.feishuTaskRemindersEnabled,
      pendingTaskReminders: 0,
    };
  }
}

async function runFeishuSync(): Promise<void> {
  if (feishuSyncState.value === "syncing") return;
  feishuSyncState.value = "syncing";
  feishuSyncMessage.value = "正在同步飞书计划表和待办提醒...";
  try {
    await updateSettings(settings.value);
    feishuSyncMessage.value = await syncFeishuNow();
    feishuSyncState.value = "success";
    await refreshFeishuStatus();
  } catch (error) {
    feishuSyncMessage.value = errorMessage(error);
    feishuSyncState.value = "error";
    await refreshFeishuStatus();
  }
}

async function openFeishuSheet(): Promise<void> {
  if (!feishuStatus.value.spreadsheetUrl) return;
  await openExternalLink(feishuStatus.value.spreadsheetUrl);
}

async function refreshFeishuSourceStatus(): Promise<void> {
  try {
    feishuSourceStatus.value = await getFeishuSourceStatus();
  } catch {
    feishuSourceStatus.value = {
      enabled: settings.value.feishuSourceEnabled,
      configured: false,
      spreadsheetUrl: settings.value.feishuSourceUrl,
      totalRows: 0,
      actionableRows: 0,
      trackedRows: 0,
      importedPlans: 0,
    };
  }
}

async function runFeishuSourceSync(): Promise<void> {
  if (feishuSourceState.value === "syncing") return;
  feishuSourceState.value = "syncing";
  feishuSourceMessage.value = "正在检查投递记录表...";
  try {
    await updateSettings(settings.value);
    feishuSourceMessage.value = await syncFeishuSourceNow();
    feishuSourceState.value = "success";
    await Promise.all([refreshFeishuSourceStatus(), refreshFeishuStatus()]);
  } catch (error) {
    feishuSourceMessage.value = errorMessage(error);
    feishuSourceState.value = "error";
    await refreshFeishuSourceStatus();
  }
}

async function openFeishuSource(): Promise<void> {
  if (!settings.value.feishuSourceUrl) return;
  await openExternalLink(settings.value.feishuSourceUrl);
}

async function exportData(): Promise<void> {
  try {
    if (await exportArchive()) notify("数据已导出", "success");
  } catch (error) {
    notify(errorMessage(error), "error");
  }
}

function navigate(page: Page): void {
  activePage.value = page;
  sidebarOpen.value = false;
  if (page === "inbox") nextTick(() => composerElement.value?.focus());
}

function notify(message: string, kind: "success" | "error"): void {
  toast.value = { message, kind };
  window.setTimeout(() => {
    if (toast.value?.message === message) toast.value = undefined;
  }, 3200);
}

function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "操作失败，请稍后再试";
}

function formatTime(timestamp: number): string {
  const date = new Date(timestamp);
  const today = new Date();
  const sameDay = date.toDateString() === today.toDateString();
  return sameDay
    ? date.toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" })
    : date.toLocaleDateString("zh-CN", { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
}

function statusLabel(status: string): string {
  return (
    {
      pending: "待整理",
      processing: "整理中",
      review: "待澄清",
      classified: "已归类",
      ignored: "仅保留原文",
      accepted: "已确认",
      rejected: "保留原文",
    }[status] ?? status
  );
}

function typeLabel(type: string): string {
  return (
    {
      Knowledge: "知识",
      Project: "项目",
      Decision: "决定",
      Idea: "想法",
      Task: "待办",
      Preference: "偏好",
      Person: "人物",
      Experience: "经历",
      Unclassified: "未分类",
    }[type] ?? type
  );
}
</script>

<template>
  <div class="app-shell">
    <div v-if="sidebarOpen" class="mobile-scrim" @click="sidebarOpen = false" />
    <aside class="sidebar" :class="{ open: sidebarOpen }">
      <div class="brand">
        <span class="brand-mark"><FileText :size="20" /></span>
        <span class="brand-name">FeedNote</span>
      </div>

      <nav class="primary-nav" aria-label="主导航">
        <button
          v-for="item in navItems"
          :key="item.id"
          class="nav-item"
          :class="{ active: activePage === item.id }"
          type="button"
          @click="navigate(item.id)"
        >
          <component :is="item.icon" :size="18" />
          <span>{{ item.label }}</span>
          <span v-if="item.id === 'review' && stats.pendingReviews" class="nav-count">{{ stats.pendingReviews }}</span>
        </button>
      </nav>

      <div class="sidebar-footer">
        <div class="privacy-state">
          <ShieldCheck :size="16" />
          <div>
            <strong>数据本机保存</strong>
            <span>{{ settings.aiEnabled ? "智谱云端理解" : "AI 已暂停" }}</span>
          </div>
        </div>
        <span v-if="!isTauri" class="preview-label">浏览器预览</span>
      </div>
    </aside>

    <main class="main-content">
      <header class="topbar">
        <button class="icon-button mobile-menu" type="button" aria-label="打开菜单" title="打开菜单" @click="sidebarOpen = true">
          <Menu :size="20" />
        </button>
        <div>
          <h1>{{ pageTitles[activePage].title }}</h1>
          <p>{{ pageTitles[activePage].subtitle }}</p>
        </div>
        <button class="quick-capture" type="button" title="快速记录" @click="navigate('inbox')">
          <Plus :size="17" />
          <span>快速记录</span>
          <kbd>Ctrl K</kbd>
        </button>
      </header>

      <section v-if="activePage === 'inbox'" class="page inbox-page">
        <div class="composer-section">
          <div class="composer-heading">
            <div>
              <h2>今天想记下什么？</h2>
              <p>先原样保存，再由 AI 自动分类和整理，不覆盖你的原话。</p>
            </div>
            <span class="save-state"><Database :size="15" /> 本机保存</span>
          </div>
          <div class="composer-wrap" :class="{ focused: composer.length > 0 }">
            <textarea
              ref="composerElement"
              v-model="composer"
              rows="5"
              maxlength="100000"
              placeholder="输入、粘贴，或者先写下一句不完整的想法..."
              @keydown.ctrl.enter.prevent="submitFeed"
              @keydown.meta.enter.prevent="submitFeed"
            />
            <div class="composer-actions">
              <span>{{ composer.length ? `${composer.length} 字` : "Ctrl + Enter 保存" }}</span>
              <button class="primary-button" type="button" :disabled="!canSubmit" @click="submitFeed">
                <LoaderCircle v-if="saving" class="spin" :size="17" />
                <Plus v-else :size="17" />
                保存
              </button>
            </div>
          </div>
        </div>

        <div class="metric-strip" aria-label="记忆概览">
          <div><strong>{{ stats.totalFeeds }}</strong><span>原始记录</span></div>
          <div><strong>{{ stats.totalMemories }}</strong><span>活跃记忆</span></div>
          <div><strong>{{ stats.pendingReviews }}</strong><span>等待澄清</span></div>
          <div><strong>{{ stats.pendingProcessing }}</strong><span>等待整理</span></div>
        </div>

        <div class="section-toolbar">
          <h2>最近投喂</h2>
          <label class="search-control compact">
            <Search :size="16" />
            <input v-model="feedSearch" type="search" placeholder="搜索原始记录" @input="searchFeeds" />
          </label>
        </div>

        <div v-if="loading" class="empty-state"><LoaderCircle class="spin" :size="22" /><span>正在读取本地记忆</span></div>
        <div v-else-if="feeds.length === 0" class="empty-state">
          <Inbox :size="28" />
          <strong>收集箱还是空的</strong>
          <span>写下一件刚刚发生的事，或粘贴一段值得留下的内容。</span>
        </div>
        <div v-else class="feed-list">
          <article v-for="feed in feeds" :key="feed.id" class="feed-row">
            <div class="timeline-dot" :class="`status-${feed.processingStatus}`" />
            <div class="feed-main" @click="openMemory(feed.memoryId)">
              <p>{{ feed.rawContent }}</p>
              <div class="row-meta">
                <span><Clock3 :size="13" /> {{ formatTime(feed.createdAt) }}</span>
                <span class="status-pill" :class="`status-${feed.processingStatus}`">{{ statusLabel(feed.processingStatus) }}</span>
              </div>
            </div>
            <div class="row-actions">
              <button
                v-if="feed.processingStatus === 'pending' && settings.aiEnabled"
                class="icon-button"
                type="button"
                title="尝试 AI 整理"
                aria-label="尝试 AI 整理"
                :disabled="processingIds.has(feed.id)"
                @click="runProcessing(feed.id)"
              >
                <LoaderCircle v-if="processingIds.has(feed.id)" class="spin" :size="17" />
                <Sparkles v-else :size="17" />
              </button>
              <button class="icon-button danger" type="button" title="永久删除" aria-label="永久删除" @click="deleteTarget = feed">
                <Trash2 :size="17" />
              </button>
              <button class="icon-button" type="button" title="查看记忆" aria-label="查看记忆" @click="openMemory(feed.memoryId)">
                <ChevronRight :size="18" />
              </button>
            </div>
          </article>
        </div>
      </section>

      <section v-else-if="activePage === 'memories'" class="page">
        <div class="filters-bar">
          <label class="search-control">
            <Search :size="17" />
            <input v-model="memorySearch" type="search" placeholder="搜索标题、正文或摘要" @input="searchMemories" />
          </label>
          <select v-model="memoryType" aria-label="记忆类型" @change="searchMemories">
            <option value="">全部类型</option>
            <option v-for="type in memoryTypes" :key="type" :value="type">{{ typeLabel(type) }}</option>
          </select>
        </div>

        <div v-if="memories.length === 0" class="empty-state">
          <BrainCircuit :size="28" />
          <strong>没有匹配的记忆</strong>
          <span>记忆会从每一次原始投喂中产生，并保留来源。</span>
        </div>
        <div v-else class="memory-list">
          <button v-for="memory in memories" :key="memory.id" class="memory-row" type="button" @click="openMemory(memory.id)">
            <span class="type-mark" :data-type="memory.memoryType">{{ typeLabel(memory.memoryType).slice(0, 1) }}</span>
            <span class="memory-copy">
              <span class="memory-title-line">
                <strong>{{ memory.title }}</strong>
                <span class="type-label">{{ typeLabel(memory.memoryType) }}</span>
              </span>
              <span>{{ memory.summary || memory.body }}</span>
            </span>
            <span class="memory-source">{{ memory.sourceCount }} 个来源</span>
            <ChevronRight :size="18" />
          </button>
        </div>
      </section>

      <section v-else-if="activePage === 'memo'" class="page memo-page">
        <div class="empty-state">
          <NotebookPen :size="28" />
          <strong>还没有备忘内容</strong>
        </div>
      </section>

      <section v-else-if="activePage === 'review'" class="page review-page">
        <div class="boundary-banner">
          <ShieldCheck :size="21" />
          <div>
            <strong>普通分类由系统自动完成</strong>
            <span>这里只处理指代不明、信息缺失或重大冲突，原始投喂始终保留。</span>
          </div>
        </div>
        <div v-if="reviews.length === 0" class="empty-state">
          <Check :size="28" />
          <strong>没有需要澄清的内容</strong>
          <span>分类、标题和摘要会自动完成，不需要逐条审批。</span>
        </div>
        <div v-else class="review-list">
          <article v-for="review in reviews" :key="review.id" class="review-card">
            <div class="review-topline">
              <span class="risk-label" :class="`risk-${review.riskLevel}`">{{ review.riskLevel === "high" ? "需要澄清" : "存在冲突" }}</span>
              <span>{{ formatTime(review.createdAt) }}</span>
            </div>
            <template v-if="review.proposedAction === 'ask'">
              <h3>{{ review.payload.question || "这条内容指的是什么？" }}</h3>
              <p>{{ review.reason }}</p>
            </template>
            <template v-else>
              <div class="proposal-grid">
                <span>建议类型</span><strong>{{ typeLabel(review.payload.memoryType || "Unclassified") }}</strong>
                <span>建议标题</span><strong>{{ review.payload.title }}</strong>
                <span>理解摘要</span><strong>{{ review.payload.summary }}</strong>
              </div>
              <p class="proposal-reason">{{ review.reason }}</p>
            </template>
            <div class="review-actions">
              <button class="secondary-button" type="button" @click="decideReview(review.id, false)"><X :size="16" />关闭问题</button>
              <button class="primary-button" type="button" @click="decideReview(review.id, true)"><Check :size="16" />暂时保留未分类</button>
            </div>
          </article>
        </div>
      </section>

      <section v-else class="page settings-page">
        <div class="settings-section">
          <div class="settings-heading">
            <div><h2>自动理解</h2><p>保存后由 Memory Engine 自动分类、归并和摘要。</p></div>
            <label class="toggle-control">
              <input v-model="settings.aiEnabled" type="checkbox" />
              <span aria-hidden="true" />
              <strong>{{ settings.aiEnabled ? "已开启" : "已关闭" }}</strong>
            </label>
          </div>
          <div class="form-grid" :class="{ muted: !settings.aiEnabled }">
            <label><span>密钥文件</span><input value="data\secrets.env" type="text" readonly /></label>
            <label><span>LLM 地址</span><input v-model="settings.llmEndpoint" type="url" :disabled="!settings.aiEnabled" /></label>
            <label><span>LLM 模型</span><input v-model="settings.llmModel" type="text" :disabled="!settings.aiEnabled" /></label>
            <label><span>Embedding 地址</span><input v-model="settings.embeddingEndpoint" type="url" :disabled="!settings.aiEnabled" /></label>
            <label><span>Embedding 模型</span><input v-model="settings.embeddingModel" type="text" :disabled="!settings.aiEnabled" /></label>
            <label><span>向量维度</span><select v-model.number="settings.embeddingDimensions" :disabled="!settings.aiEnabled"><option :value="256">256</option><option :value="512">512</option><option :value="1024">1024</option><option :value="2048">2048</option></select></label>
          </div>
          <div class="settings-actions">
            <button class="secondary-button" type="button" :disabled="!settings.aiEnabled || aiCheckState === 'checking'" @click="testAi">
              <LoaderCircle v-if="aiCheckState === 'checking'" class="spin" :size="16" />
              <RefreshCw v-else :size="16" />测试连接
            </button>
            <button class="primary-button" type="button" @click="saveSettings"><Check :size="16" />保存设置</button>
          </div>
          <p v-if="aiCheckMessage" class="connection-result" :class="aiCheckState">{{ aiCheckMessage }}</p>
        </div>

        <div class="settings-section">
          <div class="settings-heading">
            <div><h2>手机提醒</h2><p>通过 ntfy 或自有 Webhook 接收计划提醒。</p></div>
            <label class="toggle-control">
              <input v-model="settings.mobilePushEnabled" type="checkbox" />
              <span aria-hidden="true" />
              <strong>{{ settings.mobilePushEnabled ? "已开启" : "已关闭" }}</strong>
            </label>
          </div>
          <div class="form-grid" :class="{ muted: !settings.mobilePushEnabled }">
            <label><span>推送配置</span><input value="data\secrets.env" type="text" readonly /></label>
            <label><span>推送通道</span><select v-model="settings.mobilePushProvider" :disabled="!settings.mobilePushEnabled"><option value="ntfy">ntfy 手机 App</option><option value="webhook">通用 Webhook</option></select></label>
            <label><span>提前提醒</span><select v-model.number="settings.mobileReminderMinutes" :disabled="!settings.mobilePushEnabled"><option :value="0">准时</option><option :value="5">提前 5 分钟</option><option :value="15">提前 15 分钟</option><option :value="30">提前 30 分钟</option><option :value="60">提前 1 小时</option></select></label>
          </div>
          <div class="settings-actions">
            <button class="secondary-button" type="button" :disabled="!settings.mobilePushEnabled || pushCheckState === 'checking'" @click="checkMobilePush">
              <LoaderCircle v-if="pushCheckState === 'checking'" class="spin" :size="16" />
              <Send v-else :size="16" />发送测试提醒
            </button>
            <button class="primary-button" type="button" @click="saveSettings"><Check :size="16" />保存设置</button>
          </div>
          <p v-if="pushCheckMessage" class="connection-result" :class="pushCheckState">{{ pushCheckMessage }}</p>
        </div>

        <div class="settings-section">
          <div class="settings-heading">
            <div><h2>飞书投递记录</h2><p>由许科AI助手维护求职记录，不从表格反向创建计划。</p></div>
            <label class="toggle-control">
              <input v-model="settings.feishuSourceEnabled" type="checkbox" />
              <span aria-hidden="true" />
              <strong>{{ settings.feishuSourceEnabled ? "已开启" : "已关闭" }}</strong>
            </label>
          </div>
          <div class="form-grid" :class="{ muted: !settings.feishuSourceEnabled }">
            <label class="wide-field"><span>目标表格链接</span><input v-model.trim="settings.feishuSourceUrl" type="url" :disabled="!settings.feishuSourceEnabled" placeholder="https://your-team.feishu.cn/sheets/..." /></label>
            <label><span>写入字段</span><input value="状态、公司/事项、岗位/方向、链接、备注" type="text" readonly /></label>
            <label><span>写入策略</span><input value="链接或公司+岗位去重，命中则更新" type="text" readonly /></label>
          </div>
          <div class="settings-actions">
            <button class="secondary-button" type="button" :disabled="!settings.feishuSourceEnabled || !settings.feishuSourceUrl || feishuSourceState === 'syncing'" @click="runFeishuSourceSync">
              <LoaderCircle v-if="feishuSourceState === 'syncing'" class="spin" :size="16" />
              <Search v-else :size="16" />检查连接
            </button>
            <button v-if="settings.feishuSourceUrl" class="secondary-button" type="button" @click="openFeishuSource">
              <Table2 :size="16" />打开投递表<ExternalLink :size="14" />
            </button>
            <button class="primary-button" type="button" @click="saveSettings"><Check :size="16" />保存设置</button>
          </div>
          <p v-if="feishuSourceMessage" class="connection-result" :class="feishuSourceState">{{ feishuSourceMessage }}</p>
          <p v-else-if="feishuSourceStatus.lastError" class="connection-result error">{{ feishuSourceStatus.lastError }}</p>
        </div>

        <div class="settings-section">
          <div class="settings-heading">
            <div><h2>飞书计划表</h2><p>维护真正的待办，并双向同步已有计划的完成状态。</p></div>
            <label class="toggle-control">
              <input v-model="settings.feishuSyncEnabled" type="checkbox" />
              <span aria-hidden="true" />
              <strong>{{ settings.feishuSyncEnabled ? "已开启" : "已关闭" }}</strong>
            </label>
          </div>
          <div class="form-grid" :class="{ muted: !settings.feishuSyncEnabled }">
            <label><span>应用凭证</span><input value="data\secrets.env" type="text" readonly /></label>
            <label><span>连接状态</span><input :value="feishuStatus.configured ? '凭证已配置' : '等待配置凭证'" type="text" readonly /></label>
            <label><span>同步队列</span><input :value="`${feishuStatus.pendingPlans} 条待同步`" type="text" readonly /></label>
          </div>
          <div class="settings-actions">
            <button class="secondary-button" type="button" :disabled="!settings.feishuSyncEnabled || feishuSyncState === 'syncing'" @click="runFeishuSync">
              <LoaderCircle v-if="feishuSyncState === 'syncing'" class="spin" :size="16" />
              <Cloud v-else :size="16" />初始化并同步
            </button>
            <button v-if="feishuStatus.spreadsheetUrl" class="secondary-button" type="button" @click="openFeishuSheet">
              <Table2 :size="16" />打开表格<ExternalLink :size="14" />
            </button>
            <button class="primary-button" type="button" @click="saveSettings"><Check :size="16" />保存设置</button>
          </div>
          <p v-if="feishuSyncMessage" class="connection-result" :class="feishuSyncState">{{ feishuSyncMessage }}</p>
          <p v-else-if="feishuStatus.lastError" class="connection-result error">{{ feishuStatus.lastError }}</p>
        </div>

        <div class="settings-section">
          <div class="settings-heading">
            <div><h2>飞书待办提醒</h2><p>创建个人飞书任务，双向同步完成状态，并在开始前 3 小时提醒。</p></div>
            <label class="toggle-control">
              <input v-model="settings.feishuTaskRemindersEnabled" type="checkbox" />
              <span aria-hidden="true" />
              <strong>{{ settings.feishuTaskRemindersEnabled ? "已开启" : "已关闭" }}</strong>
            </label>
          </div>
          <div class="form-grid" :class="{ muted: !settings.feishuTaskRemindersEnabled }">
            <label><span>任务负责人</span><input value="投递记录表所有者" type="text" readonly /></label>
            <label><span>提醒时间</span><input value="计划开始前 3 小时" type="text" readonly /></label>
            <label><span>同步队列</span><input :value="`${feishuStatus.pendingTaskReminders} 条待同步`" type="text" readonly /></label>
          </div>
          <div class="settings-actions">
            <button class="secondary-button" type="button" :disabled="!settings.feishuTaskRemindersEnabled || feishuSyncState === 'syncing'" @click="runFeishuSync">
              <LoaderCircle v-if="feishuSyncState === 'syncing'" class="spin" :size="16" />
              <Clock3 v-else :size="16" />立即同步
            </button>
            <button class="primary-button" type="button" @click="saveSettings"><Check :size="16" />保存设置</button>
          </div>
          <p v-if="feishuStatus.taskReminderError" class="connection-result error">{{ feishuStatus.taskReminderError }}</p>
        </div>

        <div class="settings-section">
          <div class="settings-heading">
            <div><h2>数据导出</h2><p>导出原始投喂和当前记忆为带版本号的 JSON。</p></div>
            <Archive :size="22" />
          </div>
          <button class="secondary-button" type="button" @click="exportData"><Download :size="16" />选择位置并导出</button>
        </div>

        <div class="hard-boundaries">
          <h2><ShieldCheck :size="19" />不可逾越的边界</h2>
          <ul>
            <li>原始投喂不会被 AI 修改或覆盖。</li>
            <li>投递表只在选区被高置信识别为求职记录时写入，不会反向扫描生成计划。</li>
            <li>模型输出必须经过 Memory Engine 校验后才能自动写入。</li>
            <li>仅将当前输入和必要的候选记忆发送至你授权的模型服务。</li>
            <li>密钥只从 data\\secrets.env 读取，不进入数据库、前端或导出文件。</li>
            <li>不监听剪贴板、键盘，也不扫描用户目录。</li>
            <li><Smartphone :size="14" />手机推送默认关闭，只发送计划卡片字段，不发送原文和周边上下文。</li>
            <li><Cloud :size="14" />两个飞书通道独立开关；计划表只回读已有计划的完成状态，投递表绝不反向生成计划。</li>
            <li><Clock3 :size="14" />飞书待办只双向同步已有任务的完成状态，并固定提前 3 小时提醒。</li>
          </ul>
        </div>
      </section>
    </main>

    <Transition name="drawer">
      <div v-if="selectedMemory" class="drawer-layer">
        <button class="drawer-scrim" aria-label="关闭详情" @click="selectedMemory = undefined" />
        <aside class="detail-drawer">
          <header>
            <div><span class="type-label">{{ typeLabel(selectedMemory.memory.memoryType) }}</span><h2>{{ selectedMemory.memory.title }}</h2></div>
            <button class="icon-button" type="button" title="关闭" aria-label="关闭" @click="selectedMemory = undefined"><X :size="20" /></button>
          </header>
          <div class="drawer-body">
            <section class="current-memory">
              <span class="eyebrow">当前理解</span>
              <p class="memory-summary">{{ selectedMemory.memory.summary || selectedMemory.memory.body }}</p>
              <div class="source-trust"><ShieldCheck :size="16" />来自 {{ selectedMemory.memory.sourceCount }} 条原始记录</div>
            </section>
            <section class="version-section">
              <h3>版本时间线</h3>
              <article v-for="version in selectedMemory.versions" :key="version.id" class="version-item">
                <div class="version-dot" :class="{ ai: version.authorType !== 'user' }" />
                <div>
                  <div class="version-head"><strong>{{ version.changeReason }}</strong><span>{{ formatTime(version.createdAt) }}</span></div>
                  <p>{{ version.summary || version.body }}</p>
                  <div class="version-meta">
                    <span>{{ version.authorType === "user" ? "用户原文" : "AI 自动整理" }}</span>
                    <span v-if="version.modelInfo">{{ version.modelInfo }}</span>
                    <span>{{ version.sourceEventIds.length }} 个来源</span>
                  </div>
                </div>
              </article>
            </section>
          </div>
        </aside>
      </div>
    </Transition>

    <Transition name="modal">
      <div v-if="deleteTarget" class="modal-layer">
        <button class="modal-scrim" aria-label="取消删除" @click="deleteTarget = undefined" />
        <div class="confirm-dialog" role="alertdialog" aria-modal="true">
          <span class="dialog-icon"><Trash2 :size="22" /></span>
          <h2>永久删除这条记录？</h2>
          <p>对应的派生记忆、索引和待澄清问题也会删除。此操作无法撤销。</p>
          <blockquote>{{ deleteTarget.rawContent }}</blockquote>
          <div>
            <button class="secondary-button" type="button" @click="deleteTarget = undefined">取消</button>
            <button class="danger-button" type="button" @click="confirmDelete"><Trash2 :size="16" />永久删除</button>
          </div>
        </div>
      </div>
    </Transition>

    <Transition name="toast">
      <div v-if="toast" class="toast-message" :class="toast.kind">
        <Check v-if="toast.kind === 'success'" :size="17" />
        <CircleAlert v-else :size="17" />
        {{ toast.message }}
      </div>
    </Transition>
  </div>
</template>
