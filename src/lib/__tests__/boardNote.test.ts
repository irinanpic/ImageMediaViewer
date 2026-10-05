import { describe, expect, it } from "vitest";
import type { BoardNote, UpdateBoardNotePayload } from "../../types/board";

/**
 * ムードボード付箋メモの複数行テキスト処理およびペイロード整合性テスト
 */
describe("BoardNote multi-line text persistence", () => {
  it("改行を含む複数行テキストがUpdateBoardNotePayloadとして正しくシリアライズされること", () => {
    const multilineText = "1行目のメモ\n2行目のメモ（指示事項）\n3行目のメモ（カラーコード: #fef08a）";
    const payload: UpdateBoardNotePayload = {
      id: 1,
      text: multilineText,
    };

    // JSONシリアライズ/デシリアライズのラウンドトリップ検証
    const jsonStr = JSON.stringify(payload);
    expect(jsonStr).toContain("\\n");

    const parsed = JSON.parse(jsonStr) as UpdateBoardNotePayload;
    expect(parsed.text).toBe(multilineText);
    expect(parsed.text?.split("\n").length).toBe(3);
  });

  it("Windows改行コード CRLF を含むテキストも保持されること", () => {
    const crlfText = "ヘッダー行\r\n詳細行1\r\n詳細行2";
    const payload: UpdateBoardNotePayload = {
      id: 2,
      text: crlfText,
    };

    const jsonStr = JSON.stringify(payload);
    const parsed = JSON.parse(jsonStr) as UpdateBoardNotePayload;
    expect(parsed.text).toBe(crlfText);
  });

  it("BoardNote オブジェクトの更新で複数行テキストが正しく適用されること", () => {
    const initialNote: BoardNote = {
      id: 1,
      boardId: 10,
      text: "初期メモ",
      x: 100,
      y: 100,
      width: 240,
      height: 160,
      scale: 1,
      rotation: 0,
      zIndex: 1,
      color: "#fef08a",
      fontSize: 14,
      isLocked: false,
      createdAt: 1000,
      updatedAt: 1000,
    };

    const newText = "行A\n行B\n行C";
    const updatedNote: BoardNote = {
      ...initialNote,
      text: newText,
      updatedAt: 2000,
    };

    expect(updatedNote.text).toBe(newText);
    expect(updatedNote.text.split("\n")).toEqual(["行A", "行B", "行C"]);
  });
});
