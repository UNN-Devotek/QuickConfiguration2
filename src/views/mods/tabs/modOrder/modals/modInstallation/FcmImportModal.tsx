import { commands, type FcmPreview } from "@/commands/bindings";
import { commandErrorToString, type AnyError } from "@/commands/errors";
import { resourceListStoreSync } from "@/stores/resourceList";
import { useProfilesStore } from "@/stores/profiles";
import { useToastsStore } from "@/stores/toasts";
import { useState } from "react";
import { Alert, Button, ListGroup, Modal, Spinner } from "react-bootstrap";
import { useTranslation } from "react-i18next";

interface Props {
  preview: FcmPreview | null;
  onAbort: () => void;
  onApplied: () => void;
}

export default function FcmImportModal({ preview, onAbort, onApplied }: Props) {
  const { t } = useTranslation();
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");

  const apply = async () => {
    if (!preview) return;
    const profile = useProfilesStore.getState().getSelectedProfile();
    if (!profile) return;
    setPending(true);
    setError("");
    resourceListStoreSync.cancelSave();
    try {
      const backup = await commands.fcmApply(preview.token);
      await commands.iniLoad(profile.iniPath, profile.iniPrefix);
      await resourceListStoreSync.load();
      useToastsStore
        .getState()
        .addToast(
          t("fcmImport.title"),
          t("fcmImport.completed", { backup }),
          "success",
        );
      onApplied();
    } catch (reason) {
      setError(commandErrorToString(reason as AnyError));
    } finally {
      setPending(false);
    }
  };

  return (
    <Modal show={preview !== null} onHide={onAbort} size="lg">
      <Modal.Header closeButton>
        <Modal.Title>{t("fcmImport.review")}</Modal.Title>
      </Modal.Header>
      <Modal.Body>
        <p>
          {t(
            preview?.action === "remove"
              ? "fcmImport.removeIntro"
              : "fcmImport.importIntro",
          )}
        </p>
        {preview?.action !== "remove" && (
          <p className="small text-muted">{t("fcmImport.prerequisites")}</p>
        )}
        {preview && (
          <>
            <p>
              {preview.action === "installHud"
                ? t("fcmImport.hud")
                : preview.action === "installBridge"
                  ? t("fcmImport.bridge")
                  : t("fcmImport.remove")}
              {preview.package &&
                ` — ${t("fcmImport.packageVersion", {
                  version: preview.package.version,
                  source: preview.package.source,
                })}`}
            </p>
            <p>
              {t("fcmImport.detected", {
                provider: preview.provider,
                installed: preview.installed || t("fcmImport.none"),
              })}
            </p>
            <ListGroup>
              {preview.changes.map((change) => (
                <ListGroup.Item key={change.path}>
                  <strong>{change.description}</strong>
                  <br />
                  <small>{change.path}</small>
                </ListGroup.Item>
              ))}
            </ListGroup>
          </>
        )}
        {error && (
          <Alert className="mt-3" variant="danger">
            {error}
          </Alert>
        )}
      </Modal.Body>
      <Modal.Footer>
        <Button variant="secondary" onClick={onAbort} disabled={pending}>
          {t("common.cancel")}
        </Button>
        <Button onClick={apply} disabled={pending || !preview?.changes.length}>
          {pending ? <Spinner size="sm" /> : t("fcmImport.apply")}
        </Button>
      </Modal.Footer>
    </Modal>
  );
}
