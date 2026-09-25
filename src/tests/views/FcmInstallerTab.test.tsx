import { commands } from "@/commands/bindings";
import { resourceListStoreSync } from "@/stores/resourceList";
import { useProfilesStore } from "@/stores/profiles";
import { useToastsStore } from "@/stores/toasts";
import FcmInstallerTab from "@/views/mods/tabs/fcm/FcmInstallerTab";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { vi } from "vitest";

vi.mock("@/commands/bindings", () => ({
  commands: {
    fcmReleases: vi.fn(),
    fcmPreview: vi.fn(),
    fcmApply: vi.fn(),
    iniLoad: vi.fn(),
  },
}));

vi.mock("@/stores/resourceList", () => ({
  resourceListStoreSync: {
    flushSave: vi.fn(),
    cancelSave: vi.fn(),
    load: vi.fn(),
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
  vi.mocked(commands.fcmReleases).mockResolvedValue({
    hud: null,
    bridge: null,
    hudError: "offline",
    bridgeError: "offline",
  });
  vi.mocked(commands.fcmPreview).mockResolvedValue({
    token: "preview-token",
    action: "remove",
    provider: "not required",
    installed: "HUD",
    release: null,
    changes: [
      { path: "/game/Data/hudmodloader.ini", description: "Remove FCM entry" },
    ],
  });
  vi.mocked(commands.fcmApply).mockResolvedValue("/backup");
  vi.mocked(commands.iniLoad).mockResolvedValue(null);
  vi.mocked(resourceListStoreSync.flushSave).mockResolvedValue();
  vi.mocked(resourceListStoreSync.load).mockResolvedValue();
});

afterEach(() => vi.clearAllMocks());

it("allows removal while release sources are unavailable and refreshes state after apply", async () => {
  render(<FcmInstallerTab />);
  fireEvent.change(screen.getByRole("combobox"), {
    target: { value: "remove" },
  });
  fireEvent.click(screen.getByRole("button", { name: "fcmInstaller.preview" }));
  await waitFor(() =>
    expect(commands.fcmPreview).toHaveBeenCalledWith(
      "/game",
      "/ini",
      "Fallout76",
      "remove",
    ),
  );
  fireEvent.click(screen.getByRole("button", { name: "fcmInstaller.apply" }));
  await waitFor(() =>
    expect(commands.fcmApply).toHaveBeenCalledWith("preview-token"),
  );
  expect(commands.iniLoad).toHaveBeenCalledWith("/ini", "Fallout76");
  expect(resourceListStoreSync.load).toHaveBeenCalled();
  expect(useToastsStore.getState().toasts.at(-1)?.variant).toBe("success");
});
