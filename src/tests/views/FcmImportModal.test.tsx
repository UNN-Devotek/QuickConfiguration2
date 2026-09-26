import { commands, type FcmPreview } from "@/commands/bindings";
import { resourceListStoreSync } from "@/stores/resourceList";
import { useProfilesStore } from "@/stores/profiles";
import { useToastsStore } from "@/stores/toasts";
import FcmImportModal from "@/views/mods/tabs/modOrder/modals/modInstallation/FcmImportModal";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { vi } from "vitest";

vi.mock("@/commands/bindings", () => ({
  commands: {
    fcmApply: vi.fn(),
    iniLoad: vi.fn(),
  },
}));

vi.mock("@/stores/resourceList", () => ({
  resourceListStoreSync: {
    cancelSave: vi.fn(),
    load: vi.fn(),
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const preview: FcmPreview = {
  token: "local-package-preview",
  action: "installBridge",
  provider: "zfe",
  installed: "HUD",
  package: {
    version: "0.2.8",
    source: "Imported package",
  },
  changes: [
    {
      path: "/game/Data/FCMServerBridge.ba2",
      description: "Install FCM package file",
    },
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
  vi.mocked(commands.fcmApply).mockResolvedValue("/backup");
  vi.mocked(commands.iniLoad).mockResolvedValue(null);
  vi.mocked(resourceListStoreSync.load).mockResolvedValue();
});

afterEach(() => vi.clearAllMocks());

it("applies an imported bridge after review and reloads INI state", async () => {
  const onApplied = vi.fn();
  render(
    <FcmImportModal
      preview={preview}
      onAbort={vi.fn()}
      onApplied={onApplied}
    />,
  );
  expect(
    screen.getByText("/game/Data/FCMServerBridge.ba2"),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "fcmImport.apply" }));
  await waitFor(() =>
    expect(commands.fcmApply).toHaveBeenCalledWith("local-package-preview"),
  );
  expect(commands.iniLoad).toHaveBeenCalledWith("/ini", "Fallout76");
  expect(resourceListStoreSync.load).toHaveBeenCalled();
  expect(onApplied).toHaveBeenCalled();
  expect(useToastsStore.getState().toasts.at(-1)?.variant).toBe("success");
});
