import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Check, Minus, Plus, Tag, Trash2, X } from "lucide-react";
import { backendApi } from "../../lib/ipc";
import { useTranslation } from "../../locales";
import { useAppStore } from "../../store";
import type { ImageTagAssignment } from "../../types/filter";

/**
 * 新規タグ作成時のプリセットカラーパレット
 */
const TAG_COLOR_PALETTE = [
  "#ef4444", // Red
  "#f97316", // Orange
  "#eab308", // Amber
  "#22c55e", // Emerald
  "#06b6d4", // Cyan
  "#3b82f6", // Blue
  "#6366f1", // Indigo
  "#ec4899", // Pink
];

/**
 * 単一または複数選択された画像に対してタグの作成・一括付与・解除を行うモーダル
 *
 * 変更理由: 「お気に入り」を発展させ、1つの画像に複数タグを登録可能にし、
 * タイムラインの複数選択時にも3状態（全員付与:all / 一部付与:some / 未付与:none）を
 * 直感的に操作・即時新規作成できるようにするため。
 */
export const TagEditModal: React.FC = () => {
  const { t } = useTranslation();
  const isOpen = useAppStore((state) => state.isTagEditModalOpen);
  const targetImageIds = useAppStore((state) => state.tagEditTargetImageIds);
  const closeTagEditModal = useAppStore((state) => state.closeTagEditModal);
  const fetchTags = useAppStore((state) => state.fetchTags);
  const filterState = useAppStore((state) => state.filterState);
  const updateFilterState = useAppStore((state) => state.updateFilterState);

  const [assignments, setAssignments] = useState<ImageTagAssignment[]>([]);
  const [inputText, setInputText] = useState("");
  const [selectedColor, setSelectedColor] = useState<string>(TAG_COLOR_PALETTE[5]);
  const [isBusy, setIsBusy] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  const loadAssignments = useCallback(async () => {
    if (!isOpen || targetImageIds.length === 0) return;
    try {
      const list = await backendApi.getTagsForImages(targetImageIds);
      setAssignments(list);
    } catch (err) {
      console.error("画像タグ付与状態の取得に失敗:", err);
    }
  }, [isOpen, targetImageIds]);

  useEffect(() => {
    if (isOpen) {
      setInputText("");
      loadAssignments();
      setTimeout(() => {
        inputRef.current?.focus();
      }, 50);
    } else {
      setAssignments([]);
    }
  }, [isOpen, loadAssignments]);

  // Esc キーでモーダルを閉じる
  useEffect(() => {
    if (!isOpen) return;
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        closeTagEditModal();
      }
    };
    window.addEventListener("keydown", handleKeyDown, true);
    return () => window.removeEventListener("keydown", handleKeyDown, true);
  }, [isOpen, closeTagEditModal]);

  const trimmedInput = inputText.trim();

  // 入力キーワードによる絞り込み
  const filteredAssignments = useMemo(() => {
    if (!trimmedInput) return assignments;
    const lower = trimmedInput.toLowerCase();
    return assignments.filter((a) => a.name.toLowerCase().includes(lower));
  }, [assignments, trimmedInput]);

  // 完全一致する既存タグが存在するか
  const exactMatchExists = useMemo(() => {
    if (!trimmedInput) return false;
    const lower = trimmedInput.toLowerCase();
    return assignments.some((a) => a.name.toLowerCase() === lower);
  }, [assignments, trimmedInput]);

  // タグの付与 / 解除トグル（3状態: "all" -> 解除, "some" / "none" -> 全員に付与）
  const handleToggleTag = async (item: ImageTagAssignment) => {
    if (isBusy || targetImageIds.length === 0) return;
    setIsBusy(true);
    try {
      const shouldRemove = item.status === "all";
      await backendApi.updateImagesTags({
        imageIds: targetImageIds,
        addTagIds: shouldRemove ? [] : [item.tagId],
        removeTagIds: shouldRemove ? [item.tagId] : [],
      });
      await Promise.all([loadAssignments(), fetchTags()]);
    } catch (err) {
      console.error("タグ更新に失敗:", err);
    } finally {
      setIsBusy(false);
    }
  };

  // 新規タグを作成し、対象画像全員に即時付与
  const handleCreateAndAssignTag = async (e?: React.FormEvent) => {
    if (e) e.preventDefault();
    if (!trimmedInput || isBusy || targetImageIds.length === 0) return;

    setIsBusy(true);
    try {
      const created = await backendApi.createTag(trimmedInput, selectedColor);
      await backendApi.updateImagesTags({
        imageIds: targetImageIds,
        addTagIds: [created.id],
        removeTagIds: [],
      });
      setInputText("");
      await Promise.all([loadAssignments(), fetchTags()]);
    } catch (err) {
      console.error("新規タグ作成・付与に失敗:", err);
    } finally {
      setIsBusy(false);
    }
  };

  // タグ自体の削除
  const handleDeleteTag = async (e: React.MouseEvent, item: ImageTagAssignment) => {
    e.stopPropagation();
    if (isBusy) return;
    if (
      typeof window !== "undefined" &&
      typeof window.confirm === "function" &&
      !window.confirm(t("tagModal.deleteTagConfirm", { name: item.name }))
    ) {
      return;
    }

    setIsBusy(true);
    try {
      await backendApi.deleteTag(item.tagId);
      // フィルタ条件に削除対象タグが含まれていた場合は除外する
      const nextInc = (filterState.includeTagIds ?? []).filter((id) => id !== item.tagId);
      const nextExc = (filterState.excludeTagIds ?? []).filter((id) => id !== item.tagId);
      if (
        nextInc.length !== (filterState.includeTagIds ?? []).length ||
        nextExc.length !== (filterState.excludeTagIds ?? []).length
      ) {
        updateFilterState({
          includeTagIds: nextInc.length > 0 ? nextInc : null,
          excludeTagIds: nextExc.length > 0 ? nextExc : null,
        });
      }
      await Promise.all([loadAssignments(), fetchTags()]);
    } catch (err) {
      console.error("タグ削除に失敗:", err);
    } finally {
      setIsBusy(false);
    }
  };

  if (!isOpen) return null;

  return (
    <div
      className="fixed inset-0 z-[70] bg-black/60 backdrop-blur-xs flex items-center justify-center p-4 animate-in fade-in duration-150 select-none"
      onClick={closeTagEditModal}
    >
      <div
        className="bg-surface border border-border rounded-xl shadow-2xl w-full max-w-md overflow-hidden flex flex-col max-h-[82vh]"
        onClick={(e) => e.stopPropagation()}
      >
        {/* ヘッダー */}
        <div className="px-4 py-3 border-b border-border flex items-center justify-between bg-surfaceLight/30">
          <div className="flex items-center gap-2 text-sm font-bold text-textPrimary">
            <Tag className="w-4 h-4 text-accent" />
            <span>
              {targetImageIds.length > 1
                ? t("tagModal.title", { count: targetImageIds.length })
                : t("tagModal.titleSingle")}
            </span>
          </div>
          <button
            onClick={closeTagEditModal}
            className="p-1 text-textSecondary hover:text-textPrimary hover:bg-surfaceLight rounded-lg transition cursor-pointer"
            title={t("common.close")}
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* 検索 & 新規タグ作成フォーム */}
        <div className="p-3.5 border-b border-border/60 space-y-2.5 bg-surface">
          <form onSubmit={handleCreateAndAssignTag} className="flex items-center gap-2">
            <input
              ref={inputRef}
              type="text"
              value={inputText}
              onChange={(e) => setInputText(e.target.value)}
              placeholder={t("tagModal.searchOrCreatePlaceholder")}
              className="flex-1 bg-background border border-border rounded-lg px-3 py-1.5 text-xs text-textPrimary outline-none focus:border-accent transition"
            />
            {inputText && (
              <button
                type="button"
                onClick={() => setInputText("")}
                className="text-textSecondary hover:text-textPrimary p-1 cursor-pointer"
              >
                <X className="w-3.5 h-3.5" />
              </button>
            )}
          </form>

          {/* 新規タグ作成パネル（未登録名が入力されている場合、またはタグが0件の場合に表示） */}
          {(trimmedInput && !exactMatchExists) || assignments.length === 0 ? (
            <div className="bg-surfaceLight/50 border border-border/70 rounded-lg p-2.5 space-y-2">
              <div className="flex items-center justify-between gap-2">
                <span className="text-[11px] text-textSecondary font-medium">
                  {t("tagModal.colorLabel")}
                </span>
                <div className="flex items-center gap-1.5">
                  {TAG_COLOR_PALETTE.map((hex) => (
                    <button
                      key={hex}
                      type="button"
                      onClick={() => setSelectedColor(hex)}
                      style={{ backgroundColor: hex }}
                      className={`w-4 h-4 rounded-full transition transform cursor-pointer ${
                        selectedColor === hex
                          ? "ring-2 ring-white scale-110 shadow"
                          : "opacity-75 hover:opacity-100"
                      }`}
                    />
                  ))}
                </div>
              </div>
              {trimmedInput && !exactMatchExists && (
                <button
                  type="button"
                  disabled={isBusy}
                  onClick={() => handleCreateAndAssignTag()}
                  className="w-full flex items-center justify-center gap-1.5 bg-accent hover:bg-accent/90 disabled:opacity-50 text-white py-1.5 px-3 rounded-lg text-xs font-medium transition shadow-sm cursor-pointer"
                >
                  <Plus className="w-3.5 h-3.5" />
                  <span className="truncate">
                    {t("tagModal.createBtn", { name: trimmedInput })}
                  </span>
                </button>
              )}
            </div>
          ) : null}
        </div>

        {/* 既存タグ一覧（3状態チェックボックス） */}
        <div className="flex-1 overflow-y-auto p-3 space-y-1">
          <div className="text-[11px] font-semibold text-textSecondary px-1 pb-1">
            {t("tagModal.existingTagsHeader")}
          </div>

          {filteredAssignments.length === 0 ? (
            <div className="py-8 text-center text-xs text-textSecondary/70">
              {t("tagModal.noTagsCreated")}
            </div>
          ) : (
            filteredAssignments.map((item) => {
              const isAll = item.status === "all";
              const isSome = item.status === "some";

              return (
                <div
                  key={item.tagId}
                  onClick={() => handleToggleTag(item)}
                  title={
                    isAll
                      ? t("tagModal.stateAll")
                      : isSome
                      ? t("tagModal.stateSome")
                      : t("tagModal.stateNone")
                  }
                  className={`group flex items-center justify-between px-3 py-2 rounded-lg text-xs cursor-pointer transition border ${
                    isAll
                      ? "bg-accent/15 border-accent/40 text-textPrimary font-medium"
                      : isSome
                      ? "bg-amber-500/10 border-amber-500/30 text-textPrimary"
                      : "bg-transparent border-transparent hover:bg-surfaceLight text-textSecondary hover:text-textPrimary"
                  }`}
                >
                  <div className="flex items-center gap-2.5 min-w-0 flex-1">
                    {/* 3状態チェックボックスアイコン */}
                    <div
                      className={`w-4 h-4 rounded flex items-center justify-center border transition shrink-0 ${
                        isAll
                          ? "bg-accent border-accent text-white"
                          : isSome
                          ? "bg-amber-500/20 border-amber-400 text-amber-300"
                          : "border-border bg-background"
                      }`}
                    >
                      {isAll && <Check className="w-3 h-3 stroke-[3]" />}
                      {isSome && <Minus className="w-3 h-3 stroke-[3]" />}
                    </div>

                    {/* カラーバッジ + タグ名 */}
                    <span
                      className="w-2.5 h-2.5 rounded-full shrink-0"
                      style={{ backgroundColor: item.color || "#6366f1" }}
                    />
                    <span className="truncate">{item.name}</span>

                    {isSome && (
                      <span className="text-[10px] text-amber-400/90 bg-amber-500/10 px-1.5 py-0.5 rounded shrink-0">
                        一部付与
                      </span>
                    )}
                  </div>

                  {/* タグ自体の削除ボタン */}
                  <button
                    type="button"
                    onClick={(e) => handleDeleteTag(e, item)}
                    title={t("tagModal.deleteTagTooltip")}
                    className="opacity-0 group-hover:opacity-100 p-1 text-textSecondary hover:text-red-400 rounded transition cursor-pointer shrink-0"
                  >
                    <Trash2 className="w-3.5 h-3.5" />
                  </button>
                </div>
              );
            })
          )}
        </div>

        {/* フッター */}
        <div className="px-4 py-2.5 border-t border-border bg-surfaceLight/20 flex items-center justify-end">
          <button
            type="button"
            onClick={closeTagEditModal}
            className="bg-accent hover:bg-accent/90 text-white px-4 py-1.5 rounded-lg text-xs font-medium transition cursor-pointer"
          >
            {t("tagModal.applyBtn")}
          </button>
        </div>
      </div>
    </div>
  );
};
