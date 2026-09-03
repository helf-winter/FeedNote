import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type DragEvent,
  type MouseEvent,
  type PointerEvent,
} from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
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
  Tag,
} from "lucide-react";
import { canAcceptTextDrop, droppedPlainText } from "../dropText";
import {
  listPlans,
  openExternalLink,
  openMainWindow,
  prepareDragCapture,
  resolvePlanTime,
  setPlanDone,
  showPlanDockMenu,
  togglePlanDock,
  type PlanItem,
} from "../api";
import { checkForOnlineUpdate } from "../updater";

const OPACITY_KEY = "feednote.plan-dock.opacity";
const WIDTH_KEY = "feednote.plan-dock.width";
const HEIGHT_KEY = "feednote.plan-dock.height";
const MIN_WIDTH = 320;
const MIN_HEIGHT = 300;
const storedNumber = (key: string, fallback: number, minimum: number) => {
  const value = Number.parseInt(localStorage.getItem(key) ?? "", 10);
  return Number.isFinite(value) && value >= minimum ? value : fallback;
};

export default function PlanDock() {
  const [expanded, setExpanded] = useState(false);
  const [plans, setPlans] = useState<PlanItem[]>([]);
  const [tagFilter, setTagFilter] = useState<"all" | "面试">("all");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [answers, setAnswers] = useState<Record<string, string>>({});
  const [opacity, setOpacity] = useState(() =>
    Math.min(100, Math.max(45, storedNumber(OPACITY_KEY, 91, 45))),
  );
  const [textDropActive, setTextDropActive] = useState(false);
  const [dropMessage, setDropMessage] = useState("");
  const expandedRef = useRef(expanded);
  const collapsedPointer = useRef<{ id: number; x: number; y: number }>();
  const dragDepth = useRef(0);
  const dropTimer = useRef<number>();
  const visiblePlans = useMemo(
    () =>
      tagFilter === "all"
        ? plans
        : plans.filter((plan) => plan.tag === tagFilter),
    [plans, tagFilter],
  );
  const dockStyle = {
    "--dock-opacity": String(opacity / 100),
  } as CSSProperties;

  useEffect(() => {
    expandedRef.current = expanded;
  }, [expanded]);
  async function refresh() {
    setPlans(await listPlans());
  }
  useEffect(() => {
    let disposed = false;
    const stops: Array<() => void> = [];
    void refresh();
    void listen("plans-changed", () => void refresh()).then((stop) =>
      disposed ? stop() : stops.push(stop),
    );
    void listen(
      "online-update-requested",
      () => void checkForOnlineUpdate(),
    ).then((stop) => (disposed ? stop() : stops.push(stop)));
    void getCurrentWindow()
      .onResized(({ payload }) => {
        if (expandedRef.current) {
          if (payload.width >= MIN_WIDTH)
            localStorage.setItem(WIDTH_KEY, String(Math.round(payload.width)));
          if (payload.height >= MIN_HEIGHT)
            localStorage.setItem(
              HEIGHT_KEY,
              String(Math.round(payload.height)),
            );
        }
      })
      .then((stop) => (disposed ? stop() : stops.push(stop)));
    void checkForOnlineUpdate();
    const timer = window.setInterval(
      () => void checkForOnlineUpdate(),
      6 * 60 * 60 * 1000,
    );
    return () => {
      disposed = true;
      stops.forEach((stop) => stop());
      window.clearInterval(timer);
      if (dropTimer.current) window.clearTimeout(dropTimer.current);
    };
  }, []);

  function showDropMessage(message: string) {
    setDropMessage(message);
    if (dropTimer.current) window.clearTimeout(dropTimer.current);
    dropTimer.current = window.setTimeout(() => setDropMessage(""), 2400);
  }
  function enterDrop(event: DragEvent) {
    event.preventDefault();
    dragDepth.current += 1;
    const accepted = canAcceptTextDrop(event.dataTransfer);
    setTextDropActive(accepted);
    event.dataTransfer.dropEffect = accepted ? "copy" : "none";
  }
  function overDrop(event: DragEvent) {
    event.preventDefault();
    const accepted = canAcceptTextDrop(event.dataTransfer);
    setTextDropActive(accepted);
    event.dataTransfer.dropEffect = accepted ? "copy" : "none";
  }
  function leaveDrop() {
    dragDepth.current = Math.max(0, dragDepth.current - 1);
    if (!dragDepth.current) setTextDropActive(false);
  }
  async function receiveDrop(event: DragEvent) {
    event.preventDefault();
    dragDepth.current = 0;
    setTextDropActive(false);
    if (!canAcceptTextDrop(event.dataTransfer))
      return showDropMessage("只支持拖入文字");
    const text = droppedPlainText(event.dataTransfer);
    if (!text) return showDropMessage("只支持拖入文字");
    try {
      await prepareDragCapture(text);
    } catch (reason) {
      showDropMessage(String(reason));
    }
  }
  async function toggle() {
    if (expanded) {
      const size = await getCurrentWindow().innerSize();
      if (size.width >= MIN_WIDTH)
        localStorage.setItem(WIDTH_KEY, String(Math.round(size.width)));
      if (size.height >= MIN_HEIGHT)
        localStorage.setItem(HEIGHT_KEY, String(Math.round(size.height)));
    }
    const next = await togglePlanDock(
      storedNumber(WIDTH_KEY, 380, MIN_WIDTH),
      storedNumber(HEIGHT_KEY, 520, MIN_HEIGHT),
    );
    setExpanded(next);
    if (next) await refresh();
  }
  async function complete(plan: PlanItem) {
    await setPlanDone(plan.id, true);
    await refresh();
  }
  async function answer(plan: PlanItem) {
    const value = answers[plan.id]?.trim();
    if (!value || loading) return;
    setLoading(true);
    setError("");
    try {
      await resolvePlanTime(plan.id, value);
      setAnswers((current) => ({ ...current, [plan.id]: "" }));
      await refresh();
    } catch (reason) {
      setError(String(reason));
    } finally {
      setLoading(false);
    }
  }
  async function showMenu() {
    collapsedPointer.current = undefined;
    try {
      await showPlanDockMenu();
    } catch (reason) {
      showDropMessage(String(reason));
    }
  }
  function pointerDown(event: PointerEvent<HTMLButtonElement>) {
    if (event.button !== 0) return;
    collapsedPointer.current = {
      id: event.pointerId,
      x: event.screenX,
      y: event.screenY,
    };
    event.currentTarget.setPointerCapture(event.pointerId);
  }
  async function pointerMove(event: PointerEvent<HTMLButtonElement>) {
    const start = collapsedPointer.current;
    if (
      !start ||
      event.pointerId !== start.id ||
      Math.hypot(event.screenX - start.x, event.screenY - start.y) < 6
    )
      return;
    collapsedPointer.current = undefined;
    if (event.currentTarget.hasPointerCapture(event.pointerId))
      event.currentTarget.releasePointerCapture(event.pointerId);
    await getCurrentWindow().startDragging();
  }
  async function pointerUp(event: PointerEvent<HTMLButtonElement>) {
    if (collapsedPointer.current?.id !== event.pointerId) return;
    collapsedPointer.current = undefined;
    if (event.currentTarget.hasPointerCapture(event.pointerId))
      event.currentTarget.releasePointerCapture(event.pointerId);
    await toggle();
  }
  function planLink(plan: PlanItem) {
    const extracted = plan.details.match(/https?:\/\/[^\s<>"']+/i)?.[0];
    const candidate = (plan.linkUrl || extracted)?.replace(
      /[),.;，。；）]+$/,
      "",
    );
    if (!candidate) return;
    try {
      const url = new URL(candidate);
      return ["http:", "https:"].includes(url.protocol)
        ? url.toString()
        : undefined;
    } catch {
      return;
    }
  }
  function linkLabel(plan: PlanItem) {
    const url = planLink(plan);
    if (!url) return "";
    const parsed = new URL(url);
    const label = `${parsed.hostname}${parsed.pathname === "/" ? "" : parsed.pathname}`;
    return label.length > 42 ? `${label.slice(0, 39)}...` : label;
  }
  function compactContent(plan: PlanItem) {
    if (plan.content?.trim()) return plan.content.trim();
    const known = plan.details.match(
      /AI\s*面|笔试|面试|电话面|视频面|会议|答辩|考试|复试/i,
    )?.[0];
    if (known) return known.replace(/\s+/g, "");
    const sentence = plan.details.split(/[。！？!\n]/)[0].trim();
    return sentence.length > 36 ? `${sentence.slice(0, 33)}...` : sentence;
  }
  function formatTime(timestamp?: number) {
    return timestamp
      ? new Intl.DateTimeFormat("zh-CN", {
          month: "numeric",
          day: "numeric",
          weekday: "short",
          hour: "2-digit",
          minute: "2-digit",
        }).format(timestamp)
      : "等待安排时间";
  }
  const dropHandlers = {
    onDragEnter: enterDrop,
    onDragOver: overDrop,
    onDragLeave: leaveDrop,
    onDrop: (event: DragEvent) => void receiveDrop(event),
  };

  if (!expanded)
    return (
      <button
        className={`dock-tab${textDropActive ? " text-drop-active" : ""}`}
        title={dropMessage || "FeedNote"}
        type="button"
        aria-label="展开计划"
        onPointerDown={pointerDown}
        onPointerMove={(e) => void pointerMove(e)}
        onPointerUp={(e) => void pointerUp(e)}
        onPointerCancel={() => {
          collapsedPointer.current = undefined;
        }}
        onKeyDown={(event) => {
          if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            void toggle();
          }
        }}
        onContextMenu={(e) => {
          e.preventDefault();
          void showMenu();
        }}
        {...dropHandlers}
      >
        <span className="dock-logo">
          <CalendarClock size={20} />
        </span>
        {plans.length > 0 && (
          <span className="plan-count">{Math.min(plans.length, 99)}</span>
        )}
      </button>
    );

  return (
    <aside
      className={`plan-dock-panel${textDropActive ? " text-drop-active" : ""}`}
      aria-label="桌面计划"
      style={dockStyle}
      onContextMenu={(e) => {
        e.preventDefault();
        void showMenu();
      }}
      {...dropHandlers}
    >
      {(["West", "East", "North", "South"] as const).map((direction) => (
        <span
          key={direction}
          className={`dock-resize-handle dock-resize-handle-${({ West: "left", East: "right", North: "top", South: "bottom" } as const)[direction]}`}
          aria-hidden="true"
          onMouseDown={(event) => {
            event.stopPropagation();
            event.preventDefault();
            if (event.button === 0)
              void getCurrentWindow().startResizeDragging(direction);
          }}
        />
      ))}
      <header
        onMouseDown={(event: MouseEvent) => {
          if (
            event.button === 0 &&
            !(event.target as HTMLElement).closest("button, input")
          )
            void getCurrentWindow().startDragging();
        }}
      >
        <div className="dock-heading">
          <span className="dock-heading-icon">
            <CalendarClock size={17} />
          </span>
          <div className="dock-heading-content">
            <div className="dock-title-row">
              <span>桌面计划</span>
              <strong>{visiblePlans.length}</strong>
            </div>
            <label
              className="dock-opacity-control"
              title="调节面板透明度"
              onMouseDown={(e) => e.stopPropagation()}
            >
              <Droplet size={11} aria-hidden="true" />
              <input
                value={opacity}
                onChange={(e) => {
                  const next = Number(e.target.value);
                  setOpacity(next);
                  localStorage.setItem(OPACITY_KEY, String(next));
                }}
                type="range"
                min="45"
                max="100"
                step="1"
                aria-label="面板透明度"
              />
              <output>{opacity}%</output>
            </label>
          </div>
        </div>
        <nav>
          <button
            type="button"
            aria-label="打开记忆库"
            onClick={() => void openMainWindow()}
          >
            <NotebookText size={17} />
          </button>
          <button type="button" aria-label="收起" onClick={() => void toggle()}>
            <PanelRightClose size={18} />
          </button>
        </nav>
      </header>
      <div className="dock-tag-filter" role="group" aria-label="按标签筛选计划">
        <button
          type="button"
          className={tagFilter === "all" ? "active" : ""}
          onClick={() => setTagFilter("all")}
        >
          全部
        </button>
        <button
          type="button"
          className={tagFilter === "面试" ? "active" : ""}
          onClick={() => setTagFilter("面试")}
        >
          面试
        </button>
      </div>
      <div className="plan-list">
        {!visiblePlans.length && (
          <div className="empty-plans">
            <span>
              <CalendarClock size={25} />
            </span>
            <p>{tagFilter === "面试" ? "暂无面试计划" : "暂无待办计划"}</p>
          </div>
        )}
        {visiblePlans.map((plan) => (
          <article key={plan.id} className="plan-card">
            <button
              className="complete-plan"
              type="button"
              title="标记完成"
              aria-label="标记完成"
              onClick={() => void complete(plan)}
            >
              <Check size={14} />
            </button>
            <div className="plan-card-body">
              <div className="dock-plan-meta">
                <time className={!plan.scheduledAt ? "pending" : ""}>
                  {formatTime(plan.scheduledAt)}
                </time>
                {plan.tag && (
                  <span>
                    <Tag size={10} />
                    {plan.tag}
                  </span>
                )}
              </div>
              <h2>{plan.title}</h2>
              <div className="plan-fields">
                <div className="plan-field">
                  <span>内容</span>
                  <p>{compactContent(plan)}</p>
                </div>
                {planLink(plan) && (
                  <div className="plan-field">
                    <span>链接</span>
                    <button
                      className="plan-link"
                      type="button"
                      onClick={() => {
                        const url = planLink(plan);
                        if (url)
                          void openExternalLink(url).catch((reason) =>
                            setError(String(reason)),
                          );
                      }}
                    >
                      <Link2 size={12} />
                      <span>{linkLabel(plan)}</span>
                      <ExternalLink size={12} />
                    </button>
                  </div>
                )}
                {plan.notes && (
                  <div className="plan-field plan-notes">
                    <span>注意</span>
                    <p>
                      <AlertCircle size={12} />
                      {plan.notes}
                    </p>
                  </div>
                )}
              </div>
              {plan.status === "needs_clarification" && (
                <form
                  className="dock-time-answer"
                  onSubmit={(e) => {
                    e.preventDefault();
                    void answer(plan);
                  }}
                >
                  <label htmlFor={`answer-${plan.id}`}>
                    {plan.clarificationQuestion}
                  </label>
                  <div>
                    <input
                      id={`answer-${plan.id}`}
                      value={answers[plan.id] ?? ""}
                      onChange={(e) =>
                        setAnswers((current) => ({
                          ...current,
                          [plan.id]: e.target.value,
                        }))
                      }
                      maxLength={500}
                      placeholder="补充具体日期和时间"
                    />
                    <button
                      type="submit"
                      title="确认时间"
                      aria-label="确认时间"
                      disabled={loading}
                    >
                      {loading ? (
                        <LoaderCircle className="spin" size={15} />
                      ) : (
                        <Check size={15} />
                      )}
                    </button>
                  </div>
                </form>
              )}
            </div>
          </article>
        ))}
      </div>
      {(dropMessage || error) && (
        <p className="dock-error">{dropMessage || error}</p>
      )}
    </aside>
  );
}
