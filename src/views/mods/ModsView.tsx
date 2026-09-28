import {
  commands,
  type FcmPrerequisites,
  type FcmPreview,
} from "@/commands/bindings";
import { commandErrorToString, type AnyError } from "@/commands/errors";
import { useProfilesStore } from "@/stores/profiles";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { Alert, Button, Spinner } from "react-bootstrap";
import { useTranslation } from "react-i18next";
import { Link } from "react-router-dom";
import FcmImportModal from "./FcmImportModal";
import FcmPrerequisiteModal from "./FcmPrerequisiteModal";

export default function ModsView() {
  const { t } = useTranslation();
  const profile = useProfilesStore((store) => store.getSelectedProfile());
  const [preview, setPreview] = useState<FcmPreview | null>(null);
  const [prerequisites, setPrerequisites] = useState<{
    paths: string[];
    probe: FcmPrerequisites;
  } | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  async function inspect(paths: string[]) {
    if (!profile) {
      setError(t("errors.profileNotSet"));
      return;
    }
    setBusy(true);
    setError("");
    try {
      if (!(await commands.fcmDetectImport(paths))) {
        throw new Error(t("installer.invalidPackage"));
      }
      const probe = await commands.fcmProbePrerequisites(
        profile.installationPath,
      );
      if (!probe.provider || !probe.hudModLoader) {
        setPrerequisites({ paths, probe });
        return;
      }
      setPreview(
        await commands.fcmPreviewImport(
          profile.installationPath,
          profile.iniPath,
          profile.iniPrefix,
          paths,
          null,
          null,
          null,
        ),
      );
    } catch (reason) {
      setError(commandErrorToString(reason as AnyError));
    } finally {
      setBusy(false);
    }
  }

  async function chooseFile() {
    try {
      const selected = await open({
        multiple: false,
        filters: [{ name: "ZIP archive", extensions: ["zip"] }],
      });
      if (typeof selected === "string") await inspect([selected]);
    } catch (reason) {
      setError(commandErrorToString(reason as AnyError));
    }
  }

  async function chooseFolder() {
    try {
      const selected = await open({ directory: true, multiple: false });
      if (typeof selected === "string") await inspect([selected]);
    } catch (reason) {
      setError(commandErrorToString(reason as AnyError));
    }
  }

  async function remove() {
    if (!profile) {
      setError(t("errors.profileNotSet"));
      return;
    }
    setBusy(true);
    setError("");
    try {
      setPreview(
        await commands.fcmPreviewRemove(
          profile.installationPath,
          profile.iniPath,
          profile.iniPrefix,
        ),
      );
    } catch (reason) {
      setError(commandErrorToString(reason as AnyError));
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    const window = getCurrentWebviewWindow();
    const subscription = window.onDragDropEvent((event) => {
      if (event.payload.type === "drop") {
        void inspect(event.payload.paths);
      }
    });
    return () => {
      void subscription.then((unlisten) => unlisten());
    };
  }, [profile?.installationPath, profile?.iniPath, profile?.iniPrefix]);

  return (
    <main className="container-fluid p-4" style={{ maxWidth: 840 }}>
      <h1>{t("installer.heading")}</h1>
      <p>{t("installer.intro")}</p>
      {profile ? (
        <p className="small text-muted">
          {t("installer.profileLabel")}: {profile.title}
          <br />
          {t("installer.gameLabel")}: {profile.installationPath}
          <br />
          {t("installer.iniLabel")}: {profile.iniPath}
        </p>
      ) : (
        <Alert variant="warning">
          {t("installer.createProfile")}{" "}
          <Link to="/profiles">{t("installer.gameProfile")}</Link>{" "}
          {t("installer.beforeInstalling")}
        </Alert>
      )}
      {error && (
        <Alert variant="danger" role="alert">
          {error}
        </Alert>
      )}
      <div className="d-flex flex-wrap gap-2">
        <Button onClick={() => void chooseFile()} disabled={busy || !profile}>
          {t("installer.chooseZip")}
        </Button>
        <Button
          variant="outline-primary"
          onClick={() => void chooseFolder()}
          disabled={busy || !profile}
        >
          {t("installer.chooseFolder")}
        </Button>
        <Button
          variant="outline-danger"
          onClick={() => void remove()}
          disabled={busy || !profile}
        >
          {t("installer.remove")}
        </Button>
      </div>
      {busy && (
        <p className="mt-3">
          <Spinner size="sm" /> {t("installer.inspecting")}
        </p>
      )}
      <p className="small text-muted mt-4">{t("installer.installNote")}</p>
      <FcmPrerequisiteModal
        request={prerequisites}
        onAbort={() => setPrerequisites(null)}
        onPreview={(result) => {
          setPrerequisites(null);
          setPreview(result);
        }}
      />
      <FcmImportModal
        preview={preview}
        onAbort={() => setPreview(null)}
        onApplied={() => setPreview(null)}
      />
    </main>
  );
}

export const Component: React.FC = ModsView;
