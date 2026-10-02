import { ManagedMod } from "@/commands/bindings";

export { ModsEventBus, modsEventBus } from "./eventBus";
export type { Archive2Event, ProgressEvent, UIActionEvent } from "./eventBus";

export const FCM_MOD_KEY = "fcm-installed";

export function createBaseManagedMod(basename?: string): ManagedMod {
  return {
    key: crypto.randomUUID(),
    title: basename || "",
    folderName: (basename || "").replace(/[^a-zA-Z0-9\-._ ]/g, "-").trim(),
    version: "",
    url: "",
    notes: "",
    enabled: true,
    options: {
      rootFolder: ".",
    },
  };
}
