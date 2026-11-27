export interface TauriAppWindow {
  isMaximized(): Promise<boolean>;
  minimize(): Promise<void>;
  maximize(): Promise<void>;
  unmaximize(): Promise<void>;
  toggleMaximize?(): Promise<void>;
  isFullscreen?(): Promise<boolean>;
  setFullscreen?(fullscreen: boolean): Promise<void>;
  close?(): Promise<void>;
  setAlwaysOnTop?(alwaysOnTop: boolean): Promise<void>;
  setSize?(size: unknown): Promise<void>;
  onResized?(cb: () => void): Promise<() => void>;
}

export interface TauriGlobal {
  core?: {
    invoke<T = unknown>(cmd: string, args?: Record<string, unknown>): Promise<T>;
  };
  event?: {
    listen<T = unknown>(
      event: string,
      handler: (event: { payload: T }) => void
    ): Promise<() => void>;
  };
  window?: {
    getCurrentWindow(): TauriAppWindow;
  };
  dpi?: {
    LogicalSize: new (width: number, height: number) => unknown;
  };
}

declare global {
  interface Window {
    __TAURI__?: TauriGlobal;
  }
}

export {};
