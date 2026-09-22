export type DownloadEvent =
  | { event: "Started"; data: { contentLength?: number } }
  | { event: "Progress"; data: { chunkLength: number } }
  | { event: "Finished"; data?: Record<string, never> };

export type AvailableAppUpdate = {
  currentVersion: string;
  version: string;
  date?: string;
  body?: string;
  downloadAndInstall: (onEvent: (event: DownloadEvent) => void) => Promise<void>;
  close?: () => Promise<void>;
};

export type UpdateState = {
  phase: "idle" | "checking" | "available" | "downloading" | "installing" | "restart-required" | "error";
  dialogVisible: boolean;
  update: AvailableAppUpdate | null;
  downloadedBytes: number;
  totalBytes: number | null;
  error: string;
};

type UpdaterDependencies = {
  check: () => Promise<AvailableAppUpdate | null>;
  relaunch: () => Promise<void>;
};

export function createUpdateState(): UpdateState {
  return {
    phase: "idle",
    dialogVisible: false,
    update: null,
    downloadedBytes: 0,
    totalBytes: null,
    error: "",
  };
}

export function createAppUpdater(state: UpdateState, dependencies: UpdaterDependencies) {
  let activeOperation: "check" | "install" | "restart" | null = null;

  async function closeUpdate(update: AvailableAppUpdate) {
    try {
      await update.close?.();
    } catch {
      // Releasing a native resource must not hide a valid update result.
    }
  }

  async function closeSupersededUpdate(previous: AvailableAppUpdate | null, next: AvailableAppUpdate | null) {
    if (previous && previous !== next) {
      await closeUpdate(previous);
    }
  }

  function errorMessage(caught: unknown) {
    return caught instanceof Error ? caught.message : String(caught);
  }

  return {
    async check() {
      if (activeOperation) {
        return { kind: "busy" } as const;
      }
      if (state.phase === "restart-required") {
        return { kind: "restart-required", message: state.error } as const;
      }
      activeOperation = "check";
      state.phase = "checking";
      state.error = "";
      try {
        const update = await dependencies.check();
        const previous = state.update;
        if (!update) {
          state.phase = "idle";
          state.update = null;
          await closeSupersededUpdate(previous, null);
          return { kind: "up-to-date" } as const;
        }
        state.update = update;
        state.phase = "available";
        state.dialogVisible = true;
        await closeSupersededUpdate(previous, update);
        return { kind: "available", version: update.version } as const;
      } catch (caught) {
        const message = errorMessage(caught);
        state.phase = "error";
        state.error = message;
        return { kind: "error", message } as const;
      } finally {
        activeOperation = null;
      }
    },
    async install() {
      if (activeOperation) {
        return { kind: "busy" } as const;
      }
      if (state.phase === "restart-required") {
        return { kind: "restart-required", message: state.error } as const;
      }
      const update = state.update;
      if (!update) {
        return { kind: "error", message: "No update is available" } as const;
      }
      activeOperation = "install";
      state.phase = "downloading";
      state.downloadedBytes = 0;
      state.totalBytes = null;
      state.error = "";
      try {
        await update.downloadAndInstall((event) => {
          if (event.event === "Started") {
            state.totalBytes = event.data.contentLength ?? null;
            return;
          }
          if (event.event === "Progress") {
            state.downloadedBytes += event.data.chunkLength;
            return;
          }
          if (state.totalBytes !== null) {
            state.downloadedBytes = state.totalBytes;
          }
          state.phase = "installing";
        });
        state.phase = "installing";
        await closeUpdate(update);
      } catch (caught) {
        const message = errorMessage(caught);
        state.phase = "error";
        state.error = message;
        activeOperation = null;
        return { kind: "error", message } as const;
      }
      try {
        await dependencies.relaunch();
        return { kind: "installed" } as const;
      } catch (caught) {
        const message = errorMessage(caught);
        state.phase = "restart-required";
        state.error = message;
        return { kind: "restart-required", message } as const;
      } finally {
        activeOperation = null;
      }
    },
    async restart() {
      if (activeOperation) {
        return { kind: "busy" } as const;
      }
      if (state.phase !== "restart-required") {
        return { kind: "error", message: "No installed update is waiting to restart" } as const;
      }
      activeOperation = "restart";
      state.phase = "installing";
      state.error = "";
      try {
        await dependencies.relaunch();
        return { kind: "installed" } as const;
      } catch (caught) {
        const message = errorMessage(caught);
        state.phase = "restart-required";
        state.error = message;
        return { kind: "restart-required", message } as const;
      } finally {
        activeOperation = null;
      }
    },
    dismiss() {
      if (state.phase === "downloading" || state.phase === "installing") {
        return false;
      }
      state.dialogVisible = false;
      return true;
    },
  };
}
