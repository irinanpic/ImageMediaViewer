import { en, type TranslationSchema } from "./en";
import { ja } from "./ja";
import { useAppStore } from "../store";

/** サポートするロケール定義 */
export type Locale = "ja" | "en";

/** デフォルト言語（フォールバック先） */
export const DEFAULT_LOCALE: Locale = "en";

/** ネストされたオブジェクトのキーをドット区切りパス文字列として再帰的に生成する型 */
export type NestedKeyOf<ObjectType extends object> = {
  [Key in keyof ObjectType & (string | number)]: ObjectType[Key] extends object
    ? `${Key}.${NestedKeyOf<ObjectType[Key]>}`
    : `${Key}`;
}[keyof ObjectType & (string | number)];

/** 翻訳キーの型（en.ts をマスターとして自動推論） */
export type TranslationKey = NestedKeyOf<TranslationSchema>;

/** 登録済み言語リソース辞書マップ */
const resources: Record<Locale, any> = {
  en,
  ja,
};

/**
 * オブジェクトからドット区切りのパスで値を取得する内部ヘルパー
 *
 * 性能最適化:
 * - 大半のキーは "category.key" の2階層であるため、indexOf(".") による直接切り出しを行い、
 *   split(".") による配列ヒープ確保（ガベージコレクション負荷）をホットパスで完全に排除する。
 *
 * @param obj 探索対象オブジェクト
 * @param path ドット区切りのキーパス (例: "sidebar.folders")
 * @returns 取得した値（文字列）、または未定義時は undefined
 */
function getNestedValue(obj: any, path: string): string | undefined {
  if (!obj || typeof obj !== "object") return undefined;

  const dotIdx = path.indexOf(".");
  if (dotIdx !== -1) {
    const section = path.slice(0, dotIdx);
    const subObj = obj[section];
    if (!subObj || typeof subObj !== "object") return undefined;

    const remaining = path.slice(dotIdx + 1);
    const nextDot = remaining.indexOf(".");
    if (nextDot === -1) {
      // 2階層の通常ケース: 配列生成ゼロ
      const val = subObj[remaining];
      return typeof val === "string" ? val : undefined;
    }
  }

  // 3階層以上のフォールバック
  const parts = path.split(".");
  let current = obj;
  for (const part of parts) {
    if (current === null || current === undefined || typeof current !== "object") {
      return undefined;
    }
    current = current[part];
  }
  return typeof current === "string" ? current : undefined;
}

/**
 * テンプレート文字列内の `{paramName}` を引数のパラメータ値に置換する
 *
 * 性能最適化:
 * - params が未指定、またはテンプレートに "{" が含まれない静的テキストの場合、
 *   正規表現エンジンをバイパスして即座に文字列参照を返却する。
 *
 * @param template プレースホルダーを含むテンプレート文字列
 * @param params 置換パラメータマップ
 * @returns 置換後の文字列
 */
function formatTemplate(template: string, params?: Record<string, string | number>): string {
  if (!params || template.indexOf("{") === -1) return template;
  return template.replace(/\{(\w+)\}/g, (match, key) => {
    return params[key] !== undefined ? String(params[key]) : match;
  });
}

/**
 * 指定されたキーに対応する翻訳テキストを取得する関数
 *
 * 設計方針・フォールバック仕様:
 * 1. 指定された言語（または現在の言語）のリソース辞書からキーを検索。
 * 2. 翻訳が存在しない場合、または空文字の場合、自動的にデフォルト言語（英語: `en`）のリソースを透過的に参照。
 * 3. 英語リソースにもキーが存在しない場合は、キー名そのものを返却してUIのクラッシュを防止する。
 * 4. `{count}` などのパラメータプレースホルダーを置換。
 *
 * @param key 翻訳キー (例: "sidebar.folders", "status.totalImages")
 * @param params 動的置換パラメータ (例: { count: 120 })
 * @param targetLocale 明示的なロケール指定（省略時はグローバルストアの現在の言語）
 * @returns 翻訳済み文字列
 */
export function t(
  key: TranslationKey | (string & {}),
  params?: Record<string, string | number>,
  targetLocale?: Locale
): string {
  const currentLocale: Locale = targetLocale || useAppStore.getState().locale || "ja";

  // 1. 現在の言語リソースから検索
  let text = getNestedValue(resources[currentLocale], key);

  // 2. 該当キーが未定義または空文字の場合、デフォルト言語（en）へ透過的にフォールバック
  if (text === undefined || text === "") {
    text = getNestedValue(resources[DEFAULT_LOCALE], key);
  }

  // 3. デフォルト言語にも存在しない場合はキー名をそのまま返却
  if (text === undefined) {
    return key;
  }

  // 4. パラメータの置換
  return formatTemplate(text, params);
}

/**
 * React コンポーネント用の多言語翻訳フック
 *
 * ロケールの切り替えを検知してコンポーネントを再レンダリングする。
 *
 * @returns `{ t, locale, setLocale }`
 */
export function useTranslation() {
  const locale = useAppStore((state) => state.locale);
  const setLocale = useAppStore((state) => state.setLocale);

  const translate = (
    key: TranslationKey | (string & {}),
    params?: Record<string, string | number>
  ) => {
    return t(key, params, locale);
  };

  return {
    t: translate,
    locale,
    setLocale,
  };
}

export { translateLogMessage } from "./logTranslator";

