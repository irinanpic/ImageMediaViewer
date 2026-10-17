import { useEffect, useRef } from "react";
import { useAppStore } from "../store";
import { useResponsiveLayout } from "./useResponsiveLayout";

/**
 * Androidおよびブラウザの戻る操作（Backボタン / スワイプジェスチャー）を制御するカスタムフック
 *
 * 変更理由: モバイル端末（Android）において、画像拡大モーダル表示中、ムードボード表示中、
 * ログモーダル表示中、または画像複数選択中に端末の戻るボタン（Backキー）が押された際、
 * アプリが突然終了してしまうのを防ぎ、UI階層を1段ずつ安全に戻すため。
 */
export function useBackNavigation() {
  const { isCompact } = useResponsiveLayout();
  const isViewerOpen = useAppStore((state) => state.isViewerOpen);
  const closeViewer = useAppStore((state) => state.closeViewer);
  const isAddToBoardModalOpen = useAppStore((state) => state.isAddToBoardModalOpen);
  const closeAddToBoardModal = useAppStore((state) => state.closeAddToBoardModal);
  const isLogModalOpen = useAppStore((state) => state.isLogModalOpen);
  const closeLogModal = useAppStore((state) => state.closeLogModal);
  const isContextMenuSettingsOpen = useAppStore((state) => state.isContextMenuSettingsOpen);
  const closeContextMenuSettings = useAppStore((state) => state.closeContextMenuSettings);
  const isContextMenuOpen = useAppStore((state) => state.contextMenu.isOpen);
  const closeContextMenu = useAppStore((state) => state.closeContextMenu);
  const selectedImageIds = useAppStore((state) => state.selectedImageIds);
  const clearSelectedImageIds = useAppStore((state) => state.clearSelectedImageIds);
  const currentView = useAppStore((state) => state.currentView);
  const setCurrentView = useAppStore((state) => state.setCurrentView);
  const isSidebarOpen = useAppStore((state) => state.isSidebarOpen);
  const setSidebarOpen = useAppStore((state) => state.setSidebarOpen);

  // 下位から上位への正確な階層レイヤー判定
  const activeLayers: string[] = [];
  if (isCompact && isSidebarOpen) activeLayers.push("sidebar");
  if (currentView === "board") activeLayers.push("board");
  if (selectedImageIds.length > 0) activeLayers.push("selection");
  if (isViewerOpen) activeLayers.push("viewer");
  if (isAddToBoardModalOpen) activeLayers.push("addToBoard");
  if (isLogModalOpen) activeLayers.push("log");
  if (isContextMenuSettingsOpen) activeLayers.push("contextMenuSettings");
  if (isContextMenuOpen) activeLayers.push("contextMenu");

  const historyPushedCountRef = useRef(0);
  const isProgrammaticBackRef = useRef(false);

  // UI操作による開閉とブラウザ履歴スタックの同期
  useEffect(() => {
    if (typeof window === "undefined") return;

    const targetDepth = activeLayers.length;
    const currentPushed = historyPushedCountRef.current;

    if (targetDepth > currentPushed) {
      // 新しいモーダルやサブビューが開いた場合、差分分 pushState
      for (let i = currentPushed; i < targetDepth; i++) {
        window.history.pushState({ imvModal: true }, "");
      }
      historyPushedCountRef.current = targetDepth;
    } else if (targetDepth < currentPushed) {
      // UIボタン（Xボタンやキャンセル）で閉じた場合、余剰になった履歴エントリを安全に解消
      const diff = currentPushed - targetDepth;
      historyPushedCountRef.current = targetDepth;
      isProgrammaticBackRef.current = true;
      window.history.go(-diff);
    }
  }, [activeLayers.length]);

  // Android戻るボタン（popstate）のハンドリング
  useEffect(() => {
    if (typeof window === "undefined") return;

    const handlePopState = () => {
      // プログラム起因の history.go() の場合はUI操作を行わない
      if (isProgrammaticBackRef.current) {
        isProgrammaticBackRef.current = false;
        return;
      }

      if (historyPushedCountRef.current > 0) {
        historyPushedCountRef.current--;
      }

      // 最上位の階層から1段閉じる
      const topLayer = activeLayers[activeLayers.length - 1];
      if (!topLayer) return;

      switch (topLayer) {
        case "contextMenu":
          closeContextMenu();
          break;
        case "contextMenuSettings":
          closeContextMenuSettings();
          break;
        case "log":
          closeLogModal();
          break;
        case "addToBoard":
          closeAddToBoardModal();
          break;
        case "viewer":
          closeViewer();
          break;
        case "selection":
          clearSelectedImageIds();
          break;
        case "board":
          setCurrentView("timeline");
          break;
        case "sidebar":
          setSidebarOpen(false);
          break;
      }
    };

    window.addEventListener("popstate", handlePopState);
    return () => {
      window.removeEventListener("popstate", handlePopState);
    };
  }, [
    activeLayers,
    closeContextMenu,
    closeContextMenuSettings,
    closeLogModal,
    closeAddToBoardModal,
    closeViewer,
    clearSelectedImageIds,
    setCurrentView,
    setSidebarOpen,
  ]);
}
