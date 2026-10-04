/**
 * ムードボード（グループ）情報
 */
export interface Board {
  id: number;
  name: string;
  backgroundColor: string;
  panX: number;
  panY: number;
  zoom: number;
  itemCount: number;
  createdAt: number;
  updatedAt: number;
}

/**
 * ムードボード上の配置アイテム情報（非破壊変形・クリッピング）
 */
export interface BoardItem {
  id: number;
  boardId: number;
  imageId: number;
  x: number;
  y: number;
  width: number;
  height: number;
  scale: number;
  rotation: number;
  zIndex: number;
  cropX: number;
  cropY: number;
  cropW: number;
  cropH: number;
  opacity: number;
  isLocked: boolean;
  flipH: boolean;
  flipV: boolean;
  rev: number;
}

/**
 * ボード新規作成ペイロード
 */
export interface CreateBoardPayload {
  name: string;
  backgroundColor?: string;
}

/**
 * ボード設定・カメラ更新ペイロード
 */
export interface UpdateBoardPayload {
  name?: string;
  backgroundColor?: string;
  panX?: number;
  panY?: number;
  zoom?: number;
}

/**
 * ボードアイテム変形・クリッピング更新ペイロード
 */
export interface UpdateBoardItemPayload {
  id?: number;
  x?: number;
  y?: number;
  width?: number;
  height?: number;
  scale?: number;
  rotation?: number;
  zIndex?: number;
  cropX?: number;
  cropY?: number;
  cropW?: number;
  cropH?: number;
  opacity?: number;
  isLocked?: boolean;
  flipH?: boolean;
  flipV?: boolean;
}
