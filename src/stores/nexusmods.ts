import {
  NexusModsAccountInfo,
  NexusModsProfile,
  NexusModsRateLimit,
} from "@/commands/bindings";
import { commandErrorToString } from "@/commands/errors";
import NexusMods from "@/commands/nexusmods";
import { syncStore } from "@/utils/zustand";
import fastDeepEqual from "fast-deep-equal/es6";
import { create } from "zustand";

export interface NexusModsStore {
  apiKey?: string;
  profile?: NexusModsProfile;
  rateLimit?: NexusModsRateLimit;
  getAccountInfo: () => NexusModsAccountInfo | undefined;
  setAccountInfo: (account: NexusModsAccountInfo | null | undefined) => void;
  setApiKey: (apiKey: string | undefined) => void;
}

export const useNexusModsStore = create<NexusModsStore>()((set, get) => ({
  getAccountInfo: () => {
    const profile = get().profile;
    const rateLimit = get().rateLimit;
    return profile && rateLimit ? { profile, rateLimit } : undefined;
  },
  setAccountInfo: (account) =>
    set({
      apiKey: account?.profile.apiKey,
      profile: account?.profile,
      rateLimit: account?.rateLimit,
    }),
  setApiKey: (apiKey) => set({ apiKey }),
}));

export const nexusmodsStoreAccountSync = syncStore(
  useNexusModsStore,
  (_set, get) => ({
    load: async () => {
      const account = await NexusMods.getAccountInfo();
      get().setAccountInfo(account);
    },
    save: () => {
      const account = get().getAccountInfo();
      if (!account) throw new Error("Account info is null or undefined");
      return async () => await NexusMods.setAccountInfo(account);
    },
    watch: (store) => store.getAccountInfo(),
    equals: fastDeepEqual,
  }),
);

nexusmodsStoreAccountSync.onLoadRejected((error) => {
  console.error(
    "Failed to load Nexus Mods account:",
    commandErrorToString(error),
  );
});
nexusmodsStoreAccountSync.onSaveRejected((error) => {
  console.error(
    "Failed to save Nexus Mods account:",
    commandErrorToString(error),
  );
});
