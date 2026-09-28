import { Profile, Profiles, commands } from "@/commands/bindings";
import { syncStore } from "@/utils/zustand";
import fastDeepEqual from "fast-deep-equal/es6";
import { create } from "zustand";

interface Actions {
  setStore: (store: Partial<Profiles>) => void;
  setSelectedIndex: (index: number) => void;
  getSelectedProfile: () => Profile | undefined;
  updateProfile: (profile: Profile) => void;
  addProfile: (profile: Profile) => void;
  deleteProfile: (index: number) => void;
}

export type ProfilesStore = Profiles & Actions;

export const useProfilesStore = create<ProfilesStore>()((set, get) => ({
  profiles: [],
  selected: "",
  setStore: (store) => set(store),
  setSelectedIndex: (index) => {
    const profile = get().profiles[index];
    set({ selected: profile?.key || "" });
  },
  getSelectedProfile: () =>
    get().profiles.find((profile) => profile.key === get().selected),
  updateProfile: (profile) => {
    set({
      profiles: get().profiles.map((current) =>
        current.key === profile.key ? profile : current,
      ),
    });
  },
  addProfile: (profile) => set({ profiles: [...get().profiles, profile] }),
  deleteProfile: (index) => {
    const profiles = get().profiles.filter((_, current) => current !== index);
    set({
      profiles,
      selected:
        get().selected === get().profiles[index]?.key
          ? profiles[0]?.key || ""
          : get().selected,
    });
  },
}));

export const profileStoreSync = syncStore(useProfilesStore, (set, get) => ({
  load: async () => set(await commands.getProfiles()),
  save: () => {
    const profiles = get();
    return async () => await commands.saveProfiles(profiles);
  },
  equals: fastDeepEqual,
}));
profileStoreSync.load().catch(console.error);
