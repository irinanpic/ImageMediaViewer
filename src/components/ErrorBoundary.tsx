import { Component, ErrorInfo, ReactNode } from "react";
import { AlertTriangle, RefreshCw } from "lucide-react";
import { t } from "../locales";

interface Props {
  children: ReactNode;
}

interface State {
  hasError: boolean;
  error: Error | null;
}

/**
 * フロントエンドUIクラッシュ防止用エラーバウンダリ
 *
 * 変更理由: フロントエンドのレンダリング例外による白紙化を防止し、常に安全にUIを復旧可能にするため
 */
export class ErrorBoundary extends Component<Props, State> {
  public state: State = {
    hasError: false,
    error: null,
  };

  public static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error };
  }

  public componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    console.error("ErrorBoundary caught an error:", error, errorInfo);
  }

  public render() {
    if (this.state.hasError) {
      return (
        <div className="flex flex-col items-center justify-center min-h-screen w-full bg-[#121212] text-[#f3f4f6] p-6 text-center select-none">
          <div className="w-16 h-16 rounded-full bg-red-500/10 flex items-center justify-center mb-4 text-red-400">
            <AlertTriangle className="w-8 h-8" />
          </div>
          <h1 className="text-xl font-bold mb-2">{t("errorBoundary.title")}</h1>
          <p className="text-sm text-gray-400 max-w-md mb-6 break-words">
            {this.state.error?.message || t("errorBoundary.fallbackMsg")}
          </p>
          <button
            onClick={() => window.location.reload()}
            className="flex items-center gap-2 px-4 py-2 bg-blue-600 hover:bg-blue-700 text-white rounded-md text-sm font-medium transition"
          >
            <RefreshCw className="w-4 h-4" />
            <span>{t("errorBoundary.reloadBtn")}</span>
          </button>
        </div>
      );
    }

    return this.props.children;
  }
}
