import { describe, it, expect, beforeEach } from "vitest";
import { useAppStore } from "../../store";

describe("通信状態管理 (ConnectionStatus)", () => {
  beforeEach(() => {
    // ストアの通信状態をリセット
    useAppStore.setState({
      connectionStatus: "connected",
      lastConnectedAt: Date.now(),
    });
  });

  it("初期状態は connected で最終通信時刻が記録されていること", () => {
    const state = useAppStore.getState();
    expect(state.connectionStatus).toBe("connected");
    expect(state.lastConnectedAt).toBeGreaterThan(0);
  });

  it("markDisconnected を呼び出すと disconnected に遷移すること", () => {
    useAppStore.getState().markDisconnected();
    expect(useAppStore.getState().connectionStatus).toBe("disconnected");
  });

  it("markConnected を呼び出すと connected に遷移し lastConnectedAt が更新されること", () => {
    useAppStore.getState().markDisconnected();
    expect(useAppStore.getState().connectionStatus).toBe("disconnected");

    const before = Date.now();
    useAppStore.getState().markConnected();
    expect(useAppStore.getState().connectionStatus).toBe("connected");
    expect(useAppStore.getState().lastConnectedAt).toBeGreaterThanOrEqual(before);
  });

  it("setConnectionStatus で connecting 状態へ遷移できること", () => {
    useAppStore.getState().setConnectionStatus("connecting");
    expect(useAppStore.getState().connectionStatus).toBe("connecting");
  });
});
