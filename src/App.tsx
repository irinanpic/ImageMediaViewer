import React from "react";
import { Sidebar } from "./components/Sidebar/Sidebar";
import { StatusBar } from "./components/StatusBar/StatusBar";
import { TimelineToolbar } from "./components/TimelineGrid/TimelineToolbar";
import { VirtualTimeline } from "./components/TimelineGrid/VirtualTimeline";
import { ImageViewerModal } from "./components/Viewer/ImageViewerModal";
import { MoodboardCanvas } from "./components/Moodboard/MoodboardCanvas";
import { AddToBoardModal } from "./components/Moodboard/AddToBoardModal";
import { LogViewerModal } from "./components/Common/LogViewerModal";
import { useBackendEvents } from "./hooks/useBackendEvents";
import { useWindowState } from "./hooks/useWindowState";
import { useAppStore } from "./store";

export const App: React.FC = () => {
  // バックエンドイベント購読の有効化
  useBackendEvents();
  // ウィンドウ状態（サイズ・位置・最大化）の自動保存
  useWindowState();

  const currentView = useAppStore((state) => state.currentView);

  return (
    <div className="flex flex-col h-screen w-screen overflow-hidden bg-background text-textPrimary">
      {/* メインレイアウト（サイドバー + タイムラインまたはムードボードキャンバス） */}
      <div className="flex flex-1 overflow-hidden relative">
        <Sidebar />
        <div className="flex flex-col flex-1 overflow-hidden relative">
          {/* タイムライン画面（ボード表示中もDOMとスクロール位置を完全保持） */}
          <div className={`flex flex-col flex-1 overflow-hidden relative ${currentView === "timeline" ? "" : "hidden"}`}>
            <TimelineToolbar />
            <VirtualTimeline />
          </div>

          {/* ムードボードキャンバス */}
          {currentView === "board" && <MoodboardCanvas />}
        </div>
      </div>

      {/* 下部ステータスバー */}
      <StatusBar />

      {/* 単一画像詳細・全画面ビューアモーダル */}
      <ImageViewerModal />

      {/* ムードボード追加ダイアログ */}
      <AddToBoardModal />

      {/* システムログ & エラー診断モーダル */}
      <LogViewerModal />
    </div>
  );
};

export default App;
