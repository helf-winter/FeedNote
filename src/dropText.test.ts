import { describe, expect, it } from "vitest";
import { canAcceptTextDrop, droppedPlainText, type DragTextSource } from "./dropText";

function transfer(values: Record<string, string>, types = Object.keys(values)): DragTextSource {
  return {
    types,
    getData: (format) => values[format] ?? "",
  };
}

describe("dragged text", () => {
  it("accepts and preserves Unicode plain text", () => {
    const source = transfer({ "text/plain": "  微信拖拽内容\n第二行  " });
    expect(canAcceptTextDrop(source)).toBe(true);
    expect(droppedPlainText(source)).toBe("微信拖拽内容\n第二行");
  });

  it("extracts visible text from HTML without script or style content", () => {
    const source = transfer({
      "text/html": "<p>面试通知</p><style>secret</style><script>ignore()</script>",
    });
    expect(droppedPlainText(source)).toBe("面试通知");
  });

  it("rejects a file-only drag", () => {
    const source = transfer({}, ["Files"]);
    expect(canAcceptTextDrop(source)).toBe(false);
    expect(droppedPlainText(source)).toBe("");
  });

  it("rejects mixed file and text payloads to avoid leaking file paths", () => {
    const source = transfer({ "text/uri-list": "file:///D:/private.png" }, [
      "Files",
      "text/uri-list",
    ]);
    expect(canAcceptTextDrop(source)).toBe(false);
  });
});
