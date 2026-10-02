import type { Profile } from "@/commands/bindings";
import { useProfilesStore } from "@/stores/profiles";

export type FcmImportProfile = Pick<
  Profile,
  "key" | "installationPath" | "iniPath" | "iniPrefix"
>;

export function isFcmImportProfileActive(profile: FcmImportProfile): boolean {
  const selected = useProfilesStore.getState().getSelectedProfile();
  return (
    selected?.key === profile.key &&
    selected.installationPath === profile.installationPath &&
    selected.iniPath === profile.iniPath &&
    selected.iniPrefix === profile.iniPrefix
  );
}
