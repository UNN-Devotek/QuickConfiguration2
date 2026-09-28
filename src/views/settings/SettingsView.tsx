import { useSettingsStore } from "@/stores/settings";
import { open } from "@tauri-apps/plugin-dialog";
import { Button, Form } from "react-bootstrap";
import { useTranslation } from "react-i18next";

export default function SettingsView() {
  const { t } = useTranslation();
  const downloadPath = useSettingsStore(
    (store) => store.modManager.downloadPath,
  );
  const setModManagerSettings = useSettingsStore(
    (store) => store.setModManagerSettings,
  );

  async function browse() {
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected === "string") {
      setModManagerSettings((settings) => ({
        ...settings,
        downloadPath: selected,
      }));
    }
  }

  return (
    <main className="container-fluid p-4" style={{ maxWidth: 840 }}>
      <h1>{t("installer.downloadsTab")}</h1>
      <p>{t("installer.downloadsIntro")}</p>
      <Form.Label htmlFor="download-path">
        {t("installer.downloadFolder")}
      </Form.Label>
      <div className="d-flex gap-2">
        <Form.Control
          id="download-path"
          value={downloadPath}
          onChange={(event) =>
            setModManagerSettings((settings) => ({
              ...settings,
              downloadPath: event.target.value,
            }))
          }
        />
        <Button onClick={() => void browse()}>{t("installer.browse")}</Button>
      </div>
    </main>
  );
}

export const Component: React.FC = SettingsView;
