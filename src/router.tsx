import PropagateError from "@/views/errors/PropagateError";
import * as React from "react";
import { createBrowserRouter } from "react-router-dom";

export default function getRouter(container: React.ReactNode) {
  return createBrowserRouter([
    {
      element: container,
      errorElement: <PropagateError />,
      children: [
        { path: "/", lazy: () => import("@/views/mods/ModsView") },
        {
          path: "/profiles",
          lazy: () => import("@/views/profiles/ProfilesView"),
        },
        {
          path: "/settings",
          lazy: () => import("@/views/settings/SettingsView"),
        },
        {
          path: "/nexusmods",
          lazy: () => import("@/views/nexusmods/NexusModsView"),
        },
        { path: "*", lazy: () => import("@/views/errors/NotFound") },
      ],
    },
  ]);
}
