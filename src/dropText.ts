export interface DragTextSource {
  readonly types: readonly string[];
  getData(format: string): string;
}

export function canAcceptTextDrop(transfer: DragTextSource | null): boolean {
  if (!transfer) return false;
  const types = Array.from(transfer.types, (type) => type.toLowerCase());
  if (types.length === 0) return true;
  if (types.includes("files")) return false;
  return types.some((type) => type === "text" || type.startsWith("text/"));
}

export function droppedPlainText(transfer: DragTextSource | null): string {
  if (!transfer) return "";
  const plain = transfer.getData("text/plain") || transfer.getData("Text");
  if (plain.trim()) return plain.trim();
  const html = transfer.getData("text/html");
  if (!html.trim()) return "";
  const document = new DOMParser().parseFromString(html, "text/html");
  document
    .querySelectorAll("script, style, noscript")
    .forEach((element) => element.remove());
  return (document.body.textContent ?? "").trim();
}
