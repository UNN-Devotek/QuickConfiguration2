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
    fcmPreviewRemove: vi.fn(),
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
  });
  expect(commands.fcmPreviewImport).not.toHaveBeenCalled();
  expect(
    Mods.actions.tempFolder.createFromFileOrArchive,
  ).not.toHaveBeenCalled();
});

it("previews removal through the dedicated command", async () => {
  vi.mocked(commands.fcmPreviewRemove).mockResolvedValue({
    token: "remove-token",
    action: "remove",
    provider: "not required",
    installed: "HUD",
    package: null,
    changes: [],
  });
  const hook = renderHook(() => useModInstallation());
  await act(async () => {
    await hook.result.current.previewFcmRemoval();
  });
  expect(commands.fcmPreviewRemove).toHaveBeenCalledWith(
    "/game",
    "/ini",
    "Fallout76",
  );
  expect(hook.result.current.fcmModalProps.preview?.action).toBe("remove");
});
