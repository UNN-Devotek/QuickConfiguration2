import { commands, FcmPreview, ManagedMod } from "@/commands/bindings";
import { AnyError, commandErrorToString } from "@/commands/errors";
import Mods from "@/commands/mods";
import {
  FCM_MOD_KEY,
  createBaseManagedMod,
  modsEventBus,
} from "@/services/mods";
import { updateModsStore, useModsStore } from "@/stores/mods";
import { useProfilesStore } from "@/stores/profiles";
import { resourceListStoreSync } from "@/stores/resourceList";
import { useToastsStore } from "@/stores/toasts";
import { useAtom } from "jotai";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { isModDeletionModalShownAtom } from "@/views/mods/tabs/modOrder/modals";

function useModDeletionModal() {
  const { t } = useTranslation();

  const [show, setShow] = useAtom(isModDeletionModalShownAtom);
  const getMod = useModsStore((store) => store.getMod);
  const [mod, setMod] = useState<ManagedMod | undefined>(undefined);
  const [fcmRemoval, setFcmRemoval] = useState<{
    preview: FcmPreview;
    profileKey: string;
    iniPath: string;
    iniPrefix: string;
  } | null>(null);

  const prepareFcmDeletion = async () => {
    try {
      const profile = useProfilesStore.getState().getSelectedProfile();
      if (!profile) throw new Error(t("errors.profileNotSet"));
      const modsPath = useProfilesStore.getState().getModsPath();
      if (!modsPath) throw new Error(t("mods.errors.unsetModsPath"));
      const owner = await commands.fcmManagedOwner(
        modsPath,
        useModsStore.getState().getManagedMods(),
      );
      if (owner)
        throw new Error(t("fcmImport.managedOwner", { mod: owner.title }));
      modsEventBus.emitProgressUpdated(t("fcmImport.inspecting"));
      await resourceListStoreSync.flushSave();
      const preview = await commands.fcmPreviewRemove(
        profile.installationPath,
        profile.iniPath,
        profile.iniPrefix,
      );
      modsEventBus.emitProgressFinished();
      if (useProfilesStore.getState().selected !== profile.key) {
        await commands.fcmDiscard(preview.token);
        return;
      }
      if (!preview.changes.length) {
        await commands.fcmDiscard(preview.token);
        modsEventBus.emitFcmChanged();
        return;
      }
      setFcmRemoval({
        preview,
        profileKey: profile.key,
        iniPath: profile.iniPath,
        iniPrefix: profile.iniPrefix,
      });
      setMod({
        ...createBaseManagedMod(t("fcmImport.modTitle")),
        key: FCM_MOD_KEY,
      });
      setShow(true);
    } catch (error) {
      modsEventBus.emitProgressAborted(error as AnyError);
    }
  };

  const uninstallMod = async (key: string) => {
    try {
      if (key === FCM_MOD_KEY) {
        if (!fcmRemoval) throw new Error(t("fcmImport.previewExpired"));
        if (useProfilesStore.getState().selected !== fcmRemoval.profileKey) {
          await commands.fcmDiscard(fcmRemoval.preview.token);
          setFcmRemoval(null);
          throw new Error(t("fcmImport.profileChanged"));
        }
        modsEventBus.emitProgressUpdated(
          t("mods.modOrderTab.progress.deletingMod"),
        );
        resourceListStoreSync.cancelSave();
        const backup = await commands.fcmApply(fcmRemoval.preview.token);
        setFcmRemoval(null);
        modsEventBus.emitFcmChanged();
        modsEventBus.emitProgressFinished();
        useToastsStore
          .getState()
          .addToast(
            t("mods.modOrderTab.toasts.modDeleted"),
            t("fcmImport.completed", { backup }),
          );
        try {
          await commands.iniLoad(fcmRemoval.iniPath, fcmRemoval.iniPrefix);
          await resourceListStoreSync.load();
        } catch (error) {
          useToastsStore.getState().addToast(
            t("fcmImport.title"),
            t("fcmImport.refreshFailed", {
              error: commandErrorToString(error as AnyError),
            }),
            "warning",
          );
        }
        return;
      }
      const modsPath = useProfilesStore.getState().getModsPath();
      if (!modsPath) throw new Error(t("mods.errors.unsetModsPath"));

      const managedMods = useModsStore.getState().getManagedMods();
      const modTitle = useModsStore.getState().getMod(key)?.title || key;

      modsEventBus.emitProgressUpdated(
        t("mods.modOrderTab.progress.deletingMod"),
      );

      const update = await Mods.actions.mod.uninstall(
        managedMods,
        modsPath,
        key,
      );
      updateModsStore(update);

      modsEventBus.emitProgressFinished();
      useToastsStore
        .getState()
        .addToast(t("mods.modOrderTab.toasts.modDeleted"), modTitle);
    } catch (error) {
      modsEventBus.emitProgressAborted(error as AnyError);
      useToastsStore
        .getState()
        .addToast(
          t("mods.modOrderTab.toasts.modDeletedFailed"),
          t("common.error") + ": " + commandErrorToString(error as AnyError),
          "danger",
        );
    }
  };

  return {
    deleteMod: (key: string) => {
      if (key === FCM_MOD_KEY) {
        prepareFcmDeletion().catch(console.error);
        return;
      }
      if (fcmRemoval)
        commands.fcmDiscard(fcmRemoval.preview.token).catch(console.error);
      setFcmRemoval(null);
      setShow(true);
      setMod(getMod(key));
    },
    modalProps: {
      mod,
      fcmChanges: fcmRemoval?.preview.changes,
      show,
      onConfirm: (key: string) => {
        setShow(false);
        uninstallMod(key).catch(console.error);
      },
      onAbort: () => {
        if (fcmRemoval)
          commands.fcmDiscard(fcmRemoval.preview.token).catch(console.error);
        setShow(false);
        setFcmRemoval(null);
      },
    },
  };
}

export function useModManagement() {
  const { deleteMod, modalProps: deleteModModalProps } = useModDeletionModal();
  return { deleteMod, deleteModModalProps };
}
