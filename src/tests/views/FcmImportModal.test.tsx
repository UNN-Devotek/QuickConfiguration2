import { commands, type FcmPreview } from "@/commands/bindings";
import { useToastsStore } from "@/stores/toasts";
import FcmImportModal from "@/views/mods/FcmImportModal";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { vi } from "vitest";

vi.mock("@/commands/bindings", () => ({
  commands: {
    fcmApply: vi.fn(),
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
  vi.mocked(commands.fcmApply).mockResolvedValue("/backup");
});

afterEach(() => vi.clearAllMocks());

it("applies an imported bridge after review", async () => {
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
  expect(onApplied).toHaveBeenCalled();
  expect(useToastsStore.getState().toasts.at(-1)?.variant).toBe("success");
});
