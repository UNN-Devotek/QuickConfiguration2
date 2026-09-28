import ProfilesView from "@/views/profiles/ProfilesView";
import { fireEvent, render, screen } from "@testing-library/react";
import { vi } from "vitest";

const profile = {
  key: "existing",
  title: "Steam",
  installationPath: "/game",
  iniPath: "/ini",
  iniPrefix: "Fallout76",
  modsPath: "/legacy-mods",
  executableName: "Fallout76.exe",
  execParameters: "",
  launcherURL: "steam://run/1151340",
  gameEdition: "Steam" as const,
  launchOption: "OpenURL" as const,
};
const store = vi.hoisted(() => ({
  updateProfile: vi.fn(),
  addProfile: vi.fn(),
  deleteProfile: vi.fn(),
  setSelectedIndex: vi.fn(),
}));

vi.mock("@/stores/profiles", () => ({
  useProfilesStore: Object.assign(
    (selector: (state: object) => unknown) =>
      selector({
        profiles: [profile],
        getSelectedProfile: () => profile,
        ...store,
      }),
    { getState: () => ({ profiles: [profile] }) },
  ),
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

afterEach(() => vi.clearAllMocks());

it("saves only FCM path edits while preserving legacy profile fields", () => {
  render(<ProfilesView />);
  fireEvent.change(screen.getByLabelText("installer.iniPath"), {
    target: { value: "/new-ini" },
  });
  fireEvent.click(
    screen.getByRole("button", { name: "installer.saveProfile" }),
  );
  expect(store.updateProfile).toHaveBeenCalledWith({
    ...profile,
    iniPath: "/new-ini",
  });
});

it("requires complete game and INI paths", () => {
  render(<ProfilesView />);
  fireEvent.change(screen.getByLabelText("installer.gamePath"), {
    target: { value: "" },
  });
  fireEvent.click(
    screen.getByRole("button", { name: "installer.saveProfile" }),
  );
  expect(store.updateProfile).not.toHaveBeenCalled();
  expect(screen.getByRole("alert")).toHaveTextContent(
    "installer.profilePathsRequired",
  );
});
