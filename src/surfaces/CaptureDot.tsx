import { useState } from "react";
import { LoaderCircle, Sparkles } from "lucide-react";
import { prepareCapture } from "../api";

export default function CaptureDot() {
  const [busy, setBusy] = useState(false);
  async function openCapture() {
    if (busy) return;
    setBusy(true);
    try {
      await prepareCapture();
    } finally {
      setBusy(false);
    }
  }
  return (
    <button
      className={`capture-dot${busy ? " busy" : ""}`}
      type="button"
      aria-label="投喂给 FeedNote"
      onClick={() => void openCapture()}
    >
      <span className="capture-dot__core" aria-hidden="true">
        {busy ? (
          <LoaderCircle
            className="capture-dot__spinner"
            size={12}
            strokeWidth={2.4}
          />
        ) : (
          <Sparkles size={12} strokeWidth={2.4} />
        )}
      </span>
    </button>
  );
}
