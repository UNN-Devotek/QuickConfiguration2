import {
  commands,
  type FcmAction,
  type FcmPreview,
  type FcmReleases,
} from "@/commands/bindings";
import { commandErrorToString, type AnyError } from "@/commands/errors";
import { resourceListStoreSync } from "@/stores/resourceList";
import { useProfilesStore } from "@/stores/profiles";
import { useToastsStore } from "@/stores/toasts";
import { useEffect, useState } from "react";
import { Alert, Button, Card, Form, ListGroup, Spinner } from "react-bootstrap";
import { useTranslation } from "react-i18next";

export default function FcmInstallerTab() {
  const { t } = useTranslation();
  const profile = useProfilesStore((store) => store.getSelectedProfile());
  const [releases, setReleases] = useState<FcmReleases | null>(null);
  const [action, setAction] = useState<FcmAction>("installHud");
  const [preview, setPreview] = useState<FcmPreview | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const [success, setSuccess] = useState("");

  useEffect(() => {
    commands
      .fcmReleases()
      .then(setReleases)
      .catch((reason: AnyError) => {
        setError(commandErrorToString(reason));
      });
  }, []);

  async function createPreview() {
    if (!profile) return;
    setPending(true);
    setError("");
    setSuccess("");
    setPreview(null);
    try {
      await resourceListStoreSync.flushSave();
      setPreview(
        await commands.fcmPreview(
          profile.installationPath,
          profile.iniPath,
          profile.iniPrefix,
          action,
        ),
      );
    } catch (reason) {
      setError(commandErrorToString(reason as AnyError));
    } finally {
      setPending(false);
    }
  }

  async function applyPreview() {
    if (!preview || !profile) return;
    setPending(true);
    setError("");
    resourceListStoreSync.cancelSave();
    try {
      const backup = await commands.fcmApply(preview.token);
      useToastsStore
        .getState()
        .addToast(
          t("fcmInstaller.title"),
          t("fcmInstaller.completed", { backup }),
          "success",
        );
      await commands.iniLoad(profile.iniPath, profile.iniPrefix);
      await resourceListStoreSync.load();
      setSuccess(t("fcmInstaller.completed", { backup }));
      setPreview(null);
    } catch (reason) {
      setError(commandErrorToString(reason as AnyError));
    } finally {
      setPending(false);
    }
  }

  const selectedRelease =
    action === "installHud" ? releases?.hud : releases?.bridge;
  const releaseError =
    action === "installHud" ? releases?.hudError : releases?.bridgeError;

  return (
    <div className="p-3 d-flex flex-column gap-3">
      <Card>
        <Card.Body>
          <Card.Title>{t("fcmInstaller.title")}</Card.Title>
          <p>{t("fcmInstaller.intro")}</p>
          <p className="small text-muted">{t("fcmInstaller.prerequisites")}</p>
          <Form.Group>
            <Form.Label>{t("fcmInstaller.action")}</Form.Label>
            <Form.Select
              value={action}
              onChange={(event) => {
                setAction(event.target.value as FcmAction);
                setPreview(null);
                setError("");
              }}
            >
              <option value="installHud">{t("fcmInstaller.hud")}</option>
              <option value="installBridge">{t("fcmInstaller.bridge")}</option>
              <option value="remove">{t("fcmInstaller.remove")}</option>
            </Form.Select>
          </Form.Group>
          {action !== "remove" && (
            <p className="mt-3 mb-0">
              {selectedRelease
                ? t("fcmInstaller.release", {
                    version: selectedRelease.version,
                    source: selectedRelease.source,
                  })
                : releaseError || t("common.loading")}
            </p>
          )}
          <Button
            className="mt-3"
            disabled={
              pending || !profile || (action !== "remove" && !selectedRelease)
            }
            onClick={createPreview}
          >
            {pending ? <Spinner size="sm" /> : t("fcmInstaller.preview")}
          </Button>
        </Card.Body>
      </Card>
      {error && <Alert variant="danger">{error}</Alert>}
      {success && <Alert variant="success">{success}</Alert>}
      {preview && (
        <Card>
          <Card.Body>
            <Card.Title>{t("fcmInstaller.review")}</Card.Title>
            <p>
              {t("fcmInstaller.detected", {
                provider: preview.provider,
                installed: preview.installed || t("fcmInstaller.none"),
              })}
            </p>
            <ListGroup className="mb-3">
              {preview.changes.map((change) => (
                <ListGroup.Item key={change.path}>
                  <strong>{change.description}</strong>
                  <br />
                  <small>{change.path}</small>
                </ListGroup.Item>
              ))}
            </ListGroup>
            <Button
              disabled={pending || preview.changes.length === 0}
              onClick={applyPreview}
            >
              {t("fcmInstaller.apply")}
            </Button>
          </Card.Body>
        </Card>
      )}
    </div>
  );
}
