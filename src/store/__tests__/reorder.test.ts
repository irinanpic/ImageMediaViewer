import { describe, it, expect, beforeEach } from "vitest";
import { useAppStore } from "../../store";
import type { WatchedFolder } from "../../types/generated/WatchedFolder";
import type { Board } from "../../types/generated/Board";

// Node.js環境下でのテスト用に localStorage のインメモリモックをセットアップ
const storageMap = new Map<string, string>();
const localStorageMock = {
  getItem: (key: string) => storageMap.get(key) ?? null,
  setItem: (key: string, val: string) => {
    storageMap.set(key, String(val));
  },
  removeItem: (key: string) => {
    storageMap.delete(key);
  },
  clear: () => {
    storageMap.clear();
  },
};
Object.defineProperty(globalThis, "localStorage", {
  value: localStorageMock,
  writable: true,
  configurable: true,
});

describe("サイドバー並び替え永続化 (reorderFolders & reorderBoards)", () => {
  beforeEach(() => {
    localStorage.clear();
    useAppStore.setState({
      folders: [],
      boards: [],
    });
  });

  const dummyFolders: WatchedFolder[] = [
    { id: 10, path: "C:/photos/A", imageCount: 10, thumbCount: 10, status: "online" },
    { id: 20, path: "C:/photos/B", imageCount: 20, thumbCount: 20, status: "online" },
    { id: 30, path: "C:/photos/C", imageCount: 30, thumbCount: 30, status: "online" },
  ];

  const dummyBoards: Board[] = [
    { id: 101, name: "キャラデザイン", backgroundColor: "#1e1e1e", panX: 0, panY: 0, zoom: 1, itemCount: 5, createdAt: 1000, updatedAt: 1000 },
    { id: 102, name: "背景資料", backgroundColor: "#1e1e1e", panX: 0, panY: 0, zoom: 1, itemCount: 8, createdAt: 2000, updatedAt: 2000 },
    { id: 103, name: "構図ラフ", backgroundColor: "#1e1e1e", panX: 0, panY: 0, zoom: 1, itemCount: 2, createdAt: 3000, updatedAt: 3000 },
  ];

  it("reorderFolders でフォルダの順序が変更され localStorage に保存されること", () => {
    useAppStore.setState({ folders: [...dummyFolders] });

    // 先頭のフォルダ(id: 10)を末尾(index: 2)へ移動
    useAppStore.getState().reorderFolders(0, 2);

    const updated = useAppStore.getState().folders;
    expect(updated.map((f) => f.id)).toEqual([20, 30, 10]);

    const saved = JSON.parse(localStorage.getItem("imv_folder_order") || "[]");
    expect(saved).toEqual([20, 30, 10]);
  });

  it("setFolders が localStorage の imv_folder_order に従って自動ソートすること", () => {
    localStorage.setItem("imv_folder_order", JSON.stringify([30, 10, 20]));

    // 初期順序 [10, 20, 30] を渡す
    useAppStore.getState().setFolders([...dummyFolders]);

    const folders = useAppStore.getState().folders;
    expect(folders.map((f) => f.id)).toEqual([30, 10, 20]);
  });

  it("reorderBoards でボードの順序が変更され localStorage に保存されること", () => {
    useAppStore.setState({ boards: [...dummyBoards] });

    // 2番目のボード(id: 102)を先頭(index: 0)へ移動
    useAppStore.getState().reorderBoards(1, 0);

    const updated = useAppStore.getState().boards;
    expect(updated.map((b) => b.id)).toEqual([102, 101, 103]);

    const saved = JSON.parse(localStorage.getItem("imv_board_order") || "[]");
    expect(saved).toEqual([102, 101, 103]);
  });

  it("setBoards が localStorage の imv_board_order に従って自動ソートすること", () => {
    localStorage.setItem("imv_board_order", JSON.stringify([103, 101, 102]));

    // 初期順序 [101, 102, 103] を渡す
    useAppStore.getState().setBoards([...dummyBoards]);

    const boards = useAppStore.getState().boards;
    expect(boards.map((b) => b.id)).toEqual([103, 101, 102]);
  });
});
