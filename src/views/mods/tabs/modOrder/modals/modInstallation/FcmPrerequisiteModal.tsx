import {
  commands,
  type FcmPrerequisites,
  type FcmPreview,
  type NexusModsModFile,
} from "@/commands/bindings";
import { commandErrorToString, type AnyError } from "@/commands/errors";
import NexusMods from "@/commands/nexusmods";
import { nxmLinksQueueService } from "@/services/nxm";
import {
  nexusmodsStoreAccountSync,
  useNexusModsStore,
} from "@/stores/nexusmods";
import { useProfilesStore } from "@/stores/profiles";
import { useSettingsStore } from "@/stores/settings";
import { open } from "@tauri-apps/plugin-shell";
import { useRef, useState } from "react";
import { Alert, Button, Form, Modal, Spinner } from "react-bootstrap";
import { useTranslation } from "react-i18next";

interface Props {
  request: { paths: string[]; probe: FcmPrerequisites } | null;
  onAbort: () => void;
  onPreview: (preview: FcmPreview) => void;
}

const PROVIDER_MOD_IDS = { zfe: 4065, xscal: 4183 } as const;
const LOADER_MOD_ID = 3144;

export function latestMainZip(files: NexusModsModFile[]): NexusModsModFile {
  const candidates = files.filter((file) =>
    file.fileName.toLowerCase().endsWith(".zip"),
  );
  candidates.sort(
    (a, b) =>
      Date.parse(b.uploadedTime) - Date.parse(a.uploadedTime) ||
      b.fileId - a.fileId,
  );
  if (!candidates.length)
    throw new Error(
      "No current ZIP is listed in the official Nexus Mods main files",
    );
  return candidates[0];
}

function waitForNxm(
  modId: number,
  fileId: number,
  signal: AbortSignal,
): Promise<string> {
  return new Promise((resolve, reject) => {
    const subscription: { unsubscribe?: () => void } = {};
    const cleanup = () => {
      subscription.unsubscribe?.();
      signal.removeEventListener("abort", abort);
    };
    const abort = () => {
      cleanup();
      reject(new Error("Download canceled"));
    };
    if (signal.aborted) return abort();
    signal.addEventListener("abort", abort, { once: true });
    subscription.unsubscribe = nxmLinksQueueService.subscribe((link) => {
      NexusMods.extractDetailsFromNxmUrl(link)
        .then((details) => {
          if (signal.aborted) return;
          if (
            details.gameDomain !== "fallout76" ||
            details.gameScopedId !== modId ||
            details.fileId !== fileId
          )
            return;
          nxmLinksQueueService.consume(link);
          cleanup();
          resolve(link);
        })
        .catch(() => undefined);
    }, true);
  });
}

export default function FcmPrerequisiteModal({
  request,
  onAbort,
  onPreview,
}: Props) {
  const { t } = useTranslation();
  const [choice, setChoice] = useState<"zfe" | "xscal">("zfe");
  const [pending, setPending] = useState(false);
  const [status, setStatus] = useState("");
  const [error, setError] = useState("");
  const controller = useRef<AbortController | null>(null);

  const cancel = () => {
    controller.current?.abort();
    controller.current = null;
    setPending(false);
    setStatus("");
    setError("");
    onAbort();
  };

  const download = async (
    apiKey: string,
    modId: number,
    signal: AbortSignal,
  ): Promise<string> => {
    const listing = await NexusMods.api.listModFiles(
      apiKey,
      "fallout76",
      modId,
      "main",
    );
    const file = latestMainZip(listing.files);
    setStatus(
      t("fcmImport.fetchingPrerequisite", {
        name: file.name,
        version: file.version,
      }),
    );
    let links;
    try {
      links = await commands.fcmPrerequisiteDownloadLinks(
        apiKey,
        modId,
        file.fileId,
      );
    } catch {
      if (!(await commands.nxmIsRegistered())) {
        setStatus(t("fcmImport.nexusRegister"));
        await commands.nxmRegister();
      }
      setStatus(
        t("fcmImport.nexusDownload", { name: file.name, id: file.fileId }),
      );
      const nxm = waitForNxm(modId, file.fileId, signal);
      await open(
        `https://www.nexusmods.com/fallout76/mods/${modId}?tab=files&file_id=${file.fileId}`,
      );
      const link = await nxm;
      links = await NexusMods.api.requestDownloadLinks(apiKey, link);
    }
    if (signal.aborted) throw new Error("Download canceled");
    if (!links.length) throw new Error("Nexus Mods returned no download links");
    const downloadPath = useSettingsStore.getState().modManager.downloadPath;
    if (!downloadPath)
      throw new Error(
        "Set a mod download folder in Quick Configuration settings",
      );
    setStatus(t("fcmImport.downloadingPrerequisite", { name: file.name }));
    const downloaded = await commands.downloadWithProgress(links[0].uri, downloadPath);
    if (signal.aborted) throw new Error("Download canceled");
    return downloaded;
  };

  const proceed = async () => {
    if (!request) return;
    const active = new AbortController();
    controller.current = active;
    setPending(true);
    setError("");
    try {
      if (!useNexusModsStore.getState().apiKey)
        await nexusmodsStoreAccountSync.load();
      const apiKey = useNexusModsStore.getState().apiKey;
      if (!apiKey) throw new Error(t("fcmImport.nexusLogin"));
      const profile = useProfilesStore.getState().getSelectedProfile();
      if (!profile) throw new Error(t("errors.profileNotSet"));
      const provider = (request.probe.provider || choice) as "zfe" | "xscal";
      const providerZip = request.probe.provider
        ? null
        : await download(apiKey, PROVIDER_MOD_IDS[provider], active.signal);
      const loaderZip = request.probe.hudModLoader
        ? null
        : await download(apiKey, LOADER_MOD_ID, active.signal);
      if (active.signal.aborted) return;
      setStatus(t("fcmImport.inspecting"));
      const preview = await commands.fcmPreviewImport(
        profile.installationPath,
        profile.iniPath,
        profile.iniPrefix,
        request.paths,
        provider,
        providerZip,
        loaderZip,
      );
      if (!active.signal.aborted) onPreview(preview);
    } catch (reason) {
      const wasCanceled = active.signal.aborted;
      active.abort();
      if (!wasCanceled) setError(commandErrorToString(reason as AnyError));
    } finally {
      if (controller.current === active) controller.current = null;
      setPending(false);
      setStatus("");
    }
  };

  return (
    <Modal show={request !== null} onHide={cancel} size="lg">
      <Modal.Header closeButton>
        <Modal.Title>{t("fcmImport.prerequisiteTitle")}</Modal.Title>
      </Modal.Header>
      <Modal.Body>
        <p>{t("fcmImport.prerequisiteIntro")}</p>
        {request && !request.probe.provider && (
          <Form.Group>
            <Form.Label>{t("fcmImport.chooseProvider")}</Form.Label>
            <Form.Check
              id="fcm-provider-zfe"
              type="radio"
              name="fcm-provider"
              label="ZFE"
              checked={choice === "zfe"}
              disabled={pending}
              onChange={() => setChoice("zfe")}
            />
            <Form.Check
              id="fcm-provider-xscal"
              type="radio"
              name="fcm-provider"
              label="xScal"
              checked={choice === "xscal"}
              disabled={pending}
              onChange={() => setChoice("xscal")}
            />
          </Form.Group>
        )}
        {request?.probe.provider && (
          <p>
            {t("fcmImport.detectedProvider", {
              provider: request.probe.provider,
            })}
          </p>
        )}
        {request && !request.probe.hudModLoader && (
          <p>{t("fcmImport.offerLoader")}</p>
        )}
        {status && (
          <Alert variant="info" className="mt-3">
            {pending && <Spinner size="sm" className="me-2" />}
            {status}
          </Alert>
        )}
        {error && (
          <Alert variant="danger" className="mt-3">
            {error}
          </Alert>
        )}
      </Modal.Body>
      <Modal.Footer>
        <Button variant="secondary" onClick={cancel}>
          {t("common.cancel")}
        </Button>
        <Button onClick={proceed} disabled={pending}>
          {t("fcmImport.downloadPrerequisites")}
        </Button>
      </Modal.Footer>
    </Modal>
  );
}
