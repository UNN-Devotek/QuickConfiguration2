import { commands } from "@/commands/bindings";
import ModsView from "@/views/mods/ModsView";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { open } from "@tauri-apps/plugin-dialog";
import { MemoryRouter } from "react-router-dom";
import { vi } from "vitest";

const drop = vi.hoisted(() => ({
  handler: null as
    null | ((event: { payload: { type: string; paths: string[] } }) => void),
}));

vi.mock("@tauri-apps/api/webviewWindow", () => ({
  getCurrentWebviewWindow: () => ({
    onDragDropEvent: vi.fn(async (handler) => {
      drop.handler = handler;
      return () => {
        drop.handler = null;
      };
    }),
  }),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("@/commands/bindings", () => ({
  commands: {
    fcmDetectImport: vi.fn(),
    fcmProbePrerequisites: vi.fn(),
    fcmPreviewImport: vi.fn(),
    fcmPreviewRemove: vi.fn(),
  },
}));
vi.mock("@/stores/profiles", () => ({
  useProfilesStore: (selector: (store: object) => unknown) =>
    selector({
      getSelectedProfile: () => ({
        title: "Test",
        installationPath: "/game",
        iniPath: "/ini",
        iniPrefix: "Fallout76",
      }),
    }),
}));
vi.mock("@/views/mods/FcmImportModal", () => ({
  default: ({ preview }: { preview: { action: string } | null }) => (
    <div data-testid="preview">{preview?.action}</div>
  ),
}));
vi.mock("@/views/mods/FcmPrerequisiteModal", () => ({
  default: ({ request }: { request: { paths: string[] } | null }) => (
    <div data-testid="prerequisites">{request?.paths.join(",")}</div>
  ),
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

beforeEach(() => {
  vi.mocked(commands.fcmDetectImport).mockResolvedValue(true);
  vi.mocked(commands.fcmProbePrerequisites).mockResolvedValue({
    provider: "zfe",
    hudModLoader: true,
  });
  vi.mocked(commands.fcmPreviewImport).mockResolvedValue({
    token: "token",
    action: "installHud",
    provider: "zfe",
    installed: null,
    package: null,
    changes: [],
  });
});
afterEach(() => vi.clearAllMocks());

it("previews a production ZIP chosen from the main screen", async () => {
  vi.mocked(open).mockResolvedValue("/tmp/hud.zip");
  render(
    <MemoryRouter>
      <ModsView />
    </MemoryRouter>,
  );
  fireEvent.click(screen.getByRole("button", { name: "installer.chooseZip" }));
  await waitFor(() =>
    expect(commands.fcmPreviewImport).toHaveBeenCalledWith(
      "/game",
      "/ini",
      "Fallout76",
      ["/tmp/hud.zip"],
      null,
      null,
      null,
    ),
  );
  expect(screen.getByTestId("preview")).toHaveTextContent("installHud");
});

it("accepts a ZIP dropped on the main window and offers missing prerequisites", async () => {
  vi.mocked(commands.fcmProbePrerequisites).mockResolvedValue({
    provider: null,
    hudModLoader: false,
  });
  render(
    <MemoryRouter>
      <ModsView />
    </MemoryRouter>,
  );
  await waitFor(() => expect(drop.handler).not.toBeNull());
  drop.handler?.({ payload: { type: "drop", paths: ["/tmp/hud.zip"] } });
  await waitFor(() =>
    expect(screen.getByTestId("prerequisites")).toHaveTextContent(
      "/tmp/hud.zip",
    ),
  );
  expect(commands.fcmPreviewImport).not.toHaveBeenCalled();
});

it("previews removal from the same screen", async () => {
  vi.mocked(commands.fcmPreviewRemove).mockResolvedValue({
    token: "remove-token",
    action: "remove",
    provider: "zfe",
    installed: "HUD",
    package: null,
    changes: [],
  });
  render(
    <MemoryRouter>
      <ModsView />
    </MemoryRouter>,
  );
  fireEvent.click(screen.getByRole("button", { name: "installer.remove" }));
  await waitFor(() =>
    expect(commands.fcmPreviewRemove).toHaveBeenCalledWith(
      "/game",
      "/ini",
      "Fallout76",
    ),
  );
  expect(screen.getByTestId("preview")).toHaveTextContent("remove");
});

it("rejects a ZIP that is not an FCM package", async () => {
  vi.mocked(open).mockResolvedValue("/tmp/unrelated.zip");
  vi.mocked(commands.fcmDetectImport).mockResolvedValue(false);
  render(
    <MemoryRouter>
      <ModsView />
    </MemoryRouter>,
  );
  fireEvent.click(screen.getByRole("button", { name: "installer.chooseZip" }));
  await waitFor(() =>
    expect(screen.getByRole("alert")).toHaveTextContent(
      "installer.invalidPackage",
    ),
  );
  expect(commands.fcmPreviewImport).not.toHaveBeenCalled();
});
