import { useProfilesStore } from "@/stores/profiles";
import { useTranslation } from "react-i18next";
import { NavLink } from "react-router-dom";

export default function Navigation() {
  const { t } = useTranslation();
  const profile = useProfilesStore((store) => store.getSelectedProfile());
  return (
    <nav
      className="d-flex flex-column gap-2 p-3 border-end"
      style={{ width: 190, flexShrink: 0 }}
    >
      <strong>{t("installer.appName")}</strong>
      <NavLink to="/" end>
        {t("installer.installTab")}
      </NavLink>
      <NavLink to="/profiles">{t("installer.profilesTab")}</NavLink>
      <NavLink to="/nexusmods">{t("installer.nexusLogin")}</NavLink>
      <NavLink to="/settings">{t("installer.downloadsTab")}</NavLink>
      <div className="mt-auto small text-muted">
        {t("installer.activeProfile")}
        <br />
        {profile?.title || t("installer.noneSelected")}
      </div>
    </nav>
  );
}
