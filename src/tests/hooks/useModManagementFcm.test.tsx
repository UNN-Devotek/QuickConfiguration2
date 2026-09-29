import { commands, type FcmPreview } from "@/commands/bindings";
import Mods from "@/commands/mods";
import { useModManagement } from "@/hooks/mods/useModManagement";
import { FCM_MOD_KEY, modsEventBus } from "@/services/mods";
import { resourceListStoreSync } from "@/stores/resourceList";
import { useProfilesStore } from "@/stores/profiles";
import { useToastsStore } from "@/stores/toasts";
import { act, renderHook, waitFor } from "@testing-library/react";
import { vi } from "vitest";

vi.mock("@/commands/bindings", () => ({
  commands: {
    fcmPreviewRemove: vi.fn(),
    fcmApply: vi.fn(),
    fcmDiscard: vi.fn(),
    iniLoad: vi.fn(),
  },
}));

vi.mock("@/commands/mods", () => ({
  default: { actions: { mod: { uninstall: vi.fn() } } },
}));

vi.mock("@/stores/resourceList", () => ({
  resourceListStoreSync: {
    flushSave: vi.fn(),
    cancelSave: vi.fn(),
    load: vi.fn(),
  },
}));

vi.mock("@/stores/mods", () => {
  const mod = { key: "ordinary", title: "Ordinary mod" };
  const store = {
    getMod: (key: string) => (key === mod.key ? mod : undefined),
    getManagedMods: () => ({ enabled: true, mods: [mod], state: [] }),
  };
  return {
    updateModsStore: vi.fn(),
    useModsStore: Object.assign(
      (select: (state: typeof store) => typeof store.getMod) => select(store),
      { getState: () => store },
    ),
  };
});

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const removalPreview: FcmPreview = {
  token: "remove-token",
  action: "remove",
  provider: "not required",
  installed: "HUD",
  package: null,
  changes: [
    { path: "/game/Data/FCMChatWidget.ba2", description: "Remove HUD" },
  ],
};

beforeEach(() => {
  useProfilesStore.getState().setStore({
    profiles: [
      {
        key: "profile",
        title: "Test",
        installationPath: "/game",
        iniPath: "/ini",
        iniPrefix: "Fallout76",
        modsPath: "/mods",
        executableName: "Fallout76.exe",
        execParameters: "",
        launcherURL: "",
        gameEdition: "Steam",
        launchOption: "OpenURL",
      },
    ],
    selected: "profile",
  });
  vi.mocked(commands.fcmPreviewRemove).mockResolvedValue(removalPreview);
  vi.mocked(commands.fcmApply).mockResolvedValue("/backups/fcm");
  vi.mocked(commands.fcmDiscard).mockResolvedValue(null);
  vi.mocked(resourceListStoreSync.flushSave).mockResolvedValue();
  vi.mocked(resourceListStoreSync.load).mockResolvedValue();
});

afterEach(() => vi.clearAllMocks());

it("uses the normal delete confirmation and FCM cleanup for an installed HUD", async () => {
  const changed = vi.spyOn(modsEventBus, "emitFcmChanged");
  const hook = renderHook(() => useModManagement());
  act(() => hook.result.current.deleteMod(FCM_MOD_KEY));
  await waitFor(() =>
    expect(hook.result.current.deleteModModalProps.show).toBe(true),
  );
  expect(commands.fcmPreviewRemove).toHaveBeenCalledWith(
    "/game",
    "/ini",
    "Fallout76",
  );
  expect(hook.result.current.deleteModModalProps.mod?.key).toBe(FCM_MOD_KEY);
  expect(hook.result.current.deleteModModalProps.fcmChanges).toEqual(
    removalPreview.changes,
  );
  act(() => hook.result.current.deleteModModalProps.onConfirm(FCM_MOD_KEY));
  await waitFor(() =>
    expect(commands.fcmApply).toHaveBeenCalledWith("remove-token"),
  );
  await waitFor(() =>
    expect(commands.iniLoad).toHaveBeenCalledWith("/ini", "Fallout76"),
  );
  expect(Mods.actions.mod.uninstall).not.toHaveBeenCalled();
  expect(changed).toHaveBeenCalledTimes(1);
  changed.mockRestore();
});

it("keeps ordinary mods on the existing uninstall command", async () => {
  const hook = renderHook(() => useModManagement());
  act(() => hook.result.current.deleteMod("ordinary"));
  expect(hook.result.current.deleteModModalProps.mod?.key).toBe("ordinary");
  expect(hook.result.current.deleteModModalProps.fcmChanges).toBeUndefined();
  act(() => hook.result.current.deleteModModalProps.onConfirm("ordinary"));
  await waitFor(() =>
    expect(Mods.actions.mod.uninstall).toHaveBeenCalledTimes(1),
  );
  expect(commands.fcmPreviewRemove).not.toHaveBeenCalled();
});

it("discards a canceled removal preview", async () => {
  const hook = renderHook(() => useModManagement());
  act(() => hook.result.current.deleteMod(FCM_MOD_KEY));
  await waitFor(() =>
    expect(hook.result.current.deleteModModalProps.show).toBe(true),
  );
  act(() => hook.result.current.deleteModModalProps.onAbort());
  expect(commands.fcmDiscard).toHaveBeenCalledWith("remove-token");
  expect(commands.fcmApply).not.toHaveBeenCalled();
});

it("reports a successful removal even when reloading settings fails", async () => {
  vi.mocked(commands.iniLoad).mockRejectedValueOnce(new Error("reload failed"));
  const changed = vi.spyOn(modsEventBus, "emitFcmChanged");
  const hook = renderHook(() => useModManagement());
  act(() => hook.result.current.deleteMod(FCM_MOD_KEY));
  await waitFor(() =>
    expect(hook.result.current.deleteModModalProps.show).toBe(true),
  );
  act(() => hook.result.current.deleteModModalProps.onConfirm(FCM_MOD_KEY));
  await waitFor(() => expect(changed).toHaveBeenCalledTimes(1));
  await waitFor(() =>
    expect(useToastsStore.getState().toasts.at(-1)?.variant).toBe("warning"),
  );
  expect(commands.fcmApply).toHaveBeenCalledTimes(1);
  expect(Mods.actions.mod.uninstall).not.toHaveBeenCalled();
  changed.mockRestore();
});
