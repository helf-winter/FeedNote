import { useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  ArrowLeft,
  ArrowRight,
  BookOpen,
  BrainCircuit,
  LoaderCircle,
  Search,
  Sparkles,
  X,
} from "lucide-react";
import {
  dismissMemoRecall,
  getMemoRecallState,
  markMemoRecallIrrelevant,
  openMemoRecallTarget,
  recallMemos,
  type MemoRecallSurfaceState,
} from "../api";

export default function MemoRecallSurface() {
  const [state, setState] = useState<MemoRecallSurfaceState>({ mode: "input" });
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const matches = state.result?.matches ?? [];
  const current = matches[Math.min(index, Math.max(0, matches.length - 1))];
  const hitLabel = useMemo(
    () =>
      current
        ? [...current.matchedEntities, ...current.matchedTerms]
            .slice(0, 4)
            .join(" · ")
        : "",
    [current],
  );

  useEffect(() => {
    let disposed = false;
    let stop: (() => void) | undefined;
    void getMemoRecallState().then((next) => {
      if (!disposed) setState(next);
    });
    void listen<MemoRecallSurfaceState>(
      "memo-recall-changed",
      ({ payload }) => {
        setState(payload);
        setIndex(0);
        setError("");
        if (payload.mode === "input") setQuery("");
      },
    ).then((unlisten) => {
      if (disposed) unlisten();
      else stop = unlisten;
    });
    const keydown = (event: KeyboardEvent) => {
      if (event.key === "Escape") void dismissMemoRecall();
    };
    window.addEventListener("keydown", keydown);
    return () => {
      disposed = true;
      stop?.();
      window.removeEventListener("keydown", keydown);
    };
  }, []);

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    if (!query.trim() || busy) return;
    setBusy(true);
    setError("");
    try {
      const result = await recallMemos(query.trim());
      setState({ mode: "result", result });
      setIndex(0);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  }

  async function irrelevant() {
    if (!current || busy) return;
    setBusy(true);
    try {
      await markMemoRecallIrrelevant(current.memo.id, [
        ...current.matchedEntities,
        ...current.matchedTerms,
      ]);
    } catch (reason) {
      setError(String(reason));
      setBusy(false);
    }
  }

  return (
    <section className="memo-recall-surface" aria-label="召回相关备忘">
      <header>
        <span className="recall-mark">
          <Sparkles size={16} />
        </span>
        <div>
          <strong>相关备忘</strong>
          <span>本地匹配，不调用 AI</span>
        </div>
        <button
          type="button"
          title="关闭"
          aria-label="关闭"
          onClick={() => void dismissMemoRecall()}
        >
          <X size={17} />
        </button>
      </header>

      {state.mode === "input" ? (
        <form className="recall-input" onSubmit={submit}>
          <BrainCircuit size={25} />
          <strong>召回哪个主题？</strong>
          <p>输入项目名和一个主题词，例如“FeedNote 移动端”。</p>
          <div>
            <input
              autoFocus
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              maxLength={4000}
              placeholder="项目名 + 主题"
            />
            <button
              type="submit"
              disabled={busy || !query.trim()}
              title="查找"
              aria-label="查找"
            >
              {busy ? (
                <LoaderCircle className="spin" size={16} />
              ) : (
                <Search size={16} />
              )}
            </button>
          </div>
        </form>
      ) : current ? (
        <div className="recall-result">
          <div className="recall-result-meta">
            <span>命中 {state.result?.total ?? matches.length} 条</span>
            {matches.length > 1 && (
              <nav aria-label="切换召回结果">
                <button
                  type="button"
                  disabled={index === 0}
                  onClick={() => setIndex((value) => value - 1)}
                  title="上一条"
                >
                  <ArrowLeft size={14} />
                </button>
                <span>
                  {index + 1}/{matches.length}
                </span>
                <button
                  type="button"
                  disabled={index >= matches.length - 1}
                  onClick={() => setIndex((value) => value + 1)}
                  title="下一条"
                >
                  <ArrowRight size={14} />
                </button>
              </nav>
            )}
          </div>
          <article>
            <p>{current.memo.content}</p>
            <footer>
              <span>{current.memo.sourceTitle || "未知来源"}</span>
              <span>{Math.round(current.score * 100)}% 相关</span>
            </footer>
          </article>
          <p className="recall-hits">关联：{hitLabel}</p>
          <div className="recall-actions">
            <button
              type="button"
              className="recall-secondary"
              disabled={busy}
              onClick={() => void irrelevant()}
            >
              不相关
            </button>
            <button
              type="button"
              className="recall-primary"
              onClick={() => void openMemoRecallTarget()}
            >
              <BookOpen size={15} />
              查看备忘录
            </button>
          </div>
        </div>
      ) : (
        <div className="recall-empty">
          <Search size={25} />
          <strong>没有找到相关备忘</strong>
          <p>需要同时命中一个项目或实体，以及另一个主题词。</p>
          <button type="button" onClick={() => setState({ mode: "input" })}>
            换个主题
          </button>
        </div>
      )}
      {error && <p className="recall-error">{error}</p>}
    </section>
  );
}
