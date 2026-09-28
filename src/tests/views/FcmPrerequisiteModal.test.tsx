import { commands, type FcmPreview } from "@/commands/bindings";
import NexusMods from "@/commands/nexusmods";
import { nxmLinksQueueService } from "@/services/nxm";
import { useProfilesStore } from "@/stores/profiles";
import { useSettingsStore } from "@/stores/settings";
import FcmPrerequisiteModal, {
  latestMainZip,
} from "@/views/mods/tabs/modOrder/modals/modInstallation/FcmPrerequisiteModal";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { vi } from "vitest";

vi.mock("@/commands/bindings", () => ({
  commands: {
    fcmPrerequisiteDownloadLinks: vi.fn(),
    downloadWithProgress: vi.fn(),
    fcmPreviewImport: vi.fn(),
    nxmIsRegistered: vi.fn(),
    nxmRegister: vi.fn(),
  },
}));
vi.mock("@/commands/nexusmods", () => ({
  default: {
    api: { listModFiles: vi.fn(), requestDownloadLinks: vi.fn() },
    extractDetailsFromNxmUrl: vi.fn(),
  },
}));
vi.mock("@/stores/nexusmods", () => ({
  nexusmodsStoreAccountSync: { load: vi.fn().mockResolvedValue(undefined) },
  useNexusModsStore: { getState: () => ({ apiKey: "test-key" }) },
}));
vi.mock("@/services/nxm", () => ({
  nxmLinksQueueService: { subscribe: vi.fn(), consume: vi.fn() },
}));
vi.mock("@tauri-apps/plugin-shell", () => ({ open: vi.fn() }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const preview: FcmPreview = {
  token: "review",
  action: "installHud",
  provider: "xscal",
  installed: null,
  package: null,
  changes: [],
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
  useSettingsStore.getState().setModManagerSettings((settings) => ({
    ...settings,
    downloadPath: "/downloads",
  }));
  vi.mocked(NexusMods.api.listModFiles).mockImplementation(
    async (_key, _game, modId) => ({
      fileUpdates: [],
      files: [
        {
          fileId: modId === 4183 ? 100 : 200,
          fileName: "package.zip",
          name: modId === 4183 ? "xScal" : "HUDModLoader",
          version: "1.0",
          uploadedTime: "2026-09-28T00:00:00Z",
        },
      ] as never,
    }),
  );
  vi.mocked(commands.fcmPrerequisiteDownloadLinks).mockResolvedValue([
    { uri: "https://example.com/package.zip", name: "CDN", shortName: "CDN" },
  ]);
  vi.mocked(commands.downloadWithProgress)
    .mockResolvedValueOnce("/downloads/provider.zip")
    .mockResolvedValueOnce("/downloads/loader.zip");
  vi.mocked(commands.fcmPreviewImport).mockResolvedValue(preview);
});

afterEach(() => vi.clearAllMocks());

it("selects the newest main ZIP by upload time", () => {
  const files = [
    { fileId: 1, fileName: "old.zip", uploadedTime: "2026-09-01T00:00:00Z" },
    { fileId: 2, fileName: "new.zip", uploadedTime: "2026-09-28T00:00:00Z" },
    { fileId: 3, fileName: "ignored.7z", uploadedTime: "2026-09-29T00:00:00Z" },
  ];
  expect(latestMainZip(files as never).fileId).toBe(2);
});

it("downloads the selected provider and HUDModLoader before one combined preview", async () => {
  const onPreview = vi.fn();
  render(
    <FcmPrerequisiteModal
      request={{
        paths: ["/downloads/hud.zip"],
        probe: { provider: null, hudModLoader: false },
      }}
      onAbort={vi.fn()}
      onPreview={onPreview}
    />,
  );
  fireEvent.click(screen.getByLabelText("xScal"));
  fireEvent.click(
    screen.getByRole("button", { name: "fcmImport.downloadPrerequisites" }),
  );
  await waitFor(() => expect(onPreview).toHaveBeenCalledWith(preview));
  expect(NexusMods.api.listModFiles).toHaveBeenNthCalledWith(
    1,
    "test-key",
    "fallout76",
    4183,
    "main",
  );
  expect(NexusMods.api.listModFiles).toHaveBeenNthCalledWith(
    2,
    "test-key",
    "fallout76",
    3144,
    "main",
  );
  expect(commands.fcmPreviewImport).toHaveBeenCalledWith(
    "/game",
    "/ini",
    "Fallout76",
    ["/downloads/hud.zip"],
    "xscal",
    "/downloads/provider.zip",
    "/downloads/loader.zip",
  );
});

it("uses the official Mod Manager Download link when direct download is unavailable", async () => {
  vi.mocked(commands.fcmPrerequisiteDownloadLinks).mockRejectedValue(
    new Error("Premium required"),
  );
  vi.mocked(commands.nxmIsRegistered).mockResolvedValue(false);
  vi.mocked(commands.nxmRegister).mockResolvedValue(null);
  vi.mocked(NexusMods.extractDetailsFromNxmUrl).mockResolvedValue({
    gameDomain: "fallout76",
    gameScopedId: 4065,
    fileId: 200,
    key: "key",
    expires: 1,
    userId: 1,
  });
  vi.mocked(nxmLinksQueueService.subscribe).mockImplementation((listener) => {
    queueMicrotask(() => listener("nxm://fallout76/mods/4065/files/200"));
    return () => undefined;
  });
  vi.mocked(NexusMods.api.requestDownloadLinks).mockResolvedValue([
    { uri: "https://example.com/provider.zip", name: "CDN", shortName: "CDN" },
  ]);
  vi.mocked(commands.downloadWithProgress)
    .mockReset()
    .mockResolvedValue("/downloads/provider.zip");
  const onPreview = vi.fn();
  render(
    <FcmPrerequisiteModal
      request={{
        paths: ["/downloads/hud.zip"],
        probe: { provider: null, hudModLoader: true },
      }}
      onAbort={vi.fn()}
      onPreview={onPreview}
    />,
  );
  fireEvent.click(
    screen.getByRole("button", { name: "fcmImport.downloadPrerequisites" }),
  );
  await waitFor(() => expect(onPreview).toHaveBeenCalled());
  expect(commands.nxmRegister).toHaveBeenCalled();
  expect(nxmLinksQueueService.consume).toHaveBeenCalledWith(
    "nxm://fallout76/mods/4065/files/200",
  );
  expect(commands.fcmPreviewImport).toHaveBeenCalledWith(
    "/game",
    "/ini",
    "Fallout76",
    ["/downloads/hud.zip"],
    "zfe",
    "/downloads/provider.zip",
    null,
  );
});
