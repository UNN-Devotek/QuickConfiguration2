import type { Profile } from "@/commands/bindings";
import { commandErrorToString, type AnyError } from "@/commands/errors";
import * as profilesService from "@/services/profiles";
import { useProfilesStore } from "@/stores/profiles";
import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { Alert, Button, Form, ListGroup } from "react-bootstrap";
import { useTranslation } from "react-i18next";

export default function ProfilesView() {
  const { t } = useTranslation();
  const profiles = useProfilesStore((store) => store.profiles);
  const selected = useProfilesStore((store) => store.getSelectedProfile());
  const setSelectedIndex = useProfilesStore((store) => store.setSelectedIndex);
  const addProfile = useProfilesStore((store) => store.addProfile);
  const updateProfile = useProfilesStore((store) => store.updateProfile);
  const deleteProfile = useProfilesStore((store) => store.deleteProfile);
  const [draft, setDraft] = useState<Profile | null>(selected || null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    setDraft(selected || null);
    setError("");
  }, [selected?.key]);

  function change(
    field: "title" | "installationPath" | "iniPath" | "iniPrefix",
    value: string,
  ) {
    setDraft((profile) => profile && { ...profile, [field]: value });
  }

  async function browse(field: "installationPath" | "iniPath") {
    try {
      const path = await open({ directory: true, multiple: false });
      if (typeof path === "string") {
        change(field, path);
      }
    } catch (reason) {
      setError(commandErrorToString(reason as AnyError));
    }
  }

  async function add() {
    setBusy(true);
    setError("");
    try {
      let profile: Profile;
      try {
        profile = await profilesService.createProfileWithAutoDetectedDefaults(
          t("installer.newProfile"),
        );
      } catch {
        profile = profilesService.createProfileWithDefaults(
          t("installer.newProfile"),
        );
      }
      addProfile(profile);
      setSelectedIndex(useProfilesStore.getState().profiles.length - 1);
      setDraft(profile);
    } catch (reason) {
      setError(commandErrorToString(reason as AnyError));
    } finally {
      setBusy(false);
    }
  }

  function save() {
    if (!draft) return;
    if (
      !draft.title.trim() ||
      !draft.installationPath.trim() ||
      !draft.iniPath.trim() ||
      !draft.iniPrefix.trim()
    ) {
      setError(t("installer.profilePathsRequired"));
      return;
    }
    updateProfile(draft);
    setError("");
  }

  function remove() {
    const index = profiles.findIndex(
      (profile) => profile.key === selected?.key,
    );
    if (index >= 0) deleteProfile(index);
  }

  return (
    <main className="container-fluid p-4" style={{ maxWidth: 840 }}>
      <h1>{t("installer.profilesTab")}</h1>
      <p>{t("installer.profilesIntro")}</p>
      {error && (
        <Alert variant="danger" role="alert">
          {error}
        </Alert>
      )}
      <div className="d-flex gap-2 mb-3">
        <Button onClick={() => void add()} disabled={busy}>
          {t("installer.addProfile")}
        </Button>
        <Button variant="outline-danger" onClick={remove} disabled={!selected}>
          {t("installer.deleteProfile")}
        </Button>
      </div>
      <ListGroup className="mb-4">
        {profiles.map((profile, index) => (
          <ListGroup.Item
            action
            active={profile.key === selected?.key}
            key={profile.key}
            onClick={() => setSelectedIndex(index)}
          >
            {profile.title}
          </ListGroup.Item>
        ))}
      </ListGroup>
      {draft && (
        <Form>
          <Form.Group className="mb-3">
            <Form.Label htmlFor="profile-name">
              {t("installer.profileName")}
            </Form.Label>
            <Form.Control
              id="profile-name"
              value={draft.title}
              onChange={(event) => change("title", event.target.value)}
            />
          </Form.Group>
          {(["installationPath", "iniPath"] as const).map((field) => (
            <Form.Group className="mb-3" key={field}>
              <Form.Label htmlFor={field}>
                {t(
                  field === "installationPath"
                    ? "installer.gamePath"
                    : "installer.iniPath",
                )}
              </Form.Label>
              <div className="d-flex gap-2">
                <Form.Control
                  id={field}
                  value={draft[field]}
                  onChange={(event) => change(field, event.target.value)}
                />
                <Button
                  variant="outline-primary"
                  onClick={() => void browse(field)}
                >
                  {t("installer.browse")}
                </Button>
              </div>
            </Form.Group>
          ))}
          <Form.Group className="mb-3">
            <Form.Label htmlFor="ini-prefix">
              {t("installer.iniPrefix")}
            </Form.Label>
            <Form.Control
              id="ini-prefix"
              value={draft.iniPrefix}
              onChange={(event) => change("iniPrefix", event.target.value)}
            />
          </Form.Group>
          <Button onClick={save}>{t("installer.saveProfile")}</Button>
        </Form>
      )}
    </main>
  );
}

export const Component: React.FC = ProfilesView;
