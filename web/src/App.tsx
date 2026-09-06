import { useMemo, useState, type FormEvent } from "react";
import {
  CalendarDays,
  Check,
  CheckCircle2,
  ChevronRight,
  CircleAlert,
  Clock3,
  ExternalLink,
  Filter,
  Link2,
  LogOut,
  Pencil,
  Plus,
  RefreshCw,
  Tag,
  Trash2,
  X,
} from "lucide-react";
import type { CloudPlan, PlanInput } from "../../shared/plans";
import {
  useCreatePlanMutation,
  useDeletePlanMutation,
  useLogoutMutation,
  usePlansQuery,
  useSessionQuery,
  useUpdatePlanMutation,
} from "./api";

type View = "active" | "today" | "upcoming" | "done";

const errorText = (error: unknown) => {
  if (error && typeof error === "object" && "data" in error) {
    const data = (error as { data?: { message?: string } }).data;
    if (data?.message) return data.message;
  }
  return "操作失败，请稍后再试";
};

const requiresLogin = (error: unknown) => {
  if (!error || typeof error !== "object" || !("status" in error)) return false;
  return (error as { status?: number }).status === 401;
};

const dateKey = (timestamp: number) => new Date(timestamp).toDateString();
const formatTime = (timestamp?: number) =>
  timestamp
    ? new Date(timestamp).toLocaleString("zh-CN", {
        month: "short",
        day: "numeric",
        weekday: "short",
        hour: "2-digit",
        minute: "2-digit",
      })
    : "等待安排";

const emptyInput = (): PlanInput => ({
  title: "",
  details: "",
  content: "",
  linkUrl: "",
  notes: "",
  scheduledAt: null,
  status: "scheduled",
  reminderMinutesBefore: 180,
  tags: [],
  sourceTitle: "FeedNote 网页",
});

function toLocalInput(timestamp?: number) {
  if (!timestamp) return "";
  const date = new Date(timestamp - new Date().getTimezoneOffset() * 60_000);
  return date.toISOString().slice(0, 16);
}

export function App() {
  const session = useSessionQuery();
  if (session.isLoading) return <LoadingScreen />;
  if (session.isError || !session.data) return <LoginScreen />;
  return <PlanWorkspace user={session.data.user} />;
}

function LoadingScreen() {
  return (
    <main className="center-screen">
      <RefreshCw className="spin" size={24} />
      <span>正在连接 FeedNote</span>
    </main>
  );
}

function LoginScreen() {
  return (
    <main className="login-shell">
      <section className="login-content">
        <img src="/feednote.png" alt="FeedNote" className="login-logo" />
        <p className="eyebrow">FEEDNOTE CLOUD</p>
        <h1>把计划带到每一块屏幕</h1>
        <p className="login-copy">
          通过飞书身份访问你的云计划。浏览器不会保存飞书应用密钥。
        </p>
        <a className="primary-action" href="/api/auth/login">
          使用飞书登录 <ChevronRight size={18} />
        </a>
        <p className="access-note">当前版本仅向已配置的飞书账号开放</p>
      </section>
      <aside className="login-preview" aria-hidden="true">
        <div className="preview-date">9月 · 本周计划</div>
        <div className="preview-line">
          <span>09:30</span>
          <strong>产品方案评审</strong>
        </div>
        <div className="preview-line">
          <span>14:00</span>
          <strong>前端技术面试</strong>
        </div>
        <div className="preview-line muted">
          <span>17:20</span>
          <strong>取快递</strong>
        </div>
      </aside>
    </main>
  );
}

function PlanWorkspace({
  user,
}: {
  user: { name: string; avatarUrl?: string };
}) {
  const plansQuery = usePlansQuery(undefined, { pollingInterval: 30_000 });
  const [logout] = useLogoutMutation();
  const [view, setView] = useState<View>("active");
  const [tag, setTag] = useState("all");
  const [editing, setEditing] = useState<CloudPlan | "new">();
  const [toast, setToast] = useState("");
  const plans = plansQuery.data ?? [];
  const tags = useMemo(
    () => Array.from(new Set(plans.flatMap((plan) => plan.tags))),
    [plans],
  );
  const filtered = useMemo(() => {
    const today = dateKey(Date.now());
    return plans.filter((plan) => {
      if (tag !== "all" && !plan.tags.includes(tag)) return false;
      if (view === "done") return plan.status === "done";
      if (plan.status === "done") return false;
      if (view === "today")
        return Boolean(plan.scheduledAt && dateKey(plan.scheduledAt) === today);
      if (view === "upcoming")
        return Boolean(plan.scheduledAt && dateKey(plan.scheduledAt) !== today);
      return true;
    });
  }, [plans, tag, view]);
  const todayCount = plans.filter(
    (plan) =>
      plan.status !== "done" &&
      plan.scheduledAt &&
      dateKey(plan.scheduledAt) === dateKey(Date.now()),
  ).length;
  const waitingCount = plans.filter(
    (plan) => plan.status === "needs_clarification",
  ).length;

  async function signOut() {
    await logout();
    window.location.reload();
  }

  return (
    <div className="workspace">
      <aside className="sidebar">
        <div className="brand">
          <img src="/feednote.png" alt="" />
          <span>FeedNote</span>
        </div>
        <nav aria-label="计划视图">
          <NavButton
            active={view === "active"}
            onClick={() => setView("active")}
            icon={CalendarDays}
            label="全部计划"
          />
          <NavButton
            active={view === "today"}
            onClick={() => setView("today")}
            icon={Clock3}
            label="今天"
            count={todayCount}
          />
          <NavButton
            active={view === "upcoming"}
            onClick={() => setView("upcoming")}
            icon={ChevronRight}
            label="接下来"
          />
          <NavButton
            active={view === "done"}
            onClick={() => setView("done")}
            icon={CheckCircle2}
            label="已完成"
          />
        </nav>
        <div className="sidebar-foot">
          <div className="user">
            <span className="avatar">
              {user.avatarUrl ? (
                <img src={user.avatarUrl} alt="" />
              ) : (
                user.name.slice(0, 1)
              )}
            </span>
            <span>{user.name}</span>
          </div>
          <button
            className="icon-button"
            title="退出登录"
            onClick={() => void signOut()}
          >
            <LogOut size={18} />
          </button>
        </div>
      </aside>

      <main className="main-panel">
        <header className="topbar">
          <div>
            <p className="eyebrow">PERSONAL SCHEDULE</p>
            <h1>我的计划</h1>
          </div>
          <div className="top-actions">
            <button
              className="icon-button"
              title="刷新"
              onClick={() => void plansQuery.refetch()}
            >
              <RefreshCw
                size={19}
                className={plansQuery.isFetching ? "spin" : ""}
              />
            </button>
            <button
              className="primary-action compact"
              onClick={() => setEditing("new")}
            >
              <Plus size={18} />
              新计划
            </button>
          </div>
        </header>

        <section className="summary-strip" aria-label="计划概览">
          <div>
            <strong>
              {plans.filter((plan) => plan.status !== "done").length}
            </strong>
            <span>进行中</span>
          </div>
          <div>
            <strong>{todayCount}</strong>
            <span>今天</span>
          </div>
          <div className={waitingCount ? "warning" : ""}>
            <strong>{waitingCount}</strong>
            <span>待安排</span>
          </div>
          <div className="sync-state">
            <span className="sync-dot" />
            多维表格已连接
          </div>
        </section>

        <section className="filterbar">
          <div className="mobile-views">
            {(["active", "today", "upcoming", "done"] as View[]).map((item) => (
              <button
                key={item}
                className={view === item ? "active" : ""}
                onClick={() => setView(item)}
              >
                {
                  {
                    active: "全部",
                    today: "今天",
                    upcoming: "接下来",
                    done: "已完成",
                  }[item]
                }
              </button>
            ))}
          </div>
          <label>
            <Filter size={16} />
            <span>标签</span>
            <select
              value={tag}
              onChange={(event) => setTag(event.target.value)}
            >
              <option value="all">全部标签</option>
              {tags.map((item) => (
                <option value={item} key={item}>
                  {item}
                </option>
              ))}
            </select>
          </label>
          <span className="result-count">{filtered.length} 项</span>
        </section>

        {plansQuery.isError ? (
          <EmptyState
            icon={CircleAlert}
            title="无法读取计划"
            detail={
              requiresLogin(plansQuery.error)
                ? "飞书权限已更新，需要重新登录以完成授权。"
                : errorText(plansQuery.error)
            }
            action={
              requiresLogin(plansQuery.error) ? "重新登录飞书" : "重新加载"
            }
            onAction={() => {
              if (requiresLogin(plansQuery.error)) {
                window.location.assign("/api/auth/login");
                return;
              }
              void plansQuery.refetch();
            }}
          />
        ) : filtered.length === 0 ? (
          <EmptyState
            icon={CheckCircle2}
            title="这里暂时没有计划"
            detail="新计划会直接写入你的飞书多维表格。"
            action="创建计划"
            onAction={() => setEditing("new")}
          />
        ) : (
          <section className="plan-list">
            {filtered.map((plan) => (
              <PlanRow
                key={plan.recordId}
                plan={plan}
                onEdit={() => setEditing(plan)}
                onToast={setToast}
              />
            ))}
          </section>
        )}
      </main>

      {editing && (
        <PlanEditor
          plan={editing === "new" ? undefined : editing}
          onClose={() => setEditing(undefined)}
          onSaved={(message) => {
            setEditing(undefined);
            setToast(message);
          }}
        />
      )}
      {toast && (
        <div className="toast" role="status">
          {toast}
          <button title="关闭" onClick={() => setToast("")}>
            <X size={15} />
          </button>
        </div>
      )}
    </div>
  );
}

function NavButton({
  active,
  onClick,
  icon: Icon,
  label,
  count,
}: {
  active: boolean;
  onClick: () => void;
  icon: typeof CalendarDays;
  label: string;
  count?: number;
}) {
  return (
    <button
      className={active ? "nav-button active" : "nav-button"}
      onClick={onClick}
    >
      <Icon size={19} />
      <span>{label}</span>
      {count ? <b>{count}</b> : null}
    </button>
  );
}

function PlanRow({
  plan,
  onEdit,
  onToast,
}: {
  plan: CloudPlan;
  onEdit: () => void;
  onToast: (value: string) => void;
}) {
  const [update, updateState] = useUpdatePlanMutation();
  async function toggleDone() {
    try {
      await update({
        recordId: plan.recordId,
        input: {
          version: plan.version,
          status:
            plan.status === "done"
              ? plan.scheduledAt
                ? "scheduled"
                : "needs_clarification"
              : "done",
        },
      }).unwrap();
      onToast(plan.status === "done" ? "计划已恢复" : "计划已完成");
    } catch (error) {
      onToast(errorText(error));
    }
  }
  return (
    <article className={plan.status === "done" ? "plan-row done" : "plan-row"}>
      <button
        className="check-button"
        title={plan.status === "done" ? "恢复计划" : "标记完成"}
        onClick={() => void toggleDone()}
        disabled={updateState.isLoading}
      >
        {plan.status === "done" && <Check size={17} />}
      </button>
      <div className="plan-body">
        <div className="plan-meta">
          <time className={!plan.scheduledAt ? "waiting" : ""}>
            {formatTime(plan.scheduledAt)}
          </time>
          {plan.tags.map((item) => (
            <span className="tag" key={item}>
              <Tag size={12} />
              {item}
            </span>
          ))}
        </div>
        <h2>{plan.title}</h2>
        {(plan.content || plan.details) && (
          <p>{plan.content || plan.details}</p>
        )}
        <div className="plan-footer">
          {plan.linkUrl && (
            <a href={plan.linkUrl} target="_blank" rel="noreferrer">
              <Link2 size={14} />
              打开链接
              <ExternalLink size={13} />
            </a>
          )}
          {plan.notes && (
            <span className="note">
              <CircleAlert size={14} />
              {plan.notes}
            </span>
          )}
        </div>
      </div>
      <button
        className="icon-button edit-button"
        title="编辑计划"
        onClick={onEdit}
      >
        <Pencil size={17} />
      </button>
    </article>
  );
}

function EmptyState({
  icon: Icon,
  title,
  detail,
  action,
  onAction,
}: {
  icon: typeof CircleAlert;
  title: string;
  detail: string;
  action: string;
  onAction: () => void;
}) {
  return (
    <section className="empty-state">
      <Icon size={28} />
      <h2>{title}</h2>
      <p>{detail}</p>
      <button className="secondary-action" onClick={onAction}>
        {action}
      </button>
    </section>
  );
}

function PlanEditor({
  plan,
  onClose,
  onSaved,
}: {
  plan?: CloudPlan;
  onClose: () => void;
  onSaved: (message: string) => void;
}) {
  const [create, createState] = useCreatePlanMutation();
  const [update, updateState] = useUpdatePlanMutation();
  const [remove, removeState] = useDeletePlanMutation();
  const [form, setForm] = useState<PlanInput>(() =>
    plan
      ? {
          title: plan.title,
          details: plan.details,
          content: plan.content,
          linkUrl: plan.linkUrl ?? "",
          notes: plan.notes ?? "",
          scheduledAt: plan.scheduledAt ?? null,
          status: plan.status,
          reminderMinutesBefore: plan.reminderMinutesBefore,
          tags: plan.tags.filter((tag): tag is "面试" => tag === "面试"),
          sourceTitle: plan.sourceTitle,
        }
      : emptyInput(),
  );
  const [scheduledLocal, setScheduledLocal] = useState(
    toLocalInput(plan?.scheduledAt),
  );
  const [error, setError] = useState("");
  const busy =
    createState.isLoading || updateState.isLoading || removeState.isLoading;
  const set = <K extends keyof PlanInput>(key: K, value: PlanInput[K]) =>
    setForm((current) => ({ ...current, [key]: value }));

  async function submit(event: FormEvent) {
    event.preventDefault();
    setError("");
    const scheduledAt = scheduledLocal
      ? new Date(scheduledLocal).getTime()
      : null;
    const status: PlanInput["status"] =
      form.status === "done"
        ? "done"
        : scheduledAt
          ? "scheduled"
          : "needs_clarification";
    const input: PlanInput = {
      ...form,
      scheduledAt,
      status,
    };
    if (!input.title.trim()) return setError("请填写计划标题");
    try {
      if (plan)
        await update({
          recordId: plan.recordId,
          input: { ...input, version: plan.version },
        }).unwrap();
      else await create(input).unwrap();
      onSaved(plan ? "计划已更新" : "计划已创建");
    } catch (cause) {
      setError(errorText(cause));
    }
  }

  async function deleteCurrent() {
    if (!plan || !window.confirm("删除后计划将从各端隐藏，确定继续吗？"))
      return;
    try {
      await remove({ recordId: plan.recordId, version: plan.version }).unwrap();
      onSaved("计划已删除");
    } catch (cause) {
      setError(errorText(cause));
    }
  }

  return (
    <div
      className="modal-backdrop"
      onMouseDown={(event) => event.target === event.currentTarget && onClose()}
    >
      <form className="editor" onSubmit={(event) => void submit(event)}>
        <header>
          <div>
            <p className="eyebrow">PLAN DETAILS</p>
            <h2>{plan ? "编辑计划" : "创建计划"}</h2>
          </div>
          <button
            type="button"
            className="icon-button"
            title="关闭"
            onClick={onClose}
          >
            <X size={20} />
          </button>
        </header>
        <div className="editor-grid">
          <label className="wide">
            <span>标题</span>
            <input
              value={form.title}
              onChange={(event) => set("title", event.target.value)}
              maxLength={200}
              autoFocus
            />
          </label>
          <label>
            <span>时间</span>
            <input
              type="datetime-local"
              value={scheduledLocal}
              onChange={(event) => {
                setScheduledLocal(event.target.value);
                if (!event.target.value) set("status", "needs_clarification");
                else if (form.status === "needs_clarification")
                  set("status", "scheduled");
              }}
            />
          </label>
          <label>
            <span>状态</span>
            <select
              value={form.status}
              onChange={(event) =>
                set("status", event.target.value as PlanInput["status"])
              }
            >
              <option value="scheduled">已安排</option>
              <option value="needs_clarification">待安排</option>
              <option value="done">已完成</option>
            </select>
          </label>
          <label>
            <span>提醒</span>
            <select
              value={form.reminderMinutesBefore}
              onChange={(event) =>
                set("reminderMinutesBefore", Number(event.target.value))
              }
            >
              <option value={0}>准时提醒</option>
              <option value={30}>提前 30 分钟</option>
              <option value={60}>提前 1 小时</option>
              <option value={180}>提前 3 小时</option>
              <option value={1440}>提前 1 天</option>
            </select>
          </label>
          <label>
            <span>标签</span>
            <select
              value={form.tags[0] ?? ""}
              onChange={(event) =>
                set("tags", event.target.value === "面试" ? ["面试"] : [])
              }
            >
              <option value="">无标签</option>
              <option value="面试">面试</option>
            </select>
          </label>
          <label className="wide">
            <span>内容</span>
            <textarea
              value={form.content}
              onChange={(event) => set("content", event.target.value)}
              rows={3}
            />
          </label>
          <label className="wide">
            <span>详情</span>
            <textarea
              value={form.details}
              onChange={(event) => set("details", event.target.value)}
              rows={3}
            />
          </label>
          <label className="wide">
            <span>链接</span>
            <input
              type="url"
              value={form.linkUrl ?? ""}
              onChange={(event) => set("linkUrl", event.target.value)}
              placeholder="https://"
            />
          </label>
          <label className="wide">
            <span>注意事项</span>
            <textarea
              value={form.notes ?? ""}
              onChange={(event) => set("notes", event.target.value)}
              rows={2}
            />
          </label>
        </div>
        {error && (
          <p className="form-error">
            <CircleAlert size={15} />
            {error}
          </p>
        )}
        <footer>
          {plan ? (
            <button
              type="button"
              className="danger-action"
              disabled={busy}
              onClick={() => void deleteCurrent()}
            >
              <Trash2 size={17} />
              删除
            </button>
          ) : (
            <span />
          )}
          <div>
            <button
              type="button"
              className="secondary-action"
              onClick={onClose}
            >
              取消
            </button>
            <button className="primary-action compact" disabled={busy}>
              {busy ? (
                <RefreshCw className="spin" size={17} />
              ) : (
                <Check size={17} />
              )}
              保存
            </button>
          </div>
        </footer>
      </form>
    </div>
  );
}
