import React, { useEffect, useState } from "react";
import { Check, LayoutGrid, Plus, X } from "lucide-react";
import { backendApi } from "../../lib/ipc";
import { useTranslation } from "../../locales";
import { useAppStore } from "../../store";
import type { Board } from "../../types/board";

/**
 * 画像をムードボードに追加するモーダルコンポーネント
 */
export const AddToBoardModal: React.FC = () => {
  const { t } = useTranslation();
  const isAddToBoardModalOpen = useAppStore((state) => state.isAddToBoardModalOpen);
  const addToBoardTargetImageIds = useAppStore((state) => state.addToBoardTargetImageIds);
  const closeAddToBoardModal = useAppStore((state) => state.closeAddToBoardModal);
  const setCurrentView = useAppStore((state) => state.setCurrentView);
  const setActiveBoardId = useAppStore((state) => state.setActiveBoardId);
  const boards = useAppStore((state) => state.boards);
  const setBoards = useAppStore((state) => state.setBoards);

  const [newBoardName, setNewBoardName] = useState("");
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [successMessage, setSuccessMessage] = useState<string | null>(null);

  useEffect(() => {
    if (isAddToBoardModalOpen) {
      backendApi.getBoards().then(setBoards).catch(console.error);
      setNewBoardName("");
      setSuccessMessage(null);
    }
  }, [isAddToBoardModalOpen, setBoards]);

  if (!isAddToBoardModalOpen) return null;

  // 既存ボードへ追加
  const handleAddToBoard = async (board: Board) => {
    if (addToBoardTargetImageIds.length === 0) return;
    setIsSubmitting(true);
    try {
      await backendApi.addBoardItems(board.id, addToBoardTargetImageIds);
      setSuccessMessage(t("addToBoardModal.successMsg", { name: board.name }));
      setTimeout(() => {
        closeAddToBoardModal();
        setActiveBoardId(board.id);
        setCurrentView("board");
      }, 700);
    } catch (err) {
      console.error("ボードへの追加に失敗しました:", err);
    } finally {
      setIsSubmitting(false);
    }
  };

  // 新規ボード作成して追加
  const handleCreateAndAdd = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!newBoardName.trim() || addToBoardTargetImageIds.length === 0) return;
    setIsSubmitting(true);
    try {
      const created = await backendApi.createBoard({ name: newBoardName.trim() });
      await backendApi.addBoardItems(created.id, addToBoardTargetImageIds);
      const all = await backendApi.getBoards();
      setBoards(all);
      setSuccessMessage(t("addToBoardModal.createAndAddSuccessMsg", { name: created.name }));
      setTimeout(() => {
        closeAddToBoardModal();
        setActiveBoardId(created.id);
        setCurrentView("board");
      }, 700);
    } catch (err) {
      console.error("ボード作成と追加に失敗しました:", err);
    } finally {
      setIsSubmitting(false);
    }
  };

  return (
    <div className="fixed inset-0 z-[100] flex items-center justify-center bg-black/60 backdrop-blur-sm p-4 animate-in fade-in duration-150">
      <div className="bg-surface border border-border/80 rounded-xl shadow-2xl w-full max-w-md overflow-hidden">
        {/* ヘッダー */}
        <div className="flex items-center justify-between px-5 py-4 border-b border-border/50">
          <div className="flex items-center gap-2">
            <LayoutGrid className="w-5 h-5 text-accent" />
            <h3 className="font-semibold text-textPrimary text-sm">
              {t("addToBoardModal.title")}
            </h3>
          </div>
          <button
            onClick={closeAddToBoardModal}
            className="p-1 text-textSecondary hover:text-textPrimary rounded-lg hover:bg-surfaceLight/50 transition"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* コンテンツ */}
        <div className="p-5 space-y-4">
          <p className="text-xs text-textSecondary">
            {t("addToBoardModal.instruction", { count: addToBoardTargetImageIds.length })}
          </p>

          {successMessage && (
            <div className="flex items-center gap-2 p-3 bg-accent/20 border border-accent/40 rounded-lg text-accent text-xs font-medium">
              <Check className="w-4 h-4" />
              {successMessage}
            </div>
          )}

          {/* 既存ボード一覧 */}
          <div className="space-y-1.5 max-h-48 overflow-y-auto pr-1">
            {boards.length === 0 ? (
              <div className="text-center py-4 text-xs text-textSecondary border border-dashed border-border/60 rounded-lg">
                {t("addToBoardModal.noBoards")}
              </div>
            ) : (
              boards.map((b) => (
                <button
                  key={b.id}
                  disabled={isSubmitting}
                  onClick={() => handleAddToBoard(b)}
                  className="w-full flex items-center justify-between px-3 py-2.5 rounded-lg bg-surfaceLight/30 hover:bg-surfaceLight/70 border border-border/40 hover:border-accent text-left transition group"
                >
                  <span className="font-medium text-xs text-textPrimary group-hover:text-accent">
                    {b.name}
                  </span>
                  <span className="text-[11px] text-textSecondary">
                    {t("addToBoardModal.itemCount", { count: b.itemCount })}
                  </span>
                </button>
              ))
            )}
          </div>

          <div className="relative flex py-1 items-center">
            <div className="flex-grow border-t border-border/40"></div>
            <span className="flex-shrink mx-3 text-[11px] text-textSecondary">{t("addToBoardModal.or")}</span>
            <div className="flex-grow border-t border-border/40"></div>
          </div>

          {/* 新規ボード作成フォーム */}
          <form onSubmit={handleCreateAndAdd} className="space-y-2.5">
            <label className="text-xs font-medium text-textSecondary block">
              {t("addToBoardModal.createNewBoardLabel")}
            </label>
            <div className="flex gap-2">
              <input
                type="text"
                value={newBoardName}
                onChange={(e) => setNewBoardName(e.target.value)}
                placeholder={t("addToBoardModal.boardNamePlaceholder")}
                disabled={isSubmitting}
                className="flex-1 bg-surfaceLight/40 border border-border/60 rounded-lg px-3 py-2 text-xs text-textPrimary placeholder:text-textSecondary/50 focus:outline-none focus:border-accent"
              />
              <button
                type="submit"
                disabled={isSubmitting || !newBoardName.trim()}
                className="flex items-center gap-1 bg-accent hover:bg-accent/90 disabled:opacity-50 text-white text-xs font-medium px-3.5 py-2 rounded-lg transition shadow-md shadow-accent/20"
              >
                <Plus className="w-3.5 h-3.5" />
                {t("addToBoardModal.createAndAddBtn")}
              </button>
            </div>
          </form>
        </div>
      </div>
    </div>
  );
};
