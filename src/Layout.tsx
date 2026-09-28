import Navigation from "@/components/navigation/Navigation";
import Toasts from "@/components/Toasts";
import useSyncTranslation from "@/hooks/useSyncTranslation";
import { Outlet } from "react-router-dom";

function Globals() {
  useSyncTranslation();
  return <Toasts />;
}

export default function Layout() {
  return (
    <div
      style={{
        display: "flex",
        width: "100%",
        height: "100%",
        overflow: "hidden",
      }}
    >
      <Navigation />
      <div style={{ flex: 1, overflowY: "auto" }}>
        <Outlet />
      </div>
      <Globals />
    </div>
  );
}
