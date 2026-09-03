import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  CalendarClock,
  Check,
  LockKeyhole,
  LoaderCircle,
  NotebookPen,
  Send,
  Undo2,
  X,
} from "lucide-react";
import {
  commitCapture,
  discardCapture,
  getCapturePreview,
  getVaultStatus,
  initializeVault,
  recordMemoCapture,
  resolvePlanTime,
  stashCapture,
  undoSecretStash,
  unlockVault,
  type PlanItem,
  type SelectionSnapshot,
  type VaultStatus,
} from "../api";

export default function CaptureMenu() {
  const [snapshot, setSnapshot] = useState<SelectionSnapshot | null>(null);
  const [plan, setPlan] = useState<PlanItem | null>(null);
  const [answer, setAnswer] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [resultMessage, setResultMessage] = useState("");
  const [vaultStatus, setVaultStatus] = useState<VaultStatus>({
    initialized: false,
    unlocked: false,
    secretCount: 0,
  });
  const [vaultAuthVisible, setVaultAuthVisible] = useState(false);
  const [masterPassword, setMasterPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [stashedSecretId, setStashedSecretId] = useState("");
  const [activeAction, setActiveAction] = useState<
    "feed" | "memo" | "stash" | ""
  >("");
  const closeTimer = useRef<number>();

  function clearCloseTimer() {
    if (closeTimer.current !== undefined)
      window.clearTimeout(closeTimer.current);
    closeTimer.current = undefined;
  }
  function reset() {
    clearCloseTimer();
    setSnapshot(null);
    setPlan(null);
    setAnswer("");
    setError("");
    setResultMessage("");
    setVaultAuthVisible(false);
    setMasterPassword("");
    setConfirmPassword("");
    setStashedSecretId("");
  }
  function closeAfter(delay: number) {
    clearCloseTimer();
    closeTimer.current = window.setTimeout(() => {
      void discardCapture().finally(reset);
    }, delay);
  }

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void Promise.all([getCapturePreview(), getVaultStatus()]).then(
      ([preview, vault]) => {
        if (!disposed) {
          setSnapshot(preview);
          setVaultStatus(vault);
        }
      },
    );
    void listen<SelectionSnapshot>("capture-prepared", ({ payload }) => {
      setSnapshot(payload);
      setPlan(null);
      setAnswer("");
      setError("");
      setResultMessage("");
      setVaultAuthVisible(false);
      setMasterPassword("");
      setConfirmPassword("");
      setStashedSecretId("");
    }).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    });
    return () => {
      disposed = true;
      unlisten?.();
      clearCloseTimer();
    };
  }, []);

  async function reject() {
    if (busy) return;
    clearCloseTimer();
    await discardCapture();
    reset();
  }
  async function feed() {
    if (busy || !snapshot) return;
    setBusy(true);
    setActiveAction("feed");
    setError("");
    try {
      const result = await commitCapture();
      setPlan(result.plan ?? null);
      if (!result.needsClarification) {
        setResultMessage(result.message);
        closeAfter(1400);
      }
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
      setActiveAction("");
    }
  }
  async function remember() {
    if (busy || !snapshot) return;
    setBusy(true);
    setActiveAction("memo");
    setError("");
    try {
      const result = await recordMemoCapture();
      setResultMessage(result.message);
      setSnapshot(null);
      closeAfter(1400);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
      setActiveAction("");
    }
  }
  async function stashNow() {
    setBusy(true);
    setActiveAction("stash");
    setError("");
    try {
      const result = await stashCapture();
      setStashedSecretId(result.secretId);
      setResultMessage(result.message);
      setSnapshot(null);
      closeAfter(7600);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
      setActiveAction("");
    }
  }
  async function stash() {
    if (busy || !snapshot) return;
    if (!vaultStatus.unlocked) {
      setVaultAuthVisible(true);
      return;
    }
    await stashNow();
  }
  async function authorizeVaultAndStash() {
    if (!masterPassword || busy) return;
    if (!vaultStatus.initialized && masterPassword !== confirmPassword) {
      setError("两次主密码不一致");
      return;
    }
    setBusy(true);
    setActiveAction("stash");
    setError("");
    try {
      const status = vaultStatus.initialized
        ? await unlockVault(masterPassword)
        : await initializeVault(masterPassword);
      setVaultStatus(status);
      setMasterPassword("");
      setConfirmPassword("");
      setVaultAuthVisible(false);
    } catch (reason) {
      setError(String(reason));
      setBusy(false);
      setActiveAction("");
      return;
    }
    setBusy(false);
    setActiveAction("");
    await stashNow();
  }
  async function undoStash() {
    if (!stashedSecretId || busy) return;
    setBusy(true);
    try {
      await undoSecretStash(stashedSecretId);
      setResultMessage("已撤销");
      setStashedSecretId("");
      closeAfter(800);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  }
  async function submitTime(event: React.FormEvent) {
    event.preventDefault();
    if (busy || !plan || !answer.trim()) return;
    setBusy(true);
    setError("");
    try {
      const result = await resolvePlanTime(plan.id, answer.trim());
      setPlan(result.plan ?? null);
      setAnswer("");
      if (!result.needsClarification) {
        setResultMessage(result.message);
        closeAfter(1000);
      }
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="capture-popover" aria-label="处理选区">
      <header>
        <div className="surface-mark">
          <CalendarClock size={17} />
        </div>
        <strong>
          {resultMessage ? "处理完成" : plan ? "安排时间" : "处理选区"}
        </strong>
        <button
          className="icon-button"
          type="button"
          title="关闭"
          aria-label="关闭"
          onClick={() => void reject()}
        >
          <X size={17} />
        </button>
      </header>
      {resultMessage ? (
        <div className="capture-success">
          <span>
            <Check size={20} />
          </span>
          <strong>{resultMessage}</strong>
          {stashedSecretId && (
            <button
              className="undo-stash"
              type="button"
              disabled={busy}
              onClick={() => void undoStash()}
            >
              <Undo2 size={13} />
              撤销
            </button>
          )}
        </div>
      ) : plan?.status === "needs_clarification" ? (
        <>
          <p className="question">{plan.clarificationQuestion}</p>
          <form className="time-answer" onSubmit={submitTime}>
            <input
              value={answer}
              onChange={(e) => setAnswer(e.target.value)}
              maxLength={500}
              autoFocus
              placeholder="例如：明天下午 3 点"
            />
            <button
              type="submit"
              title="确认时间"
              aria-label="确认时间"
              disabled={busy || !answer.trim()}
            >
              {busy ? (
                <LoaderCircle className="spin" size={16} />
              ) : (
                <Check size={16} />
              )}
            </button>
          </form>
        </>
      ) : vaultAuthVisible ? (
        <form
          className="vault-auth"
          onSubmit={(e) => {
            e.preventDefault();
            void authorizeVaultAndStash();
          }}
        >
          <p>
            {vaultStatus.initialized
              ? "输入主密码以解锁秘密备忘录"
              : "设置独立主密码，丢失后无法从本机恢复"}
          </p>
          <input
            value={masterPassword}
            onChange={(e) => setMasterPassword(e.target.value)}
            type="password"
            minLength={6}
            maxLength={256}
            autoFocus
            autoComplete="off"
            placeholder="主密码（至少 6 个字符）"
          />
          {!vaultStatus.initialized && (
            <input
              value={confirmPassword}
              onChange={(e) => setConfirmPassword(e.target.value)}
              type="password"
              minLength={6}
              maxLength={256}
              autoComplete="off"
              placeholder="再次输入主密码"
            />
          )}
          <button
            className="feed-button"
            type="submit"
            disabled={busy || masterPassword.length < 6}
          >
            {busy ? (
              <LoaderCircle className="spin" size={15} />
            ) : (
              <span>
                {vaultStatus.initialized ? "解锁并藏入" : "创建并藏入"}
              </span>
            )}
          </button>
        </form>
      ) : (
        <>
          <blockquote>{snapshot?.selectedText || "正在读取选区..."}</blockquote>
          <div className="feed-actions">
            <button
              className="feed-button"
              type="button"
              title="交给 AI 处理"
              disabled={busy || !snapshot}
              onClick={() => void feed()}
            >
              {busy && activeAction === "feed" ? (
                <LoaderCircle className="spin" size={15} />
              ) : (
                <>
                  <Send size={14} />
                  <span>喂</span>
                </>
              )}
            </button>
            <button
              className="memo-button"
              type="button"
              title="记入普通备忘录并同步飞书"
              disabled={busy || !snapshot}
              onClick={() => void remember()}
            >
              {busy && activeAction === "memo" ? (
                <LoaderCircle className="spin" size={15} />
              ) : (
                <>
                  <NotebookPen size={14} />
                  <span>记</span>
                </>
              )}
            </button>
            <button
              className="stash-button"
              type="button"
              title="藏入加密秘密备忘录"
              disabled={busy || !snapshot}
              onClick={() => void stash()}
            >
              {busy && activeAction === "stash" ? (
                <LoaderCircle className="spin" size={15} />
              ) : (
                <>
                  <LockKeyhole size={14} />
                  <span>藏</span>
                </>
              )}
            </button>
          </div>
        </>
      )}
      {error && <p className="surface-error">{error}</p>}
    </section>
  );
}
