import React from "react";
import { ProfilesSettings } from "./ProfilesSettings";

/** DanyWhere: the page "Profiles" in the sidebar, right after General: the
 * profiles with their applications, choosing by application, export and
 * import. Which profile is in use is chosen at the top of the sidebar (or
 * with Use here); the list marks it as active. */
export const ProfilesPage: React.FC = () => (
  <div className="max-w-3xl w-full mx-auto space-y-6">
    <ProfilesSettings />
  </div>
);
