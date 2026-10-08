/// デスクトップ環境用タスクトレイメニューの多言語メッセージ構造体
#[derive(Debug, Clone, Copy)]
pub struct TrayMessages {
    pub tooltip: &'static str,
    pub open_client: &'static str,
    pub stay_in_tray: &'static str,
    pub rescan: &'static str,
    pub open_data_folder: &'static str,
    pub open_log_folder: &'static str,
    pub quit: &'static str,
}

/// 英語マスターリソース（デフォルト言語）
pub const TRAY_EN: TrayMessages = TrayMessages {
    tooltip: "ImageMediaViewer (Running)",
    open_client: "Open Client",
    stay_in_tray: "Stay in Tray",
    rescan: "Rescan Folders",
    open_data_folder: "Open Data Folder",
    open_log_folder: "Open Log Folder",
    quit: "Quit",
};

/// 日本語言語リソース
pub const TRAY_JA: TrayMessages = TrayMessages {
    tooltip: "ImageMediaViewer (稼働中)",
    open_client: "クライアントを開く",
    stay_in_tray: "常駐する",
    rescan: "フォルダを再走査",
    open_data_folder: "データフォルダを開く",
    open_log_folder: "ログフォルダを開く",
    quit: "終了",
};

/// 指定されたロケールに対応するトレイメッセージを取得
///
/// 変更理由: 性能劣化を防ぐためヒープ割り当て（String）を行わず、
/// 'static ライフタイムの構造体参照を返却する。
/// 未知または未定義の言語が指定された場合は自動的に英語（TRAY_EN）へフォールバックする。
///
/// @param locale 言語コード ("ja", "en" 等)
/// @return トレイメッセージ構造体への不変静的参照
pub fn get_tray_messages(locale: &str) -> &'static TrayMessages {
    match locale {
        "ja" => &TRAY_JA,
        _ => &TRAY_EN, // デフォルト言語（英語）へフォールバック
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tray_messages_resolution_and_fallback() {
        // 日本語
        let ja = get_tray_messages("ja");
        assert_eq!(ja.open_client, "クライアントを開く");

        // 英語
        let en = get_tray_messages("en");
        assert_eq!(en.open_client, "Open Client");

        // 未知言語の場合は英語へ透過的にフォールバック
        let fallback = get_tray_messages("fr");
        assert_eq!(fallback.open_client, "Open Client");
        assert_eq!(fallback.quit, "Quit");
    }
}
