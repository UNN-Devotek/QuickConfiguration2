import {
  commands,
  type FcmPreview,
  type ManagedMod,
  type ModInstallationState,
} from "@/commands/bindings";
import Mods from "@/commands/mods";
import { useModInstallation } from "@/hooks/mods/useModInstallation";
import { resourceListStoreSync } from "@/stores/resourceList";
import { useProfilesStore } from "@/stores/profiles";
import { act, renderHook } from "@testing-library/react";
import { vi } from "vitest";

const managed = vi.hoisted(() => ({
  mods: [] as ManagedMod[],
  state: [] as ModInstallationState[],
}));

vi.mock("@/commands/bindings", () => ({
  commands: {
    fcmDetectImport: vi.fn(),
    fcmManagedOwner: vi.fn(),
    fcmProbePrerequisites: vi.fn(),
    fcmPreviewImport: vi.fn(),
    fcmDiscard: vi.fn(),
  },
}));

vi.mock("@/commands/mods", () => ({
  default: {
    actions: {
      tempFolder: {
        createFromFileOrArchive: vi.fn(),
      },
    },
  },
}));

vi.mock("@/stores/resourceList", () => ({
  resourceListStoreSync: {
    flushSave: vi.fn(),
  },
}));

vi.mock("@/stores/mods", () => ({
  updateModsStore: vi.fn(),
  useModsStore: {
    getState: () => ({ getManagedMods: () => managed }),
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

beforeEach(() => {
  managed.mods = [];
  managed.state = [];
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
  vi.mocked(commands.fcmDetectImport).mockResolvedValue(true);
  vi.mocked(commands.fcmManagedOwner).mockResolvedValue(null);
  vi.mocked(commands.fcmProbePrerequisites).mockResolvedValue({
    provider: "zfe",
    hudModLoader: true,
  });
  vi.mocked(commands.fcmPreviewImport).mockResolvedValue({
    token: "preview-token",
    action: "installBridge",
    provider: "zfe",
    installed: null,
    package: null,
    changes: [],
  } satisfies FcmPreview);
  vi.mocked(commands.fcmDiscard).mockResolvedValue(null);
  vi.mocked(resourceListStoreSync.flushSave).mockResolvedValue();
});

afterEach(() => vi.clearAllMocks());

it("routes an FCM ZIP through safe preview before ordinary mod staging", async () => {
  const hook = renderHook(() => useModInstallation());
  await act(async () => {
    await hook.result.current.installFromFileWithPath(
      "/downloads/overlay.zip",
      {},
    );
  });
  expect(commands.fcmDetectImport).toHaveBeenCalledWith([
    "/downloads/overlay.zip",
  ]);
  expect(commands.fcmPreviewImport).toHaveBeenCalledWith(
    "/game",
    "/ini",
    "Fallout76",
    ["/downloads/overlay.zip"],
    null,
    null,
    null,
  );
  expect(
    Mods.actions.tempFolder.createFromFileOrArchive,
  ).not.toHaveBeenCalled();
  expect(hook.result.current.fcmModalProps.preview?.token).toBe(
    "preview-token",
  );
});

it.each([
  ["Data", "FCMChatWidget.ba2"],
  [".", "Data\\FCMServerBridge.ba2"],
  ["Data/sub/..", "FCMChatWidget.ba2"],
])(
  "keeps a managed FCM archive at %s/%s on the normal path",
  async (rootFolder, file) => {
    managed.mods = [{ key: "managed-fcm", title: "Managed FCM" } as ManagedMod];
    managed.state = [{ key: "managed-fcm", rootFolder, files: [file] }];
    vi.mocked(commands.fcmManagedOwner).mockResolvedValue(managed.mods[0]);
    const hook = renderHook(() => useModInstallation());
    await act(async () => {
      await hook.result.current.installFromFileWithPath(
        "/downloads/hud.zip",
        {},
      );
    });
    expect(commands.fcmProbePrerequisites).not.toHaveBeenCalled();
    expect(commands.fcmPreviewImport).not.toHaveBeenCalled();
    expect(hook.result.current.fcmModalProps.preview).toBeNull();
  },
);

it("blocks guided import when a staged managed FCM mod owns the archive", async () => {
  managed.mods = [{ key: "staged-fcm", title: "Staged FCM" } as ManagedMod];
  vi.mocked(commands.fcmManagedOwner).mockResolvedValue(managed.mods[0]);
  const hook = renderHook(() => useModInstallation());
  await act(async () => {
    await hook.result.current.installFromFileWithPath("/downloads/hud.zip", {});
  });
  expect(commands.fcmManagedOwner).toHaveBeenCalledWith("/mods", managed);
  expect(commands.fcmPreviewImport).not.toHaveBeenCalled();
});

it("discards a canceled preview", async () => {
  const hook = renderHook(() => useModInstallation());
  await act(async () => {
    await hook.result.current.installFromFileWithPath("/downloads/hud.zip", {});
  });
  act(() => hook.result.current.fcmModalProps.onAbort());
  expect(commands.fcmDiscard).toHaveBeenCalledWith("preview-token");
});

it("asks for a provider and HUDModLoader inside the normal import flow when missing", async () => {
  vi.mocked(commands.fcmProbePrerequisites).mockResolvedValue({
    provider: null,
    hudModLoader: false,
  });
  const hook = renderHook(() => useModInstallation());
  await act(async () => {
    await hook.result.current.installFromFileWithPath("/downloads/hud.zip", {});
  });
  expect(hook.result.current.fcmPrerequisiteProps.request).toEqual({
    paths: ["/downloads/hud.zip"],
    probe: { provider: null, hudModLoader: false },
    profile: {
      key: "profile",
      installationPath: "/game",
      iniPath: "/ini",
      iniPrefix: "Fallout76",
    },
  });
  expect(commands.fcmPreviewImport).not.toHaveBeenCalled();
  expect(
    Mods.actions.tempFolder.createFromFileOrArchive,
  ).not.toHaveBeenCalled();
});

it("discards an import preview when the profile changes during inspection", async () => {
  let resolveProbe: (value: {
    provider: string;
    hudModLoader: boolean;
  }) => void = () => undefined;
  vi.mocked(commands.fcmProbePrerequisites).mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        resolveProbe = resolve;
      }),
  );
  const hook = renderHook(() => useModInstallation());
  await act(async () => {
    const installation = hook.result.current.installFromFileWithPath(
      "/downloads/hud.zip",
      {},
    );
    await Promise.resolve();
    useProfilesStore.getState().setStore({ selected: "other-profile" });
    resolveProbe({ provider: "zfe", hudModLoader: true });
    await installation;
  });
  expect(commands.fcmPreviewImport).not.toHaveBeenCalled();
  expect(hook.result.current.fcmModalProps.preview).toBeNull();
});
