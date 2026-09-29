import { ManagedMod, ManagedMods } from "@/commands/bindings";

export { ModsEventBus, modsEventBus } from "./eventBus";
export type { Archive2Event, ProgressEvent, UIActionEvent } from "./eventBus";

export const FCM_MOD_KEY = "fcm-installed";

export function getFcmManagedOwner(
  managed: ManagedMods,
): ManagedMod | undefined {
  const owner = managed.state.find((entry) =>
    entry.files.some((file) => {
      const parts: string[] = [];
      for (const part of `${entry.rootFolder}/${file}`
        .replaceAll("\\", "/")
        .split("/")) {
        if (part === "..") parts.pop();
        else if (part && part !== ".") parts.push(part);
      }
      return /(?:^|\/)data\/(?:fcmchatwidget|fcmserverbridge)\.ba2$/i.test(
        parts.join("/"),
      );
    }),
  );
  return managed.mods.find((mod) => mod.key === owner?.key);
}

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
