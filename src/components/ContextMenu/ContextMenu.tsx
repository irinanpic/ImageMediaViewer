import React, { useEffect, useRef, useState } from "react";
import {
  ChevronsDown,
  ChevronsUp,
  Copy,
  ExternalLink,
  FlipHorizontal,
  FlipVertical,
  FolderOpen,
  LayoutGrid,
  Lock,
  Palette,
  Play,
  Settings,
  Trash2,
  Unlock,
  XCircle,
} from "lucide-react";
import { useAppStore } from "../../store";
import { useTranslation } from "../../locales";
import { ContextMenuRegistry } from "../../plugins/actions/registry";
import { backendApi } from "../../lib/ipc";
import type { ImageDetail } from "../../types/generated/ImageDetail";
import type { ActionContext } from "../../plugins/actions/types";

// アイコンマッピング
const ICON_MAP: Record<string, React.ComponentType<{ className?: string }>> = {
  ExternalLink,
  Palette,
  FolderOpen,
  Copy,
  LayoutGrid,
  Trash2,
  Play,
};

/**
 * 右クリック（長押し）時にカーソル位置へ出現するフローティングコンテキストメニュー
 *
 * 変更理由: 仕様書およびプラグインアーキテクチャに準拠し、
 * タイムライン・ビューア・ムードボードのすべてで共通のプラグインアクションおよび
 * カスタム外部連携アプリを動的レンダリングするとともに、
 * ムードボード上のアイテム右クリック時にはボード固有の配置操作もシームレスに提供するため。
 */
export const ContextMenu: React.FC = () => {
  const { locale } = useTranslation();
  const contextMenu = useAppStore((state) => state.contextMenu);
  const closeContextMenu = useAppStore((state) => state.closeContextMenu);
  const openContextMenuSettings = useAppStore((state) => state.openContextMenuSettings);
  const appSettings = useAppStore((state) => state.appSettings);
  const refreshTimeline = useAppStore((state) => state.refreshTimeline);
  const openViewer = useAppStore((state) => state.openViewer);
  const openAddToBoardModal = useAppStore((state) => state.openAddToBoardModal);

  const menuRef = useRef<HTMLDivElement>(null);
  const [detail, setDetail] = useState<ImageDetail | undefined>(undefined);
  const [position, setPosition] = useState<{ x: number; y: number }>({ x: 0, y: 0 });

  // 画像詳細情報を非同期取得（拡張子判定や実ファイルパス解決用）
  useEffect(() => {
    if (!contextMenu.isOpen || !contextMenu.record) {
      setDetail(undefined);
      return;
    }

    let isMounted = true;
    backendApi
      .getImageDetail(contextMenu.record.id)
      .then((d) => {
        if (isMounted) setDetail(d);
      })
      .catch((err) => {
        console.warn("コンテキストメニュー表示時の詳細取得失敗:", err);
      });

    return () => {
      isMounted = false;
    };
  }, [contextMenu.isOpen, contextMenu.record?.id]);

  // 画面端はみ出しクランプ計算
  useEffect(() => {
    if (!contextMenu.isOpen) return;

    const updatePosition = () => {
      const el = menuRef.current;
      const menuWidth = el ? el.offsetWidth : 220;
      const menuHeight = el ? el.offsetHeight : 320;
      const padding = 12;

      let clampedX = contextMenu.x;
      let clampedY = contextMenu.y;

      if (clampedX + menuWidth > window.innerWidth - padding) {
        clampedX = Math.max(padding, window.innerWidth - menuWidth - padding);
      }
      if (clampedY + menuHeight > window.innerHeight - padding) {
        clampedY = Math.max(padding, window.innerHeight - menuHeight - padding);
      }

      setPosition({ x: clampedX, y: clampedY });
    };

    // 初期配置 & 寸法確定後の再調整
    updatePosition();
    const timer = setTimeout(updatePosition, 0);
    return () => clearTimeout(timer);
  }, [contextMenu.isOpen, contextMenu.x, contextMenu.y, detail]);

  // 外部クリック、ウィンドウリサイズ、スクロール、Escapeキーで自動クローズ
  useEffect(() => {
    if (!contextMenu.isOpen) return;

    const handlePointerDown = (e: MouseEvent | TouchEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        closeContextMenu();
      }
    };

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        closeContextMenu();
      }
    };

    const handleScroll = () => {
      closeContextMenu();
    };

    window.addEventListener("mousedown", handlePointerDown);
    window.addEventListener("touchstart", handlePointerDown);
    window.addEventListener("keydown", handleKeyDown);
    window.addEventListener("scroll", handleScroll, true);
    window.addEventListener("resize", handleScroll);

    return () => {
      window.removeEventListener("mousedown", handlePointerDown);
      window.removeEventListener("touchstart", handlePointerDown);
      window.removeEventListener("keydown", handleKeyDown);
      window.removeEventListener("scroll", handleScroll, true);
      window.removeEventListener("resize", handleScroll);
    };
  }, [contextMenu.isOpen, closeContextMenu]);

  if (!contextMenu.isOpen || !contextMenu.record) {
    return null;
  }

  const actions = ContextMenuRegistry.getAvailableActions(
    contextMenu.record,
    appSettings,
    detail
  );

  const boardCtx = contextMenu.boardItemContext;

  const handleActionClick = async (action: (typeof actions)[0]) => {
    closeContextMenu();
    if (!contextMenu.record) return;

    const actionContext: ActionContext = {
      record: contextMenu.record,
      detail,
      refreshTimeline,
      openViewer,
      openAddToBoardModal,
    };

    try {
      await action.execute(actionContext);
    } catch (err) {
      console.error(`アクション [${action.id}] の実行に失敗しました:`, err);
    }
  };

  return (
    <div
      ref={menuRef}
      role="menu"
      style={{
        left: `${position.x}px`,
        top: `${position.y}px`,
      }}
      className="fixed z-50 min-w-[210px] max-w-[290px] bg-surface/95 backdrop-blur-md border border-border shadow-2xl rounded-lg py-1.5 text-textPrimary text-sm animate-in fade-in zoom-in-95 duration-100 select-none"
    >
      <div className="px-3 py-1 text-xs text-textSecondary font-medium truncate border-b border-border/40 mb-1">
        {detail?.filePath
          ? detail.filePath.split(/[\\/]/).pop()
          : `Image #${contextMenu.record.id}`}
      </div>

      {/* ムードボード上のアイテム右クリック時専用操作セクション */}
      {boardCtx && (
        <>
          <div className="px-3 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-textSecondary/70">
            {locale === "en" ? "Board Item" : "ボード配置操作"}
          </div>
          <button
            role="menuitem"
            onClick={() => {
              closeContextMenu();
              boardCtx.onBringToFront();
            }}
            className="w-full text-left px-3 py-1.5 flex items-center gap-2.5 hover:bg-accent/20 hover:text-accent text-textPrimary transition-colors"
          >
            <ChevronsUp className="w-4 h-4 flex-shrink-0" />
            <span className="truncate flex-1">
              {locale === "en" ? "Bring to Front" : "最前面へ移動"}
            </span>
          </button>
          <button
            role="menuitem"
            onClick={() => {
              closeContextMenu();
              boardCtx.onSendToBack();
            }}
            className="w-full text-left px-3 py-1.5 flex items-center gap-2.5 hover:bg-accent/20 hover:text-accent text-textPrimary transition-colors"
          >
            <ChevronsDown className="w-4 h-4 flex-shrink-0" />
            <span className="truncate flex-1">
              {locale === "en" ? "Send to Back" : "最背面へ移動"}
            </span>
          </button>
          <button
            role="menuitem"
            onClick={() => {
              closeContextMenu();
              boardCtx.onToggleFlipH();
            }}
            className="w-full text-left px-3 py-1.5 flex items-center gap-2.5 hover:bg-accent/20 hover:text-accent text-textPrimary transition-colors"
          >
            <FlipHorizontal className="w-4 h-4 flex-shrink-0" />
            <span className="truncate flex-1">
              {locale === "en" ? "Flip Horizontal" : "左右反転"}
            </span>
          </button>
          <button
            role="menuitem"
            onClick={() => {
              closeContextMenu();
              boardCtx.onToggleFlipV();
            }}
            className="w-full text-left px-3 py-1.5 flex items-center gap-2.5 hover:bg-accent/20 hover:text-accent text-textPrimary transition-colors"
          >
            <FlipVertical className="w-4 h-4 flex-shrink-0" />
            <span className="truncate flex-1">
              {locale === "en" ? "Flip Vertical" : "上下反転"}
            </span>
          </button>
          <button
            role="menuitem"
            onClick={() => {
              closeContextMenu();
              boardCtx.onToggleLock();
            }}
            className="w-full text-left px-3 py-1.5 flex items-center gap-2.5 hover:bg-accent/20 hover:text-accent text-textPrimary transition-colors"
          >
            {boardCtx.isLocked ? (
              <Unlock className="w-4 h-4 flex-shrink-0 text-accent" />
            ) : (
              <Lock className="w-4 h-4 flex-shrink-0" />
            )}
            <span className="truncate flex-1">
              {boardCtx.isLocked
                ? locale === "en"
                  ? "Unlock Position"
                  : "ロックを解除"
                : locale === "en"
                ? "Lock Position"
                : "位置をロック"}
            </span>
          </button>
          {!boardCtx.isLocked && (
            <button
              role="menuitem"
              onClick={() => {
                closeContextMenu();
                boardCtx.onRemoveFromBoard();
              }}
              className="w-full text-left px-3 py-1.5 flex items-center gap-2.5 hover:bg-amber-500/20 text-amber-400 hover:text-amber-300 transition-colors"
            >
              <XCircle className="w-4 h-4 flex-shrink-0" />
              <span className="truncate flex-1">
                {locale === "en" ? "Remove from Board" : "このボードから除外"}
              </span>
            </button>
          )}
          <div className="h-px bg-border/40 my-1" />
        </>
      )}

      {/* 共通ファイル・プラグインアクション群 */}
      {actions.map((action) => {
        const IconComponent = action.icon ? ICON_MAP[action.icon] : null;
        const isTrash = action.id === "action.trash_file";

        return (
          <button
            key={action.id}
            role="menuitem"
            onClick={() => handleActionClick(action)}
            className={`w-full text-left px-3 py-1.5 flex items-center gap-2.5 transition-colors ${
              isTrash
                ? "hover:bg-red-500/20 text-red-400 hover:text-red-300"
                : "hover:bg-accent/20 hover:text-accent text-textPrimary"
            }`}
          >
            {IconComponent && <IconComponent className="w-4 h-4 flex-shrink-0" />}
            <span className="truncate flex-1">{action.label(locale)}</span>
          </button>
        );
      })}

      <div className="h-px bg-border/40 my-1" />

      <button
        role="menuitem"
        onClick={() => {
          closeContextMenu();
          openContextMenuSettings();
        }}
        className="w-full text-left px-3 py-1.5 flex items-center gap-2.5 hover:bg-surfaceLight/80 text-textSecondary hover:text-textPrimary transition-colors text-xs"
      >
        <Settings className="w-3.5 h-3.5 flex-shrink-0" />
        <span className="truncate">
          {locale === "en" ? "Plugins & Menu Settings..." : "プラグイン・メニュー設定..."}
        </span>
      </button>
    </div>
  );
};
