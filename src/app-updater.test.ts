import { describe, expect, test } from "vitest";
import {
  createAppUpdater,
  createUpdateState,
  type AvailableAppUpdate,
} from "./app-updater";

function availableUpdate(): AvailableAppUpdate {
  return {
    currentVersion: "0.1.5",
    version: "0.1.6",
    date: "2026-09-22T08:00:00Z",
    body: "Adds in-app updates.",
    downloadAndInstall: async () => undefined,
  };
}

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  const promise = new Promise<T>((promiseResolve) => {
    resolve = promiseResolve;
  });
  return { promise, resolve };
}

describe("app updater", () => {
  test("shows a discovered update and keeps its release metadata", async () => {
    const state = createUpdateState();
    const updater = createAppUpdater(state, {
      check: async () => availableUpdate(),
      relaunch: async () => undefined,
    });

    const result = await updater.check();

    expect(result).toEqual({ kind: "available", version: "0.1.6" });
    expect(state.phase).toBe("available");
    expect(state.dialogVisible).toBe(true);
    expect(state.update).toMatchObject({
      currentVersion: "0.1.5",
      version: "0.1.6",
      body: "Adds in-app updates.",
    });
  });

  test("records an up-to-date result without opening the dialog", async () => {
    const state = createUpdateState();
    const updater = createAppUpdater(state, {
      check: async () => null,
      relaunch: async () => undefined,
    });

    const result = await updater.check();

    expect(result).toEqual({ kind: "up-to-date" });
    expect(state.phase).toBe("idle");
    expect(state.dialogVisible).toBe(false);
    expect(state.update).toBeNull();
  });

  test("records check failures without opening the dialog", async () => {
    const state = createUpdateState();
    const updater = createAppUpdater(state, {
      check: async () => {
        throw new Error("release endpoint unavailable");
      },
      relaunch: async () => undefined,
    });

    const result = await updater.check();

    expect(result).toEqual({ kind: "error", message: "release endpoint unavailable" });
    expect(state.phase).toBe("error");
    expect(state.dialogVisible).toBe(false);
    expect(state.error).toBe("release endpoint unavailable");
  });

  test("tracks download progress and relaunches after installation", async () => {
    let relaunched = false;
    const update = availableUpdate();
    update.downloadAndInstall = async (onEvent) => {
      onEvent({ event: "Started", data: { contentLength: 100 } });
      onEvent({ event: "Progress", data: { chunkLength: 35 } });
      onEvent({ event: "Progress", data: { chunkLength: 25 } });
      onEvent({ event: "Finished" });
    };
    const state = createUpdateState();
    const updater = createAppUpdater(state, {
      check: async () => update,
      relaunch: async () => {
        relaunched = true;
      },
    });
    await updater.check();

    const result = await updater.install();

    expect(result).toEqual({ kind: "installed" });
    expect(state.phase).toBe("installing");
    expect(state.downloadedBytes).toBe(100);
    expect(state.totalBytes).toBe(100);
    expect(relaunched).toBe(true);
  });

  test("keeps the dialog open and does not relaunch when installation fails", async () => {
    let relaunched = false;
    const update = availableUpdate();
    update.downloadAndInstall = async () => {
      throw new Error("signature verification failed");
    };
    const state = createUpdateState();
    const updater = createAppUpdater(state, {
      check: async () => update,
      relaunch: async () => {
        relaunched = true;
      },
    });
    await updater.check();

    const result = await updater.install();

    expect(result).toEqual({ kind: "error", message: "signature verification failed" });
    expect(state.phase).toBe("error");
    expect(state.dialogVisible).toBe(true);
    expect(state.error).toBe("signature verification failed");
    expect(relaunched).toBe(false);
  });

  test("only dismisses an update before its download starts", async () => {
    let finishDownload: (() => void) | undefined;
    const update = availableUpdate();
    update.downloadAndInstall = () => new Promise<void>((resolve) => {
      finishDownload = resolve;
    });
    const state = createUpdateState();
    const updater = createAppUpdater(state, {
      check: async () => update,
      relaunch: async () => undefined,
    });
    await updater.check();

    const install = updater.install();
    const dismissedWhileDownloading = updater.dismiss();

    expect(dismissedWhileDownloading).toBe(false);
    expect(state.dialogVisible).toBe(true);
    finishDownload?.();
    await install;
  });

  test("rejects checks while an installation is active", async () => {
    const download = deferred<void>();
    const update = availableUpdate();
    update.downloadAndInstall = () => download.promise;
    let checkCount = 0;
    const state = createUpdateState();
    const updater = createAppUpdater(state, {
      check: async () => {
        checkCount += 1;
        return update;
      },
      relaunch: async () => undefined,
    });
    await updater.check();

    const installation = updater.install();
    const result = await updater.check();

    expect(result).toEqual({ kind: "busy" });
    expect(checkCount).toBe(1);
    expect(state.phase).toBe("downloading");
    download.resolve();
    await installation;
  });

  test("rejects installation while an update check is active", async () => {
    const checkedUpdate = deferred<AvailableAppUpdate | null>();
    const state = createUpdateState();
    const updater = createAppUpdater(state, {
      check: () => checkedUpdate.promise,
      relaunch: async () => undefined,
    });

    const checking = updater.check();
    const result = await updater.install();

    expect(result).toEqual({ kind: "busy" });
    expect(state.phase).toBe("checking");
    checkedUpdate.resolve(availableUpdate());
    await checking;
  });

  test("enters the installing phase as soon as the download finishes", async () => {
    const installation = deferred<void>();
    const update = availableUpdate();
    update.downloadAndInstall = async (onEvent) => {
      onEvent({ event: "Started", data: { contentLength: 100 } });
      onEvent({ event: "Progress", data: { chunkLength: 100 } });
      onEvent({ event: "Finished" });
      await installation.promise;
    };
    const state = createUpdateState();
    const updater = createAppUpdater(state, {
      check: async () => update,
      relaunch: async () => undefined,
    });
    await updater.check();

    const installing = updater.install();
    await Promise.resolve();

    expect(state.phase).toBe("installing");
    installation.resolve();
    await installing;
  });

  test("keeps an installed update ready to restart when relaunch fails", async () => {
    let relaunchCount = 0;
    const state = createUpdateState();
    const updater = createAppUpdater(state, {
      check: async () => availableUpdate(),
      relaunch: async () => {
        relaunchCount += 1;
        if (relaunchCount === 1) {
          throw new Error("relaunch denied");
        }
      },
    });
    await updater.check();

    const installResult = await updater.install();

    expect(installResult).toEqual({ kind: "restart-required", message: "relaunch denied" });
    expect(state.phase).toBe("restart-required");
    expect(state.error).toBe("relaunch denied");

    const restartResult = await updater.restart();

    expect(restartResult).toEqual({ kind: "installed" });
    expect(relaunchCount).toBe(2);
  });

  test("preserves restart-required against new checks and installs", async () => {
    let checkCount = 0;
    let downloadCount = 0;
    const update = availableUpdate();
    update.downloadAndInstall = async () => {
      downloadCount += 1;
    };
    const state = createUpdateState();
    const updater = createAppUpdater(state, {
      check: async () => {
        checkCount += 1;
        return update;
      },
      relaunch: async () => {
        throw new Error("relaunch denied");
      },
    });
    await updater.check();
    await updater.install();

    const checkResult = await updater.check();
    const installResult = await updater.install();

    expect(checkResult).toEqual({ kind: "restart-required", message: "relaunch denied" });
    expect(installResult).toEqual({ kind: "restart-required", message: "relaunch denied" });
    expect(checkCount).toBe(1);
    expect(downloadCount).toBe(1);
    expect(state.phase).toBe("restart-required");
  });

  test("closes the native update resource after installation completes", async () => {
    let closed = 0;
    const update = availableUpdate();
    update.close = async () => {
      closed += 1;
    };
    const state = createUpdateState();
    const updater = createAppUpdater(state, {
      check: async () => update,
      relaunch: async () => {
        throw new Error("relaunch denied");
      },
    });
    await updater.check();

    await updater.install();

    expect(closed).toBe(1);
    expect(state.phase).toBe("restart-required");
  });

  test("closes an update resource when a later check supersedes it", async () => {
    let closed = 0;
    const firstUpdate = availableUpdate();
    firstUpdate.close = async () => {
      closed += 1;
    };
    const secondUpdate = { ...availableUpdate(), version: "0.1.7" };
    const checks = [firstUpdate, secondUpdate];
    const state = createUpdateState();
    const updater = createAppUpdater(state, {
      check: async () => checks.shift() ?? null,
      relaunch: async () => undefined,
    });
    await updater.check();

    await updater.check();

    expect(closed).toBe(1);
    expect(state.update).toBe(secondUpdate);
  });
});
