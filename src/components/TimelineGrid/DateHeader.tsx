import React from "react";
import { Calendar, Folder } from "lucide-react";

interface DateHeaderProps {
  day: string;
  count: number;
}

/**
 * タイムラインのグループヘッダコンポーネント（日付またはフォルダ）
 *
 * 変更理由: フォルダ順表示の際、フォルダアイコンとフォルダ名（およびフルパス）を明瞭に表示するため。
 */
export const DateHeader: React.FC<DateHeaderProps> = React.memo(({ day, count }) => {
  // パス形式（\ や / を含む）か判定
  const isFolderPath = day.includes("\\") || day.includes("/");
  const folderName = isFolderPath ? day.split(/[\\/]/).filter(Boolean).pop() || day : day;

  return (
    <div className="flex items-center justify-between px-4 py-2 bg-background/95 backdrop-blur sticky top-0 z-10 border-b border-border select-none">
      <div className="flex items-center gap-2 min-w-0" title={day}>
        {isFolderPath ? (
          <Folder className="w-4 h-4 text-accent shrink-0" />
        ) : (
          <Calendar className="w-4 h-4 text-accent shrink-0" />
        )}
        <span className="font-semibold text-sm text-textPrimary truncate">{folderName}</span>
        {isFolderPath && (
          <span className="text-xs text-textSecondary/70 truncate hidden sm:inline" title={day}>
            {day}
          </span>
        )}
      </div>
      <span className="text-xs text-textSecondary bg-surfaceLight px-2 py-0.5 rounded-full shrink-0">
        {count.toLocaleString()} 枚
      </span>
    </div>
  );
});

DateHeader.displayName = "DateHeader";
