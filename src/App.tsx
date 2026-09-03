import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  Archive,
  BrainCircuit,
  CalendarClock,
  Check,
  ChevronRight,
  CircleAlert,
  Clock3,
  Cloud,
  Copy,
  Database,
  Download,
  Eye,
  EyeOff,
  ExternalLink,
  FileText,
  Inbox,
  LoaderCircle,
  LockKeyhole,
  Menu,
  NotebookPen,
  Pencil,
  Plus,
  RefreshCw,
  Search,
  Send,
  Settings,
  ShieldCheck,
  Smartphone,
  Sparkles,
  Table2,
  Tag,
  Trash2,
  UnlockKeyhole,
  X,
  type LucideIcon,
} from "lucide-react";
import {
  checkAi,
  createFeed,
  deleteFeed,
  deleteSecretItem,
  exportArchive,
  getMemory,
  initializeVault,
  isTauri,
  listSecretItems,
  lockVault,
  openExternalLink,
  processFeed,
  requestDeleteFeed,
  resolveReview,
  setPlanDone,
  syncFeishuMemosNow,
  syncFeishuNow,
  syncFeishuSecretsNow,
  testMobilePush,
  unlockVault,
  updateMemo,
  updatePlan,
  updateSecretItem,
  updateSettings,
  type AppSettings,
  type FeedEvent,
  type FeishuSecretStatus,
  type FeishuSyncStatus,
  type MemoryDetail,
  type MemoItem,
  type Page,
  type PlanItem,
  type SecretItem,
} from "./api";
import {
  loadDashboard,
  loadFeeds,
  loadFeishuStatuses,
  loadMemos,
  loadMemories,
  loadPlans,
  loadVault,
  settingsChanged,
  vaultChanged,
} from "./store/appSlice";
import { useAppDispatch, useAppSelector } from "./store/store";

interface SecretEditorForm {
  id: string;
  title: string;
  secretType: string;
  account: string;
  secretValue: string;
  website: string;
  notes: string;
}
interface MemoEditorForm {
  id: string;
  content: string;
}
interface PlanEditorForm {
  id: string;
  title: string;
  details: string;
  content: string;
  linkUrl: string;
  notes: string;
  scheduledLocal: string;
  reminderMinutesBefore: number;
  tag: string;
}
type RequestState = "idle" | "checking" | "syncing" | "success" | "error";

const pageTitles: Record<Page, { title: string; subtitle: string }> = {
  inbox: { title: "收集箱", subtitle: "原样保存每一次输入，再慢慢理解" },
  memories: { title: "记忆", subtitle: "当前理解，以及它从哪里来" },
  memo: { title: "备忘录", subtitle: "留给更长远的事情" },
  plans: { title: "桌面计划", subtitle: "查看并调整接下来的安排" },
  secrets: { title: "秘密备忘录", subtitle: "本地加密保存，仅在解锁后显示" },
  review: { title: "待澄清", subtitle: "只有无法可靠理解的内容才会停在这里" },
  settings: { title: "设置", subtitle: "模型、数据和隐私边界" },
};
const navItems: Array<{ id: Page; label: string; icon: LucideIcon }> = [
  { id: "inbox", label: "收集箱", icon: Inbox },
  { id: "memories", label: "记忆", icon: BrainCircuit },
  { id: "memo", label: "备忘录", icon: NotebookPen },
  { id: "plans", label: "桌面计划", icon: CalendarClock },
  { id: "secrets", label: "秘密备忘录", icon: LockKeyhole },
  { id: "review", label: "待澄清", icon: CircleAlert },
  { id: "settings", label: "设置", icon: Settings },
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
const errorMessage = (error: unknown) =>
  typeof error === "string"
    ? error
    : error instanceof Error
      ? error.message
      : "操作失败，请稍后再试";
const formatTime = (timestamp: number) => {
  const date = new Date(timestamp);
  const today = new Date();
  return date.toDateString() === today.toDateString()
    ? date.toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" })
    : date.toLocaleDateString("zh-CN", {
        month: "short",
        day: "numeric",
        hour: "2-digit",
        minute: "2-digit",
      });
};
const formatPlanSchedule = (timestamp?: number) =>
  timestamp
    ? new Date(timestamp).toLocaleString("zh-CN", {
        month: "short",
        day: "numeric",
        weekday: "short",
        hour: "2-digit",
        minute: "2-digit",
      })
    : "等待安排时间";
const toLocalDateTimeInput = (timestamp?: number) => {
  if (!timestamp) return "";
  const date = new Date(timestamp);
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
};
const statusLabel = (status: string) =>
  ({
    pending: "待整理",
    processing: "整理中",
    review: "待澄清",
    classified: "已归类",
    ignored: "仅保留原文",
    accepted: "已确认",
    rejected: "保留原文",
  })[status] ?? status;
const typeLabel = (type: string) =>
  ({
    Knowledge: "知识",
    Project: "项目",
    Decision: "决定",
    Idea: "想法",
    Task: "待办",
    Preference: "偏好",
    Person: "人物",
    Experience: "经历",
    Unclassified: "未分类",
  })[type] ?? type;

export default function App() {
  const dispatch = useAppDispatch();
  const {
    feeds,
    memories,
    memos,
    plans,
    reviews,
    stats,
    settings,
    vault,
    feishu,
    feishuMemo,
    feishuSecret,
    loading,
  } = useAppSelector((state) => state.app);
  const [activePage, setActivePage] = useState<Page>("inbox");
  const activePageRef = useRef(activePage);
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [composer, setComposer] = useState("");
  const composerElement = useRef<HTMLTextAreaElement>(null);
  const [feedSearch, setFeedSearch] = useState("");
  const [memorySearch, setMemorySearch] = useState("");
  const [memoryType, setMemoryType] = useState("");
  const [planView, setPlanView] = useState<"active" | "all">("active");
  const [planTagFilter, setPlanTagFilter] = useState<"all" | "面试">("all");
  const [selectedMemory, setSelectedMemory] = useState<MemoryDetail>();
  const [deleteTarget, setDeleteTarget] = useState<FeedEvent>();
  const [saving, setSaving] = useState(false);
  const [processingIds, setProcessingIds] = useState(() => new Set<string>());
  const [toast, setToast] = useState<{
    message: string;
    kind: "success" | "error";
  }>();
  const [aiCheck, setAiCheck] = useState<{
    state: RequestState;
    message: string;
  }>({ state: "idle", message: "" });
  const [pushCheck, setPushCheck] = useState<{
    state: RequestState;
    message: string;
  }>({ state: "idle", message: "" });
  const [feishuSync, setFeishuSync] = useState<{
    state: RequestState;
    message: string;
  }>({ state: "idle", message: "" });
  const [secretSync, setSecretSync] = useState<{
    state: RequestState;
    message: string;
  }>({ state: "idle", message: "" });
  const [memoSync, setMemoSync] = useState<{
    state: RequestState;
    message: string;
  }>({ state: "idle", message: "" });
  const [memoEditor, setMemoEditor] = useState<MemoEditorForm>();
  const [memoEditBusy, setMemoEditBusy] = useState(false);
  const [planEditor, setPlanEditor] = useState<PlanEditorForm>();
  const [planEditBusy, setPlanEditBusy] = useState(false);
  const [planStatusBusy, setPlanStatusBusy] = useState(() => new Set<string>());
  const [secretSearch, setSecretSearch] = useState("");
  const [secretItems, setSecretItems] = useState<SecretItem[]>([]);
  const [vaultPassword, setVaultPassword] = useState("");
  const [vaultPasswordConfirm, setVaultPasswordConfirm] = useState("");
  const [vaultBusy, setVaultBusy] = useState(false);
  const [revealedSecrets, setRevealedSecrets] = useState(
    () => new Set<string>(),
  );
  const [secretEditor, setSecretEditor] = useState<SecretEditorForm>();
  const [secretEditBusy, setSecretEditBusy] = useState(false);
  const [secretDeleteTarget, setSecretDeleteTarget] = useState<SecretItem>();
  const [secretDeletePassword, setSecretDeletePassword] = useState("");
  const [secretDeleteError, setSecretDeleteError] = useState("");
  const [secretDeleteBusy, setSecretDeleteBusy] = useState(false);
  const activePlanCount = useMemo(
    () => plans.filter((plan) => plan.status !== "done").length,
    [plans],
  );
  const visiblePlans = useMemo(
    () =>
      plans.filter(
        (plan) =>
          (planView === "all" || plan.status !== "done") &&
          (planTagFilter === "all" || plan.tag === planTagFilter),
      ),
    [plans, planView, planTagFilter],
  );
  const filteredSecrets = useMemo(() => {
    const query = secretSearch.trim().toLowerCase();
    return query
      ? secretItems.filter((secret) =>
          [
            secret.title,
            secret.secretType,
            secret.account,
            secret.website,
            secret.notes,
          ]
            .filter(Boolean)
            .some((value) => value!.toLowerCase().includes(query)),
        )
      : secretItems;
  }, [secretItems, secretSearch]);

  function notify(message: string, kind: "success" | "error") {
    setToast({ message, kind });
    window.setTimeout(
      () =>
        setToast((current) =>
          current?.message === message ? undefined : current,
        ),
      3200,
    );
  }
  const reloadAll = () =>
    dispatch(
      loadDashboard({
        feed: feedSearch,
        memory: memorySearch,
        type: memoryType,
      }),
    )
      .unwrap()
      .catch((error) => notify(errorMessage(error), "error"));
  async function refreshSecretVault(
    loadDecryptedItems = activePageRef.current === "secrets",
  ) {
    try {
      const status = await dispatch(loadVault()).unwrap();
      if (!status.unlocked || !loadDecryptedItems) {
        setSecretItems([]);
        setRevealedSecrets(new Set());
        return;
      }
      setSecretItems(await listSecretItems());
    } catch (error) {
      notify(errorMessage(error), "error");
    }
  }
  useEffect(() => {
    activePageRef.current = activePage;
  }, [activePage]);
  useEffect(() => {
    void reloadAll();
    void dispatch(loadFeishuStatuses());
    void refreshSecretVault();
    const stops: Array<() => void> = [];
    let disposed = false;
    if (isTauri) {
      void listen("vault-changed", () => {
        void refreshSecretVault();
        void dispatch(loadFeishuStatuses());
      }).then((stop) => (disposed ? stop() : stops.push(stop)));
      void listen("memos-changed", () => {
        if (activePageRef.current === "memo") void dispatch(loadMemos());
      }).then((stop) => (disposed ? stop() : stops.push(stop)));
      void listen("plans-changed", () => void dispatch(loadPlans(true))).then(
        (stop) => (disposed ? stop() : stops.push(stop)),
      );
    }
    const keydown = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        navigate("inbox");
        window.setTimeout(() => composerElement.current?.focus());
      }
      if (event.key === "Escape") {
        setSelectedMemory(undefined);
        setDeleteTarget(undefined);
        closeEditors();
        setSidebarOpen(false);
      }
    };
    window.addEventListener("keydown", keydown);
    return () => {
      disposed = true;
      stops.forEach((stop) => stop());
      window.removeEventListener("keydown", keydown);
    };
  }, []);

  function setSetting<K extends keyof AppSettings>(
    key: K,
    value: AppSettings[K],
  ) {
    dispatch(settingsChanged({ ...settings, [key]: value }));
  }
  function closeEditors() {
    setMemoEditor(undefined);
    setPlanEditor(undefined);
    setSecretEditor(undefined);
    setSecretDeleteTarget(undefined);
    setSecretDeletePassword("");
    setSecretDeleteError("");
  }
  function navigate(page: Page) {
    closeEditors();
    setActivePage(page);
    setSidebarOpen(false);
    if (page !== "secrets") {
      setSecretItems([]);
      setRevealedSecrets(new Set());
    }
    if (page === "memo") void dispatch(loadMemos());
    if (page === "plans") void dispatch(loadPlans(true));
    if (page === "secrets") void refreshSecretVault(true);
  }
  async function submitFeed(event?: FormEvent) {
    event?.preventDefault();
    if (!composer.trim() || saving) return;
    setSaving(true);
    try {
      const result = await createFeed(composer);
      setComposer("");
      notify("已保存原始记录", "success");
      await reloadAll();
      if (settings.aiEnabled) void runProcessing(result.feedId, false);
    } catch (error) {
      notify(errorMessage(error), "error");
    } finally {
      setSaving(false);
      window.setTimeout(() => composerElement.current?.focus());
    }
  }
  async function runProcessing(feedId: string, announce = true) {
    setProcessingIds((current) => new Set(current).add(feedId));
    try {
      const result = await processFeed(feedId);
      if (announce || result.status === "review")
        notify(result.message, "success");
      await reloadAll();
    } catch (error) {
      notify(errorMessage(error), "error");
      await reloadAll();
    } finally {
      setProcessingIds((current) => {
        const next = new Set(current);
        next.delete(feedId);
        return next;
      });
    }
  }
  async function openMemory(memoryId?: string) {
    if (!memoryId) return;
    try {
      setSelectedMemory(await getMemory(memoryId));
    } catch (error) {
      notify(errorMessage(error), "error");
    }
  }
  async function decideReview(reviewId: string, accept: boolean) {
    try {
      await resolveReview(reviewId, accept);
      notify(accept ? "已按原文保留为未分类记忆" : "已关闭这条澄清", "success");
      await reloadAll();
    } catch (error) {
      notify(errorMessage(error), "error");
    }
  }
  async function confirmDelete() {
    if (!deleteTarget) return;
    try {
      const token = await requestDeleteFeed(deleteTarget.id);
      await deleteFeed(deleteTarget.id, token);
      setDeleteTarget(undefined);
      notify("记录及其派生记忆已永久删除", "success");
      await reloadAll();
    } catch (error) {
      notify(errorMessage(error), "error");
    }
  }
  async function saveSettings() {
    try {
      await updateSettings(settings);
      notify("设置已保存", "success");
    } catch (error) {
      notify(errorMessage(error), "error");
    }
  }
  async function testAi() {
    setAiCheck({ state: "checking", message: "正在检查 LLM 与 Embedding..." });
    try {
      setAiCheck({ state: "success", message: await checkAi() });
    } catch (error) {
      setAiCheck({ state: "error", message: errorMessage(error) });
    }
  }
  async function checkMobilePush() {
    setPushCheck({ state: "checking", message: "正在发送测试提醒..." });
    try {
      setPushCheck({
        state: "success",
        message: await testMobilePush(settings.mobilePushProvider),
      });
    } catch (error) {
      setPushCheck({ state: "error", message: errorMessage(error) });
    }
  }
  async function runPlanSync() {
    if (feishuSync.state === "syncing") return;
    setFeishuSync({
      state: "syncing",
      message: "正在同步飞书计划表和待办提醒...",
    });
    try {
      await updateSettings(settings);
      setFeishuSync({ state: "success", message: await syncFeishuNow() });
      await dispatch(loadFeishuStatuses());
    } catch (error) {
      setFeishuSync({ state: "error", message: errorMessage(error) });
      await dispatch(loadFeishuStatuses());
    }
  }
  async function runMemoSync() {
    if (memoSync.state === "syncing") return;
    setMemoSync({ state: "syncing", message: "正在同步飞书备忘录..." });
    try {
      setMemoSync({ state: "success", message: await syncFeishuMemosNow() });
      await dispatch(loadMemos());
    } catch (error) {
      setMemoSync({ state: "error", message: errorMessage(error) });
      await dispatch(loadMemos());
    }
  }
  async function runSecretSync() {
    if (secretSync.state === "syncing") return;
    setSecretSync({ state: "syncing", message: "正在单向同步秘密记录..." });
    try {
      await updateSettings(settings);
      setSecretSync({
        state: "success",
        message: await syncFeishuSecretsNow(),
      });
      await Promise.all([refreshSecretVault(), dispatch(loadFeishuStatuses())]);
    } catch (error) {
      setSecretSync({ state: "error", message: errorMessage(error) });
      await dispatch(loadFeishuStatuses());
    }
  }
  async function openUrl(url?: string) {
    if (url) await openExternalLink(url);
  }
  async function saveMemoEdit(event: FormEvent) {
    event.preventDefault();
    if (!memoEditor?.content.trim() || memoEditBusy) return;
    setMemoEditBusy(true);
    try {
      await updateMemo(memoEditor.id, memoEditor.content);
      setMemoEditor(undefined);
      await dispatch(loadMemos());
      notify("备忘已更新", "success");
    } catch (error) {
      notify(errorMessage(error), "error");
    } finally {
      setMemoEditBusy(false);
    }
  }
  function openPlanEditor(plan: PlanItem) {
    setPlanEditor({
      id: plan.id,
      title: plan.title,
      details: plan.details,
      content: plan.content,
      linkUrl: plan.linkUrl ?? "",
      notes: plan.notes ?? "",
      scheduledLocal: toLocalDateTimeInput(plan.scheduledAt),
      reminderMinutesBefore: plan.reminderMinutesBefore,
      tag: plan.tag ?? "",
    });
  }
  async function savePlanEdit(event: FormEvent) {
    event.preventDefault();
    if (
      !planEditor ||
      planEditBusy ||
      !planEditor.title.trim() ||
      !planEditor.content.trim() ||
      !planEditor.details.trim()
    )
      return;
    const scheduledAt = planEditor.scheduledLocal
      ? new Date(planEditor.scheduledLocal).getTime()
      : undefined;
    if (scheduledAt !== undefined && !Number.isFinite(scheduledAt))
      return notify("计划时间无效", "error");
    setPlanEditBusy(true);
    try {
      await updatePlan(planEditor.id, {
        title: planEditor.title,
        details: planEditor.details,
        content: planEditor.content,
        linkUrl: planEditor.linkUrl || undefined,
        notes: planEditor.notes || undefined,
        scheduledAt,
        reminderMinutesBefore: planEditor.reminderMinutesBefore,
        tag: planEditor.tag || undefined,
      });
      setPlanEditor(undefined);
      await Promise.all([
        dispatch(loadPlans(true)),
        dispatch(loadFeishuStatuses()),
      ]);
      notify("桌面计划已更新", "success");
    } catch (error) {
      notify(errorMessage(error), "error");
    } finally {
      setPlanEditBusy(false);
    }
  }
  async function changePlanDone(plan: PlanItem, done: boolean) {
    if (planStatusBusy.has(plan.id)) return;
    setPlanStatusBusy((current) => new Set(current).add(plan.id));
    try {
      await setPlanDone(plan.id, done);
      await Promise.all([
        dispatch(loadPlans(true)),
        dispatch(loadFeishuStatuses()),
      ]);
      notify(done ? "计划已完成" : "计划已重新打开", "success");
    } catch (error) {
      notify(errorMessage(error), "error");
    } finally {
      setPlanStatusBusy((current) => {
        const next = new Set(current);
        next.delete(plan.id);
        return next;
      });
    }
  }
  async function authenticateVault(initializing: boolean) {
    if (vaultBusy || vaultPassword.length < 6) return;
    if (initializing && vaultPassword !== vaultPasswordConfirm)
      return notify("两次主密码不一致", "error");
    setVaultBusy(true);
    try {
      const status = initializing
        ? await initializeVault(vaultPassword)
        : await unlockVault(vaultPassword);
      dispatch(vaultChanged(status));
      setVaultPassword("");
      setVaultPasswordConfirm("");
      await refreshSecretVault();
    } catch (error) {
      notify(errorMessage(error), "error");
    } finally {
      setVaultBusy(false);
    }
  }
  async function lockSecretVault() {
    closeEditors();
    dispatch(vaultChanged(await lockVault()));
    setSecretItems([]);
    setRevealedSecrets(new Set());
  }
  function toggleSecretReveal(id: string) {
    setRevealedSecrets((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }
  async function copySecret(secret: SecretItem) {
    try {
      await navigator.clipboard.writeText(secret.secretValue);
      notify("秘密值已复制", "success");
    } catch (error) {
      notify(errorMessage(error), "error");
    }
  }
  function openSecretEditor(secret: SecretItem) {
    setSecretDeleteTarget(undefined);
    setSecretEditor({
      id: secret.id,
      title: secret.title,
      secretType: secret.secretType,
      account: secret.account ?? "",
      secretValue: secret.secretValue,
      website: secret.website ?? "",
      notes: secret.notes ?? "",
    });
  }
  async function saveSecretEdit(event: FormEvent) {
    event.preventDefault();
    if (
      !secretEditor ||
      secretEditBusy ||
      !secretEditor.title.trim() ||
      !secretEditor.secretType.trim() ||
      !secretEditor.secretValue.trim()
    )
      return;
    setSecretEditBusy(true);
    try {
      await updateSecretItem(secretEditor.id, secretEditor);
      setSecretEditor(undefined);
      await refreshSecretVault();
      notify("秘密记录已更新", "success");
    } catch (error) {
      notify(errorMessage(error), "error");
    } finally {
      setSecretEditBusy(false);
    }
  }
  async function confirmSecretDelete(event: FormEvent) {
    event.preventDefault();
    if (
      !secretDeleteTarget ||
      secretDeleteBusy ||
      secretDeletePassword.length < 6
    )
      return;
    setSecretDeleteBusy(true);
    setSecretDeleteError("");
    try {
      await deleteSecretItem(secretDeleteTarget.id, secretDeletePassword);
      setSecretDeleteTarget(undefined);
      setSecretDeletePassword("");
      await refreshSecretVault();
      notify("秘密记录已删除", "success");
    } catch (error) {
      setSecretDeleteError(errorMessage(error));
      setSecretDeletePassword("");
    } finally {
      setSecretDeleteBusy(false);
    }
  }

  return (
    <div className="app-shell">
      {sidebarOpen && (
        <div className="mobile-scrim" onClick={() => setSidebarOpen(false)} />
      )}
      <aside className={`sidebar${sidebarOpen ? " open" : ""}`}>
        <div className="brand">
          <span className="brand-mark">
            <FileText size={20} />
          </span>
          <span className="brand-name">FeedNote</span>
        </div>
        <nav className="primary-nav" aria-label="主导航">
          {navItems.map(({ id, label, icon: Icon }) => (
            <button
              key={id}
              className={`nav-item${activePage === id ? " active" : ""}`}
              type="button"
              onClick={() => navigate(id)}
            >
              <Icon size={18} />
              <span>{label}</span>
              {id === "plans" && activePlanCount > 0 && (
                <span className="nav-count plan-count">{activePlanCount}</span>
              )}
              {id === "review" && stats.pendingReviews > 0 && (
                <span className="nav-count">{stats.pendingReviews}</span>
              )}
            </button>
          ))}
        </nav>
        <div className="sidebar-footer">
          <div className="privacy-state">
            <ShieldCheck size={16} />
            <div>
              <strong>数据本机保存</strong>
              <span>
                {settings.aiEnabled ? "DeepSeek 云端理解" : "AI 已暂停"}
              </span>
            </div>
          </div>
          {!isTauri && <span className="preview-label">浏览器预览</span>}
        </div>
      </aside>
      <main className="main-content">
        <header className="topbar">
          <button
            className="icon-button mobile-menu"
            type="button"
            aria-label="打开菜单"
            title="打开菜单"
            onClick={() => setSidebarOpen(true)}
          >
            <Menu size={20} />
          </button>
          <div>
            <h1>{pageTitles[activePage].title}</h1>
            <p>{pageTitles[activePage].subtitle}</p>
          </div>
          <button
            className="quick-capture"
            type="button"
            title="快速记录"
            onClick={() => navigate("inbox")}
          >
            <Plus size={17} />
            <span>快速记录</span>
            <kbd>Ctrl K</kbd>
          </button>
        </header>
        {activePage === "inbox" && (
          <section className="page inbox-page">
            <div className="composer-section">
              <div className="composer-heading">
                <div>
                  <h2>今天想记下什么？</h2>
                  <p>先原样保存，再由 AI 自动分类和整理，不覆盖你的原话。</p>
                </div>
                <span className="save-state">
                  <Database size={15} /> 本机保存
                </span>
              </div>
              <div className={`composer-wrap${composer ? " focused" : ""}`}>
                <textarea
                  ref={composerElement}
                  value={composer}
                  onChange={(e) => setComposer(e.target.value)}
                  rows={5}
                  maxLength={100000}
                  placeholder="输入、粘贴，或者先写下一句不完整的想法..."
                  onKeyDown={(e) => {
                    if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
                      e.preventDefault();
                      void submitFeed();
                    }
                  }}
                />
                <div className="composer-actions">
                  <span>
                    {composer.length
                      ? `${composer.length} 字`
                      : "Ctrl + Enter 保存"}
                  </span>
                  <button
                    className="primary-button"
                    type="button"
                    disabled={!composer.trim() || saving}
                    onClick={() => void submitFeed()}
                  >
                    {saving ? (
                      <LoaderCircle className="spin" size={17} />
                    ) : (
                      <Plus size={17} />
                    )}
                    保存
                  </button>
                </div>
              </div>
            </div>
            <div className="metric-strip" aria-label="记忆概览">
              <div>
                <strong>{stats.totalFeeds}</strong>
                <span>原始记录</span>
              </div>
              <div>
                <strong>{stats.totalMemories}</strong>
                <span>活跃记忆</span>
              </div>
              <div>
                <strong>{stats.pendingReviews}</strong>
                <span>等待澄清</span>
              </div>
              <div>
                <strong>{stats.pendingProcessing}</strong>
                <span>等待整理</span>
              </div>
            </div>
            <div className="section-toolbar">
              <h2>最近投喂</h2>
              <label className="search-control compact">
                <Search size={16} />
                <input
                  value={feedSearch}
                  onChange={(e) => {
                    setFeedSearch(e.target.value);
                    void dispatch(loadFeeds(e.target.value));
                  }}
                  type="search"
                  placeholder="搜索原始记录"
                />
              </label>
            </div>
            {loading ? (
              <Empty icon={LoaderCircle} text="正在读取本地记忆" spin />
            ) : feeds.length === 0 ? (
              <Empty
                icon={Inbox}
                title="收集箱还是空的"
                text="写下一件刚刚发生的事，或粘贴一段值得留下的内容。"
              />
            ) : (
              <div className="feed-list">
                {feeds.map((feed) => (
                  <article key={feed.id} className="feed-row">
                    <div
                      className={`timeline-dot status-${feed.processingStatus}`}
                    />
                    <div
                      className="feed-main"
                      onClick={() => void openMemory(feed.memoryId)}
                    >
                      <p>{feed.rawContent}</p>
                      <div className="row-meta">
                        <span>
                          <Clock3 size={13} /> {formatTime(feed.createdAt)}
                        </span>
                        <span
                          className={`status-pill status-${feed.processingStatus}`}
                        >
                          {statusLabel(feed.processingStatus)}
                        </span>
                      </div>
                    </div>
                    <div className="row-actions">
                      {feed.processingStatus === "pending" &&
                        settings.aiEnabled && (
                          <button
                            className="icon-button"
                            type="button"
                            title="尝试 AI 整理"
                            aria-label="尝试 AI 整理"
                            disabled={processingIds.has(feed.id)}
                            onClick={() => void runProcessing(feed.id)}
                          >
                            {processingIds.has(feed.id) ? (
                              <LoaderCircle className="spin" size={17} />
                            ) : (
                              <Sparkles size={17} />
                            )}
                          </button>
                        )}
                      <button
                        className="icon-button danger"
                        type="button"
                        title="永久删除"
                        aria-label="永久删除"
                        onClick={() => setDeleteTarget(feed)}
                      >
                        <Trash2 size={17} />
                      </button>
                      <button
                        className="icon-button"
                        type="button"
                        title="查看记忆"
                        aria-label="查看记忆"
                        onClick={() => void openMemory(feed.memoryId)}
                      >
                        <ChevronRight size={18} />
                      </button>
                    </div>
                  </article>
                ))}
              </div>
            )}
          </section>
        )}
        {activePage === "memories" && (
          <section className="page">
            <div className="filters-bar">
              <label className="search-control">
                <Search size={17} />
                <input
                  value={memorySearch}
                  onChange={(e) => {
                    setMemorySearch(e.target.value);
                    void dispatch(
                      loadMemories({ query: e.target.value, type: memoryType }),
                    );
                  }}
                  type="search"
                  placeholder="搜索标题、正文或摘要"
                />
              </label>
              <select
                value={memoryType}
                onChange={(e) => {
                  setMemoryType(e.target.value);
                  void dispatch(
                    loadMemories({ query: memorySearch, type: e.target.value }),
                  );
                }}
                aria-label="记忆类型"
              >
                <option value="">全部类型</option>
                {memoryTypes.map((type) => (
                  <option key={type} value={type}>
                    {typeLabel(type)}
                  </option>
                ))}
              </select>
            </div>
            {memories.length === 0 ? (
              <Empty
                icon={BrainCircuit}
                title="没有匹配的记忆"
                text="记忆会从每一次原始投喂中产生，并保留来源。"
              />
            ) : (
              <div className="memory-list">
                {memories.map((memory) => (
                  <button
                    key={memory.id}
                    className="memory-row"
                    type="button"
                    onClick={() => void openMemory(memory.id)}
                  >
                    <span className="type-mark" data-type={memory.memoryType}>
                      {typeLabel(memory.memoryType).slice(0, 1)}
                    </span>
                    <span className="memory-copy">
                      <span className="memory-title-line">
                        <strong>{memory.title}</strong>
                        <span className="type-label">
                          {typeLabel(memory.memoryType)}
                        </span>
                      </span>
                      <span>{memory.summary || memory.body}</span>
                    </span>
                    <span className="memory-source">
                      {memory.sourceCount} 个来源
                    </span>
                    <ChevronRight size={18} />
                  </button>
                ))}
              </div>
            )}
          </section>
        )}
        {activePage === "memo" && (
          <MemoPage
            memos={memos}
            status={feishuMemo}
            sync={memoSync}
            onSync={() => void runMemoSync()}
            onOpen={() => void openUrl(feishuMemo.spreadsheetUrl)}
            onEdit={(memo) =>
              setMemoEditor({ id: memo.id, content: memo.content })
            }
          />
        )}
        {activePage === "plans" && (
          <PlansPage
            plans={visiblePlans}
            allCount={plans.length}
            activeCount={activePlanCount}
            view={planView}
            tag={planTagFilter}
            busy={planStatusBusy}
            onView={setPlanView}
            onTag={setPlanTagFilter}
            onEdit={openPlanEditor}
            onDone={(plan, done) => void changePlanDone(plan, done)}
          />
        )}
        {activePage === "secrets" && (
          <SecretsPage
            vault={vault}
            secrets={filteredSecrets}
            total={secretItems.length}
            search={secretSearch}
            password={vaultPassword}
            confirm={vaultPasswordConfirm}
            busy={vaultBusy}
            revealed={revealedSecrets}
            onSearch={setSecretSearch}
            onPassword={setVaultPassword}
            onConfirm={setVaultPasswordConfirm}
            onAuth={(initializing) => void authenticateVault(initializing)}
            onLock={() => void lockSecretVault()}
            onReveal={toggleSecretReveal}
            onCopy={(secret) => void copySecret(secret)}
            onEdit={openSecretEditor}
            onDelete={(secret) => {
              setSecretEditor(undefined);
              setSecretDeleteTarget(secret);
              setSecretDeletePassword("");
              setSecretDeleteError("");
            }}
            onOpen={(url) => void openUrl(url)}
          />
        )}
        {activePage === "review" && (
          <section className="page review-page">
            <div className="boundary-banner">
              <ShieldCheck size={21} />
              <div>
                <strong>普通分类由系统自动完成</strong>
                <span>
                  这里只处理指代不明、信息缺失或重大冲突，原始投喂始终保留。
                </span>
              </div>
            </div>
            {reviews.length === 0 ? (
              <Empty
                icon={Check}
                title="没有需要澄清的内容"
                text="分类、标题和摘要会自动完成，不需要逐条审批。"
              />
            ) : (
              <div className="review-list">
                {reviews.map((review) => (
                  <article key={review.id} className="review-card">
                    <div className="review-topline">
                      <span className={`risk-label risk-${review.riskLevel}`}>
                        {review.riskLevel === "high" ? "需要澄清" : "存在冲突"}
                      </span>
                      <span>{formatTime(review.createdAt)}</span>
                    </div>
                    {review.proposedAction === "ask" ? (
                      <>
                        <h3>
                          {review.payload.question || "这条内容指的是什么？"}
                        </h3>
                        <p>{review.reason}</p>
                      </>
                    ) : (
                      <>
                        <div className="proposal-grid">
                          <span>建议类型</span>
                          <strong>
                            {typeLabel(
                              review.payload.memoryType || "Unclassified",
                            )}
                          </strong>
                          <span>建议标题</span>
                          <strong>{review.payload.title}</strong>
                          <span>理解摘要</span>
                          <strong>{review.payload.summary}</strong>
                        </div>
                        <p className="proposal-reason">{review.reason}</p>
                      </>
                    )}
                    <div className="review-actions">
                      <button
                        className="secondary-button"
                        type="button"
                        onClick={() => void decideReview(review.id, false)}
                      >
                        <X size={16} />
                        关闭问题
                      </button>
                      <button
                        className="primary-button"
                        type="button"
                        onClick={() => void decideReview(review.id, true)}
                      >
                        <Check size={16} />
                        暂时保留未分类
                      </button>
                    </div>
                  </article>
                ))}
              </div>
            )}
          </section>
        )}
        {activePage === "settings" && (
          <SettingsPage
            settings={settings}
            feishu={feishu}
            secretStatus={feishuSecret}
            vaultUnlocked={vault.unlocked}
            aiCheck={aiCheck}
            pushCheck={pushCheck}
            planSync={feishuSync}
            secretSync={secretSync}
            onSetting={setSetting}
            onSave={() => void saveSettings()}
            onTestAi={() => void testAi()}
            onTestPush={() => void checkMobilePush()}
            onPlanSync={() => void runPlanSync()}
            onSecretSync={() => void runSecretSync()}
            onOpen={openUrl}
            onExport={() =>
              void exportArchive()
                .then((done) => done && notify("数据已导出", "success"))
                .catch((error) => notify(errorMessage(error), "error"))
            }
          />
        )}
      </main>
      {selectedMemory && (
        <MemoryDrawer
          detail={selectedMemory}
          onClose={() => setSelectedMemory(undefined)}
        />
      )}
      {planEditor && (
        <PlanEditor
          editor={planEditor}
          busy={planEditBusy}
          onChange={setPlanEditor}
          onClose={() => setPlanEditor(undefined)}
          onSubmit={savePlanEdit}
        />
      )}
      {memoEditor && (
        <MemoEditor
          editor={memoEditor}
          busy={memoEditBusy}
          onChange={setMemoEditor}
          onClose={() => setMemoEditor(undefined)}
          onSubmit={saveMemoEdit}
        />
      )}
      {secretEditor && (
        <SecretEditor
          editor={secretEditor}
          busy={secretEditBusy}
          onChange={setSecretEditor}
          onClose={() => setSecretEditor(undefined)}
          onSubmit={saveSecretEdit}
        />
      )}
      {secretDeleteTarget && (
        <div className="modal-layer">
          <button
            className="modal-scrim"
            aria-label="取消删除"
            onClick={() => setSecretDeleteTarget(undefined)}
          />
          <form
            className="secret-dialog secret-delete-dialog"
            role="alertdialog"
            aria-modal="true"
            onSubmit={confirmSecretDelete}
          >
            <span className="dialog-icon">
              <Trash2 size={22} />
            </span>
            <h2>永久删除“{secretDeleteTarget.title}”？</h2>
            <p>请输入秘密备忘录主密码。密码只用于本次删除验证。</p>
            <label className="delete-password-field">
              <span>主密码</span>
              <input
                value={secretDeletePassword}
                onChange={(e) => setSecretDeletePassword(e.target.value)}
                type="password"
                minLength={6}
                maxLength={256}
                autoComplete="current-password"
                autoFocus
                placeholder="至少 6 个字符"
              />
            </label>
            {secretDeleteError && (
              <p className="dialog-error">{secretDeleteError}</p>
            )}
            <div className="dialog-actions">
              <button
                className="secondary-button"
                type="button"
                onClick={() => setSecretDeleteTarget(undefined)}
              >
                取消
              </button>
              <button
                className="danger-button"
                type="submit"
                disabled={secretDeleteBusy || secretDeletePassword.length < 6}
              >
                {secretDeleteBusy ? (
                  <LoaderCircle className="spin" size={16} />
                ) : (
                  <Trash2 size={16} />
                )}
                永久删除
              </button>
            </div>
          </form>
        </div>
      )}
      {deleteTarget && (
        <div className="modal-layer">
          <button
            className="modal-scrim"
            aria-label="取消删除"
            onClick={() => setDeleteTarget(undefined)}
          />
          <div className="confirm-dialog" role="alertdialog" aria-modal="true">
            <span className="dialog-icon">
              <Trash2 size={22} />
            </span>
            <h2>永久删除这条记录？</h2>
            <p>对应的派生记忆、索引和待澄清问题也会删除。此操作无法撤销。</p>
            <blockquote>{deleteTarget.rawContent}</blockquote>
            <div>
              <button
                className="secondary-button"
                type="button"
                onClick={() => setDeleteTarget(undefined)}
              >
                取消
              </button>
              <button
                className="danger-button"
                type="button"
                onClick={() => void confirmDelete()}
              >
                <Trash2 size={16} />
                永久删除
              </button>
            </div>
          </div>
        </div>
      )}
      {toast && (
        <div className={`toast-message ${toast.kind}`}>
          {toast.kind === "success" ? (
            <Check size={17} />
          ) : (
            <CircleAlert size={17} />
          )}
          {toast.message}
        </div>
      )}
    </div>
  );
}

function Empty({
  icon: Icon,
  title,
  text,
  spin,
}: {
  icon: LucideIcon;
  title?: string;
  text: string;
  spin?: boolean;
}) {
  return (
    <div className="empty-state">
      <Icon className={spin ? "spin" : ""} size={title ? 28 : 22} />
      {title && <strong>{title}</strong>}
      <span>{text}</span>
    </div>
  );
}

function MemoPage({
  memos,
  status,
  sync,
  onSync,
  onOpen,
  onEdit,
}: {
  memos: MemoItem[];
  status: {
    configured: boolean;
    pendingMemos: number;
    spreadsheetUrl?: string;
    lastError?: string;
  };
  sync: { state: RequestState; message: string };
  onSync: () => void;
  onOpen: () => void;
  onEdit: (memo: MemoItem) => void;
}) {
  return (
    <section className="page memo-page">
      <div className="memo-toolbar">
        <span>
          {memos.length} 条备忘
          {status.pendingMemos ? ` · ${status.pendingMemos} 条待同步` : ""}
        </span>
        <div>
          <button
            className="secondary-button"
            type="button"
            disabled={sync.state === "syncing" || !status.configured}
            onClick={onSync}
          >
            {sync.state === "syncing" ? (
              <LoaderCircle className="spin" size={15} />
            ) : (
              <RefreshCw size={15} />
            )}
            同步
          </button>
          {status.spreadsheetUrl && (
            <button className="secondary-button" type="button" onClick={onOpen}>
              <ExternalLink size={15} />
              打开飞书表格
            </button>
          )}
        </div>
      </div>
      {sync.message ? (
        <p className={`connection-result ${sync.state}`}>{sync.message}</p>
      ) : (
        status.lastError && (
          <p className="connection-result error">{status.lastError}</p>
        )
      )}
      {memos.length === 0 ? (
        <Empty
          icon={NotebookPen}
          title="还没有备忘内容"
          text="选中文字后点击 FeedNote 浮球，再选择“记”。"
        />
      ) : (
        <div className="memo-list">
          {memos.map((memo) => (
            <article key={memo.id} className="memo-card">
              <span className="memo-card-icon">
                <NotebookPen size={17} />
              </span>
              <p>{memo.content}</p>
              <button
                className="icon-button memo-edit-button"
                type="button"
                title="编辑备忘"
                aria-label="编辑备忘"
                onClick={() => onEdit(memo)}
              >
                <Pencil size={15} />
              </button>
              <footer>
                <span>{memo.sourceTitle || "未知来源"}</span>
                <span>{formatTime(memo.createdAt)}</span>
                <span
                  className={
                    memo.feishuSyncedAt ? "memo-synced" : "memo-pending"
                  }
                >
                  {memo.feishuSyncedAt ? "已同步飞书" : "等待同步"}
                </span>
              </footer>
            </article>
          ))}
        </div>
      )}
    </section>
  );
}

function PlansPage({
  plans,
  allCount,
  activeCount,
  view,
  tag,
  busy,
  onView,
  onTag,
  onEdit,
  onDone,
}: {
  plans: PlanItem[];
  allCount: number;
  activeCount: number;
  view: "active" | "all";
  tag: "all" | "面试";
  busy: Set<string>;
  onView: (value: "active" | "all") => void;
  onTag: (value: "all" | "面试") => void;
  onEdit: (plan: PlanItem) => void;
  onDone: (plan: PlanItem, done: boolean) => void;
}) {
  return (
    <section className="page plans-page">
      <div className="plans-toolbar">
        <span>
          {activeCount} 项进行中 · {allCount} 项全部
        </span>
        <div className="plans-toolbar-controls">
          <label className="plan-tag-filter">
            <Tag size={14} />
            <select
              value={tag}
              onChange={(e) => onTag(e.target.value as "all" | "面试")}
              aria-label="按标签筛选计划"
            >
              <option value="all">全部标签</option>
              <option value="面试">面试</option>
            </select>
          </label>
          <div className="plan-view-control" role="group" aria-label="计划范围">
            <button
              type="button"
              className={view === "active" ? "active" : ""}
              onClick={() => onView("active")}
            >
              进行中
            </button>
            <button
              type="button"
              className={view === "all" ? "active" : ""}
              onClick={() => onView("all")}
            >
              全部
            </button>
          </div>
        </div>
      </div>
      {plans.length === 0 ? (
        <Empty
          icon={CalendarClock}
          title={
            tag === "面试"
              ? "没有面试计划"
              : view === "active"
                ? "没有进行中的计划"
                : "还没有桌面计划"
          }
          text="从选区浮球选择“喂”后，识别出的待办会出现在这里。"
        />
      ) : (
        <div className="main-plan-list">
          {plans.map((plan) => (
            <article
              key={plan.id}
              className={`main-plan-card${plan.status === "done" ? " done" : ""}${!plan.scheduledAt ? " unscheduled" : ""}`}
            >
              <div className="main-plan-head">
                <div>
                  <div className="main-plan-meta">
                    <span className="plan-schedule">
                      <CalendarClock size={15} />
                      {formatPlanSchedule(plan.scheduledAt)}
                    </span>
                    {plan.tag && (
                      <span className="plan-tag">
                        <Tag size={12} />
                        {plan.tag}
                      </span>
                    )}
                  </div>
                  <h2>{plan.title}</h2>
                </div>
                <button
                  className="icon-button"
                  type="button"
                  title="编辑计划"
                  aria-label="编辑计划"
                  onClick={() => onEdit(plan)}
                >
                  <Pencil size={16} />
                </button>
              </div>
              <dl className="main-plan-fields">
                {plan.content && (
                  <>
                    <dt>内容</dt>
                    <dd>{plan.content}</dd>
                  </>
                )}
                {plan.details && plan.details !== plan.content && (
                  <>
                    <dt>详情</dt>
                    <dd>{plan.details}</dd>
                  </>
                )}
                {plan.linkUrl && (
                  <>
                    <dt>链接</dt>
                    <dd>
                      <button
                        type="button"
                        onClick={() => void openExternalLink(plan.linkUrl!)}
                      >
                        <span>{plan.linkUrl}</span>
                        <ExternalLink size={14} />
                      </button>
                    </dd>
                  </>
                )}
                {plan.notes && (
                  <>
                    <dt>注意</dt>
                    <dd>{plan.notes}</dd>
                  </>
                )}
                {plan.clarificationQuestion && !plan.scheduledAt && (
                  <>
                    <dt>待补充</dt>
                    <dd>{plan.clarificationQuestion}</dd>
                  </>
                )}
              </dl>
              <footer className="main-plan-footer">
                <span>{plan.sourceTitle || "未知来源"}</span>
                <span>更新于 {formatTime(plan.updatedAt)}</span>
                <button
                  className="plan-state-button"
                  type="button"
                  disabled={busy.has(plan.id)}
                  onClick={() => onDone(plan, plan.status !== "done")}
                >
                  {busy.has(plan.id) ? (
                    <LoaderCircle className="spin" size={15} />
                  ) : plan.status === "done" ? (
                    <RefreshCw size={15} />
                  ) : (
                    <Check size={15} />
                  )}
                  {plan.status === "done" ? "重新打开" : "标记完成"}
                </button>
              </footer>
            </article>
          ))}
        </div>
      )}
    </section>
  );
}

function SecretsPage({
  vault,
  secrets,
  total,
  search,
  password,
  confirm,
  busy,
  revealed,
  onSearch,
  onPassword,
  onConfirm,
  onAuth,
  onLock,
  onReveal,
  onCopy,
  onEdit,
  onDelete,
  onOpen,
}: {
  vault: { initialized: boolean; unlocked: boolean };
  secrets: SecretItem[];
  total: number;
  search: string;
  password: string;
  confirm: string;
  busy: boolean;
  revealed: Set<string>;
  onSearch: (value: string) => void;
  onPassword: (value: string) => void;
  onConfirm: (value: string) => void;
  onAuth: (initializing: boolean) => void;
  onLock: () => void;
  onReveal: (id: string) => void;
  onCopy: (secret: SecretItem) => void;
  onEdit: (secret: SecretItem) => void;
  onDelete: (secret: SecretItem) => void;
  onOpen: (url: string) => void;
}) {
  if (!vault.initialized)
    return (
      <section className="page secrets-page">
        <form
          className="vault-gate"
          onSubmit={(e) => {
            e.preventDefault();
            onAuth(true);
          }}
        >
          <span className="vault-gate-icon">
            <LockKeyhole size={24} />
          </span>
          <h2>设置秘密备忘录主密码</h2>
          <p>主密码不会上传，也没有找回通道。</p>
          <input
            value={password}
            onChange={(e) => onPassword(e.target.value)}
            type="password"
            minLength={6}
            maxLength={256}
            autoComplete="new-password"
            placeholder="至少 6 个字符"
          />
          <input
            value={confirm}
            onChange={(e) => onConfirm(e.target.value)}
            type="password"
            minLength={6}
            maxLength={256}
            autoComplete="new-password"
            placeholder="再次输入主密码"
          />
          <button
            className="primary-button"
            type="submit"
            disabled={busy || password.length < 6 || !confirm}
          >
            {busy ? (
              <LoaderCircle className="spin" size={16} />
            ) : (
              <LockKeyhole size={16} />
            )}
            创建保险箱
          </button>
        </form>
      </section>
    );
  if (!vault.unlocked)
    return (
      <section className="page secrets-page">
        <form
          className="vault-gate"
          onSubmit={(e) => {
            e.preventDefault();
            onAuth(false);
          }}
        >
          <span className="vault-gate-icon">
            <LockKeyhole size={24} />
          </span>
          <h2>秘密备忘录已锁定</h2>
          <input
            value={password}
            onChange={(e) => onPassword(e.target.value)}
            type="password"
            minLength={6}
            maxLength={256}
            autoComplete="current-password"
            autoFocus
            placeholder="输入主密码"
          />
          <button
            className="primary-button"
            type="submit"
            disabled={busy || password.length < 6}
          >
            {busy ? (
              <LoaderCircle className="spin" size={16} />
            ) : (
              <UnlockKeyhole size={16} />
            )}
            解锁
          </button>
        </form>
      </section>
    );
  return (
    <section className="page secrets-page">
      <div className="secrets-toolbar">
        <label className="search-control">
          <Search size={17} />
          <input
            value={search}
            onChange={(e) => onSearch(e.target.value)}
            type="search"
            placeholder="搜索名称、类型、账号或网站"
          />
        </label>
        <span>{total} 条</span>
        <button className="secondary-button" type="button" onClick={onLock}>
          <LockKeyhole size={16} />
          锁定
        </button>
      </div>
      {secrets.length === 0 ? (
        <Empty icon={LockKeyhole} title="没有匹配的秘密" text="" />
      ) : (
        <div className="secret-list">
          {secrets.map((secret) => (
            <article key={secret.id} className="secret-card">
              <div className="secret-card-head">
                <div>
                  <span className="secret-type">{secret.secretType}</span>
                  <h2>{secret.title}</h2>
                </div>
                <div className="secret-actions">
                  <button
                    className="icon-button"
                    type="button"
                    title="编辑"
                    aria-label="编辑秘密"
                    onClick={() => onEdit(secret)}
                  >
                    <Pencil size={16} />
                  </button>
                  <button
                    className="icon-button"
                    type="button"
                    title={revealed.has(secret.id) ? "隐藏" : "显示"}
                    aria-label={
                      revealed.has(secret.id) ? "隐藏秘密" : "显示秘密"
                    }
                    onClick={() => onReveal(secret.id)}
                  >
                    {revealed.has(secret.id) ? (
                      <EyeOff size={17} />
                    ) : (
                      <Eye size={17} />
                    )}
                  </button>
                  <button
                    className="icon-button"
                    type="button"
                    title="复制"
                    aria-label="复制秘密"
                    onClick={() => onCopy(secret)}
                  >
                    <Copy size={16} />
                  </button>
                  <button
                    className="icon-button danger"
                    type="button"
                    title="删除"
                    aria-label="删除秘密"
                    onClick={() => onDelete(secret)}
                  >
                    <Trash2 size={16} />
                  </button>
                </div>
              </div>
              <code
                className={`secret-value${revealed.has(secret.id) ? " revealed" : ""}`}
              >
                {revealed.has(secret.id)
                  ? secret.secretValue
                  : "••••••••••••••••"}
              </code>
              <dl className="secret-meta">
                {secret.account && (
                  <>
                    <dt>账号</dt>
                    <dd>{secret.account}</dd>
                  </>
                )}
                {secret.website && (
                  <>
                    <dt>网站</dt>
                    <dd>
                      <button
                        type="button"
                        onClick={() => onOpen(secret.website!)}
                      >
                        {secret.website}
                      </button>
                    </dd>
                  </>
                )}
                {secret.notes && (
                  <>
                    <dt>备注</dt>
                    <dd>{secret.notes}</dd>
                  </>
                )}
                <dt>更新</dt>
                <dd>
                  {formatTime(secret.updatedAt)}
                  {secret.feishuSyncedAt && (
                    <span className="secret-sync-mark">已同步</span>
                  )}
                </dd>
              </dl>
            </article>
          ))}
        </div>
      )}
    </section>
  );
}

function MemoryDrawer({
  detail,
  onClose,
}: {
  detail: MemoryDetail;
  onClose: () => void;
}) {
  return (
    <div className="drawer-layer">
      <button
        className="drawer-scrim"
        aria-label="关闭详情"
        onClick={onClose}
      />
      <aside className="detail-drawer">
        <header>
          <div>
            <span className="type-label">
              {typeLabel(detail.memory.memoryType)}
            </span>
            <h2>{detail.memory.title}</h2>
          </div>
          <button
            className="icon-button"
            type="button"
            title="关闭"
            aria-label="关闭"
            onClick={onClose}
          >
            <X size={20} />
          </button>
        </header>
        <div className="drawer-body">
          <section className="current-memory">
            <span className="eyebrow">当前理解</span>
            <p className="memory-summary">
              {detail.memory.summary || detail.memory.body}
            </p>
            <div className="source-trust">
              <ShieldCheck size={16} />
              来自 {detail.memory.sourceCount} 条原始记录
            </div>
          </section>
          <section className="version-section">
            <h3>版本时间线</h3>
            {detail.versions.map((version) => (
              <article key={version.id} className="version-item">
                <div
                  className={`version-dot${version.authorType !== "user" ? " ai" : ""}`}
                />
                <div>
                  <div className="version-head">
                    <strong>{version.changeReason}</strong>
                    <span>{formatTime(version.createdAt)}</span>
                  </div>
                  <p>{version.summary || version.body}</p>
                  <div className="version-meta">
                    <span>
                      {version.authorType === "user"
                        ? "用户原文"
                        : "AI 自动整理"}
                    </span>
                    {version.modelInfo && <span>{version.modelInfo}</span>}
                    <span>{version.sourceEventIds.length} 个来源</span>
                  </div>
                </div>
              </article>
            ))}
          </section>
        </div>
      </aside>
    </div>
  );
}

function PlanEditor({
  editor,
  busy,
  onChange,
  onClose,
  onSubmit,
}: {
  editor: PlanEditorForm;
  busy: boolean;
  onChange: (value: PlanEditorForm) => void;
  onClose: () => void;
  onSubmit: (event: FormEvent) => void;
}) {
  const set = <K extends keyof PlanEditorForm>(
    key: K,
    value: PlanEditorForm[K],
  ) => onChange({ ...editor, [key]: value });
  return (
    <div className="modal-layer">
      <button className="modal-scrim" aria-label="取消编辑" onClick={onClose} />
      <form
        className="secret-dialog plan-edit-dialog"
        role="dialog"
        aria-modal="true"
        onSubmit={onSubmit}
      >
        <header>
          <div>
            <span className="plan-editor-type">桌面计划</span>
            <h2>编辑计划</h2>
          </div>
          <button
            className="icon-button"
            type="button"
            title="关闭"
            aria-label="关闭"
            onClick={onClose}
          >
            <X size={18} />
          </button>
        </header>
        <div className="plan-edit-grid">
          <label className="wide-field">
            <span>标题</span>
            <input
              value={editor.title}
              onChange={(e) => set("title", e.target.value)}
              maxLength={80}
              required
              autoFocus
            />
          </label>
          <label>
            <span>时间</span>
            <input
              value={editor.scheduledLocal}
              onChange={(e) => set("scheduledLocal", e.target.value)}
              type="datetime-local"
            />
          </label>
          <label>
            <span>飞书提醒</span>
            <select
              value={editor.reminderMinutesBefore}
              onChange={(e) =>
                set("reminderMinutesBefore", Number(e.target.value))
              }
            >
              {[0, 5, 15, 30, 60, 180, 1440].map((n) => (
                <option key={n} value={n}>
                  {n === 0
                    ? "准时"
                    : n < 60
                      ? `提前 ${n} 分钟`
                      : n === 60
                        ? "提前 1 小时"
                        : n === 180
                          ? "提前 3 小时"
                          : "提前 1 天"}
                </option>
              ))}
            </select>
          </label>
          <label>
            <span>标签</span>
            <select
              value={editor.tag}
              onChange={(e) => set("tag", e.target.value)}
            >
              <option value="">无标签</option>
              <option value="面试">面试</option>
            </select>
          </label>
          <label>
            <span>内容</span>
            <input
              value={editor.content}
              onChange={(e) => set("content", e.target.value)}
              maxLength={60}
              required
            />
          </label>
          <label className="wide-field">
            <span>详情</span>
            <textarea
              value={editor.details}
              onChange={(e) => set("details", e.target.value)}
              rows={4}
              maxLength={4000}
              required
            />
          </label>
          <label className="wide-field">
            <span>链接</span>
            <input
              value={editor.linkUrl}
              onChange={(e) => set("linkUrl", e.target.value)}
              type="url"
              maxLength={2000}
              placeholder="https://"
            />
          </label>
          <label className="wide-field">
            <span>注意事项</span>
            <textarea
              value={editor.notes}
              onChange={(e) => set("notes", e.target.value)}
              rows={3}
              maxLength={500}
            />
          </label>
        </div>
        <DialogActions
          busy={busy}
          disabled={
            !editor.title.trim() ||
            !editor.content.trim() ||
            !editor.details.trim()
          }
          onClose={onClose}
        />
      </form>
    </div>
  );
}
function MemoEditor({
  editor,
  busy,
  onChange,
  onClose,
  onSubmit,
}: {
  editor: MemoEditorForm;
  busy: boolean;
  onChange: (value: MemoEditorForm) => void;
  onClose: () => void;
  onSubmit: (event: FormEvent) => void;
}) {
  return (
    <div className="modal-layer">
      <button className="modal-scrim" aria-label="取消编辑" onClick={onClose} />
      <form
        className="secret-dialog memo-edit-dialog"
        role="dialog"
        aria-modal="true"
        onSubmit={onSubmit}
      >
        <header>
          <div>
            <span className="secret-type">备忘录</span>
            <h2>编辑备忘</h2>
          </div>
          <button
            className="icon-button"
            type="button"
            title="关闭"
            aria-label="关闭"
            onClick={onClose}
          >
            <X size={18} />
          </button>
        </header>
        <label className="memo-edit-field">
          <span>内容</span>
          <textarea
            value={editor.content}
            onChange={(e) => onChange({ ...editor, content: e.target.value })}
            rows={8}
            maxLength={4000}
            required
            autoFocus
          />
          <small>{editor.content.length} / 4000</small>
        </label>
        <DialogActions
          busy={busy}
          disabled={!editor.content.trim()}
          onClose={onClose}
        />
      </form>
    </div>
  );
}
function SecretEditor({
  editor,
  busy,
  onChange,
  onClose,
  onSubmit,
}: {
  editor: SecretEditorForm;
  busy: boolean;
  onChange: (value: SecretEditorForm) => void;
  onClose: () => void;
  onSubmit: (event: FormEvent) => void;
}) {
  const set = (key: keyof SecretEditorForm, value: string) =>
    onChange({ ...editor, [key]: value });
  return (
    <div className="modal-layer">
      <button className="modal-scrim" aria-label="取消编辑" onClick={onClose} />
      <form
        className="secret-dialog secret-edit-dialog"
        role="dialog"
        aria-modal="true"
        onSubmit={onSubmit}
      >
        <header>
          <div>
            <span className="secret-type">秘密备忘录</span>
            <h2>编辑秘密</h2>
          </div>
          <button
            className="icon-button"
            type="button"
            title="关闭"
            aria-label="关闭"
            onClick={onClose}
          >
            <X size={18} />
          </button>
        </header>
        <div className="secret-edit-grid">
          <label>
            <span>名称</span>
            <input
              value={editor.title}
              onChange={(e) => set("title", e.target.value)}
              maxLength={120}
              required
              autoFocus
            />
          </label>
          <label>
            <span>类型</span>
            <input
              value={editor.secretType}
              onChange={(e) => set("secretType", e.target.value)}
              list="secret-type-options"
              maxLength={40}
              required
            />
          </label>
          <datalist id="secret-type-options">
            {["密码", "API Key", "私钥", "恢复码", "令牌", "其他"].map(
              (item) => (
                <option key={item} value={item} />
              ),
            )}
          </datalist>
          <label className="wide-field">
            <span>秘密值</span>
            <textarea
              value={editor.secretValue}
              onChange={(e) => set("secretValue", e.target.value)}
              rows={3}
              maxLength={100000}
              required
            />
          </label>
          <label>
            <span>账号</span>
            <input
              value={editor.account}
              onChange={(e) => set("account", e.target.value)}
              maxLength={300}
            />
          </label>
          <label>
            <span>网站</span>
            <input
              value={editor.website}
              onChange={(e) => set("website", e.target.value)}
              type="url"
              maxLength={2000}
              placeholder="https://"
            />
          </label>
          <label className="wide-field">
            <span>备注</span>
            <textarea
              value={editor.notes}
              onChange={(e) => set("notes", e.target.value)}
              rows={3}
              maxLength={1000}
            />
          </label>
        </div>
        <DialogActions
          busy={busy}
          disabled={
            !editor.title.trim() ||
            !editor.secretType.trim() ||
            !editor.secretValue.trim()
          }
          onClose={onClose}
        />
      </form>
    </div>
  );
}
function DialogActions({
  busy,
  disabled,
  onClose,
}: {
  busy: boolean;
  disabled: boolean;
  onClose: () => void;
}) {
  return (
    <div className="dialog-actions">
      <button className="secondary-button" type="button" onClick={onClose}>
        取消
      </button>
      <button
        className="primary-button"
        type="submit"
        disabled={busy || disabled}
      >
        {busy ? (
          <LoaderCircle className="spin" size={16} />
        ) : (
          <Check size={16} />
        )}
        保存
      </button>
    </div>
  );
}

function SettingsPage({
  settings,
  feishu,
  secretStatus,
  vaultUnlocked,
  aiCheck,
  pushCheck,
  planSync,
  secretSync,
  onSetting,
  onSave,
  onTestAi,
  onTestPush,
  onPlanSync,
  onSecretSync,
  onOpen,
  onExport,
}: {
  settings: AppSettings;
  feishu: FeishuSyncStatus;
  secretStatus: FeishuSecretStatus;
  vaultUnlocked: boolean;
  aiCheck: { state: RequestState; message: string };
  pushCheck: { state: RequestState; message: string };
  planSync: { state: RequestState; message: string };
  secretSync: { state: RequestState; message: string };
  onSetting: <K extends keyof AppSettings>(
    key: K,
    value: AppSettings[K],
  ) => void;
  onSave: () => void;
  onTestAi: () => void;
  onTestPush: () => void;
  onPlanSync: () => void;
  onSecretSync: () => void;
  onOpen: (url?: string) => Promise<void>;
  onExport: () => void;
}) {
  return (
    <section className="page settings-page">
      <SettingSection
        title="应用启动"
        description="登录 Windows 后在后台启动 FeedNote。"
        toggle={
          <Toggle
            checked={settings.launchAtLogin}
            label={settings.launchAtLogin ? "开机自启动" : "手动启动"}
            onChange={(value) => onSetting("launchAtLogin", value)}
          />
        }
      >
        <Actions onSave={onSave} />
      </SettingSection>
      <SettingSection
        title="自动理解"
        description="保存后由 Memory Engine 自动分类、归并和摘要。"
        toggle={
          <Toggle
            checked={settings.aiEnabled}
            label={settings.aiEnabled ? "已开启" : "已关闭"}
            onChange={(value) => onSetting("aiEnabled", value)}
          />
        }
      >
        <div className={`form-grid${!settings.aiEnabled ? " muted" : ""}`}>
          <ReadField label="密钥文件" value="data\secrets.env" />
          <TextField
            label="LLM 地址"
            value={settings.llmEndpoint}
            disabled={!settings.aiEnabled}
            onChange={(value) => onSetting("llmEndpoint", value)}
          />
          <TextField
            label="LLM 模型"
            value={settings.llmModel}
            disabled={!settings.aiEnabled}
            onChange={(value) => onSetting("llmModel", value)}
          />
          <TextField
            label="Embedding 地址"
            value={settings.embeddingEndpoint}
            disabled={!settings.aiEnabled}
            onChange={(value) => onSetting("embeddingEndpoint", value)}
          />
          <TextField
            label="Embedding 模型"
            value={settings.embeddingModel}
            disabled={!settings.aiEnabled}
            onChange={(value) => onSetting("embeddingModel", value)}
          />
          <label>
            <span>向量维度</span>
            <select
              value={settings.embeddingDimensions}
              disabled={!settings.aiEnabled}
              onChange={(e) =>
                onSetting("embeddingDimensions", Number(e.target.value))
              }
            >
              {[256, 512, 1024, 2048].map((n) => (
                <option key={n} value={n}>
                  {n}
                </option>
              ))}
            </select>
          </label>
        </div>
        <Actions onSave={onSave}>
          <button
            className="secondary-button"
            type="button"
            disabled={!settings.aiEnabled || aiCheck.state === "checking"}
            onClick={onTestAi}
          >
            {aiCheck.state === "checking" ? (
              <LoaderCircle className="spin" size={16} />
            ) : (
              <RefreshCw size={16} />
            )}
            测试连接
          </button>
        </Actions>
        {aiCheck.message && (
          <p className={`connection-result ${aiCheck.state}`}>
            {aiCheck.message}
          </p>
        )}
      </SettingSection>
      <SettingSection
        title="手机提醒"
        description="通过 ntfy 或自有 Webhook 接收计划提醒。"
        toggle={
          <Toggle
            checked={settings.mobilePushEnabled}
            label={settings.mobilePushEnabled ? "已开启" : "已关闭"}
            onChange={(value) => onSetting("mobilePushEnabled", value)}
          />
        }
      >
        <div
          className={`form-grid${!settings.mobilePushEnabled ? " muted" : ""}`}
        >
          <ReadField label="推送配置" value="data\secrets.env" />
          <label>
            <span>推送通道</span>
            <select
              value={settings.mobilePushProvider}
              disabled={!settings.mobilePushEnabled}
              onChange={(e) =>
                onSetting(
                  "mobilePushProvider",
                  e.target.value as AppSettings["mobilePushProvider"],
                )
              }
            >
              <option value="ntfy">ntfy 手机 App</option>
              <option value="webhook">通用 Webhook</option>
            </select>
          </label>
          <label>
            <span>提前提醒</span>
            <select
              value={settings.mobileReminderMinutes}
              disabled={!settings.mobilePushEnabled}
              onChange={(e) =>
                onSetting(
                  "mobileReminderMinutes",
                  Number(
                    e.target.value,
                  ) as AppSettings["mobileReminderMinutes"],
                )
              }
            >
              {[0, 5, 15, 30, 60].map((n) => (
                <option key={n} value={n}>
                  {n ? `提前 ${n} 分钟` : "准时"}
                </option>
              ))}
            </select>
          </label>
        </div>
        <Actions onSave={onSave}>
          <button
            className="secondary-button"
            type="button"
            disabled={
              !settings.mobilePushEnabled || pushCheck.state === "checking"
            }
            onClick={onTestPush}
          >
            {pushCheck.state === "checking" ? (
              <LoaderCircle className="spin" size={16} />
            ) : (
              <Send size={16} />
            )}
            发送测试提醒
          </button>
        </Actions>
        {pushCheck.message && (
          <p className={`connection-result ${pushCheck.state}`}>
            {pushCheck.message}
          </p>
        )}
      </SettingSection>
      <SettingSection
        title="飞书计划表"
        description="维护真正的待办，并双向同步已有计划的完成状态。"
        toggle={
          <Toggle
            checked={settings.feishuSyncEnabled}
            label={settings.feishuSyncEnabled ? "已开启" : "已关闭"}
            onChange={(value) => onSetting("feishuSyncEnabled", value)}
          />
        }
      >
        <div
          className={`form-grid${!settings.feishuSyncEnabled ? " muted" : ""}`}
        >
          <ReadField label="应用凭证" value="data\secrets.env" />
          <ReadField
            label="连接状态"
            value={feishu.configured ? "凭证已配置" : "等待配置凭证"}
          />
          <ReadField
            label="同步队列"
            value={`${feishu.pendingPlans} 条待同步`}
          />
        </div>
        <Actions onSave={onSave}>
          <button
            className="secondary-button"
            type="button"
            disabled={
              !settings.feishuSyncEnabled || planSync.state === "syncing"
            }
            onClick={onPlanSync}
          >
            {planSync.state === "syncing" ? (
              <LoaderCircle className="spin" size={16} />
            ) : (
              <Cloud size={16} />
            )}
            初始化并同步
          </button>
          {feishu.spreadsheetUrl && (
            <button
              className="secondary-button"
              type="button"
              onClick={() => void onOpen(feishu.spreadsheetUrl)}
            >
              <Table2 size={16} />
              打开表格
              <ExternalLink size={14} />
            </button>
          )}
        </Actions>
        {planSync.message ? (
          <p className={`connection-result ${planSync.state}`}>
            {planSync.message}
          </p>
        ) : (
          feishu.lastError && (
            <p className="connection-result error">{feishu.lastError}</p>
          )
        )}
      </SettingSection>
      <SettingSection
        title="飞书待办提醒"
        description="创建个人飞书任务，双向同步标题、时间和完成状态，并在开始前 3 小时提醒。"
        toggle={
          <Toggle
            checked={settings.feishuTaskRemindersEnabled}
            label={settings.feishuTaskRemindersEnabled ? "已开启" : "已关闭"}
            onChange={(value) => onSetting("feishuTaskRemindersEnabled", value)}
          />
        }
      >
        <div
          className={`form-grid${!settings.feishuTaskRemindersEnabled ? " muted" : ""}`}
        >
          <ReadField label="任务负责人" value="已配置的飞书负责人" />
          <ReadField label="提醒时间" value="计划开始前 3 小时" />
          <ReadField
            label="同步队列"
            value={`${feishu.pendingTaskReminders} 条待同步`}
          />
        </div>
        <Actions onSave={onSave}>
          <button
            className="secondary-button"
            type="button"
            disabled={
              !settings.feishuTaskRemindersEnabled ||
              planSync.state === "syncing"
            }
            onClick={onPlanSync}
          >
            {planSync.state === "syncing" ? (
              <LoaderCircle className="spin" size={16} />
            ) : (
              <Clock3 size={16} />
            )}
            立即同步
          </button>
        </Actions>
        {feishu.taskReminderError && (
          <p className="connection-result error">{feishu.taskReminderError}</p>
        )}
      </SettingSection>
      <SettingSection
        title="飞书秘密表"
        description="把已藏内容单向写入独立表格，便于在手机上查看。"
        toggle={
          <Toggle
            checked={settings.feishuSecretEnabled}
            label={settings.feishuSecretEnabled ? "已开启" : "已关闭"}
            onChange={(value) => onSetting("feishuSecretEnabled", value)}
          />
        }
      >
        <p className="security-warning">
          <CircleAlert size={17} />
          开启后，秘密值会以明文写入飞书。飞书访问权限不是本地加密，也不提供端到端保密保证。
        </p>
        <div
          className={`form-grid${!settings.feishuSecretEnabled ? " muted" : ""}`}
        >
          <ReadField label="同步方向" value="FeedNote → 飞书（单向）" />
          <ReadField
            label="本地保险箱"
            value={vaultUnlocked ? "已解锁" : "需先解锁"}
          />
          <ReadField
            label="同步队列"
            value={`${secretStatus.pendingSecrets} 条待同步`}
          />
        </div>
        <Actions onSave={onSave}>
          <button
            className="secondary-button"
            type="button"
            disabled={
              !settings.feishuSecretEnabled ||
              !vaultUnlocked ||
              secretSync.state === "syncing"
            }
            onClick={onSecretSync}
          >
            {secretSync.state === "syncing" ? (
              <LoaderCircle className="spin" size={16} />
            ) : (
              <Cloud size={16} />
            )}
            初始化并同步
          </button>
          {secretStatus.spreadsheetUrl && (
            <button
              className="secondary-button"
              type="button"
              onClick={() => void onOpen(secretStatus.spreadsheetUrl)}
            >
              <Table2 size={16} />
              打开秘密表
              <ExternalLink size={14} />
            </button>
          )}
        </Actions>
        {secretSync.message ? (
          <p className={`connection-result ${secretSync.state}`}>
            {secretSync.message}
          </p>
        ) : (
          secretStatus.lastError && (
            <p className="connection-result error">{secretStatus.lastError}</p>
          )
        )}
      </SettingSection>
      <SettingSection
        title="数据导出"
        description="导出原始投喂和当前记忆为带版本号的 JSON。"
        toggle={<Archive size={22} />}
      >
        <button className="secondary-button" type="button" onClick={onExport}>
          <Download size={16} />
          选择位置并导出
        </button>
      </SettingSection>
      <div className="hard-boundaries">
        <h2>
          <ShieldCheck size={19} />
          不可逾越的边界
        </h2>
        <ul>
          <li>原始投喂不会被 AI 修改或覆盖。</li>
          <li>模型输出必须经过 Memory Engine 校验后才能自动写入。</li>
          <li>仅将当前输入和必要的候选记忆发送至你授权的模型服务。</li>
          <li>
            模型与飞书服务凭证只从 data\secrets.env
            读取，不进入数据库、前端或导出文件。
          </li>
          <li>不监听剪贴板、键盘，也不扫描用户目录。</li>
          <li>
            <LockKeyhole size={14} />
            “藏”的原文先在本地加密，绝不发送给 LLM；模型只接收已替换为 [SECRET]
            的周边文本。
          </li>
          <li>
            <Smartphone size={14} />
            手机推送默认关闭，只发送计划卡片字段。
          </li>
          <li>
            <Cloud size={14} />
            飞书通道独立开关；计划表只回读已有计划。
          </li>
          <li>
            <CircleAlert size={14} />
            秘密表单向写入且为明文，安全边界由飞书账号和文档权限承担。
          </li>
          <li>
            <Clock3 size={14} />
            飞书待办只回读已有任务的标题、时间和完成状态。
          </li>
        </ul>
      </div>
    </section>
  );
}
function SettingSection({
  title,
  description,
  toggle,
  children,
}: {
  title: string;
  description: string;
  toggle: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <div className="settings-section">
      <div className="settings-heading">
        <div>
          <h2>{title}</h2>
          <p>{description}</p>
        </div>
        {toggle}
      </div>
      {children}
    </div>
  );
}
function Toggle({
  checked,
  label,
  onChange,
}: {
  checked: boolean;
  label: string;
  onChange: (value: boolean) => void;
}) {
  return (
    <label className="toggle-control">
      <input
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
        type="checkbox"
      />
      <span aria-hidden="true" />
      <strong>{label}</strong>
    </label>
  );
}
function ReadField({ label, value }: { label: string; value: string }) {
  return (
    <label>
      <span>{label}</span>
      <input value={value} type="text" readOnly />
    </label>
  );
}
function TextField({
  label,
  value,
  disabled,
  onChange,
}: {
  label: string;
  value: string;
  disabled: boolean;
  onChange: (value: string) => void;
}) {
  return (
    <label>
      <span>{label}</span>
      <input
        value={value}
        onChange={(e) => onChange(e.target.value)}
        type="text"
        disabled={disabled}
      />
    </label>
  );
}
function Actions({
  onSave,
  children,
}: {
  onSave: () => void;
  children?: React.ReactNode;
}) {
  return (
    <div className="settings-actions">
      {children}
      <button className="primary-button" type="button" onClick={onSave}>
        <Check size={16} />
        保存设置
      </button>
    </div>
  );
}
