import { commands, type FcmPreview } from "@/commands/bindings";
import Mods from "@/commands/mods";
import { useModInstallation } from "@/hooks/mods/useModInstallation";
import { resourceListStoreSync } from "@/stores/resourceList";
import { useProfilesStore } from "@/stores/profiles";
import { act, renderHook } from "@testing-library/react";
import { vi } from "vitest";

vi.mock("@/commands/bindings", () => ({
  commands: {
    fcmDetectImport: vi.fn(),
    fcmProbePrerequisites: vi.fn(),
    fcmPreviewImport: vi.fn(),
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
    getState: vi.fn(),
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

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
  vi.mocked(commands.fcmDetectImport).mockResolvedValue(true);
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
