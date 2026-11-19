// DanyWhere: the quiet bar after a coupled value was changed in a profile
// (P7.3, sketch "profilblick_leiste"): "“Paste Method” now applies only to
// “Work”. [Change the Default Instead] [Undo]". It goes after a few seconds
// or with the next click elsewhere.
import React, { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { Info } from "lucide-react";
import { commands, type JsonValue } from "@/bindings";
import { useSettingsStore } from "@/stores/settingsStore";
import { useProfiles } from "@/hooks/useProfiles";
import { entriesOf } from "@/lib/profileScope";
import { Button } from "@/components/ui/Button";
import { DECOUPLE_BAR_MS, useDecoupleBar } from "./decoupleBarStore";
import { kindOf } from "./fieldKinds";
import { runCoupling } from "./profileWriter";
import { useValueLabels } from "./useValueLabels";

export const DecoupleBar: React.FC<{ profileId: string }> = ({ profileId }) => {
  const { t } = useTranslation();
  const bar = useDecoupleBar((s) => s.bar);
  const hide = useDecoupleBar((s) => s.hide);
  const { profiles, displayName } = useProfiles();
  const { fieldLabel, entryName } = useValueLabels();
  const ref = useRef<HTMLDivElement>(null);
  const shown = bar !== null && bar.profileId === profileId;

  // Gone after a while, or with the next click outside it.
  useEffect(() => {
    if (!shown) return;
    const timer = window.setTimeout(hide, DECOUPLE_BAR_MS);
    const onPointerDown = (event: PointerEvent) => {
      if (ref.current && !ref.current.contains(event.target as Node)) hide();
    };
    // Registered after the click that changed the value has passed.
    const listen = window.setTimeout(
      () => document.addEventListener("pointerdown", onPointerDown, true),
      0,
    );
    return () => {
      window.clearTimeout(timer);
      window.clearTimeout(listen);
      document.removeEventListener("pointerdown", onPointerDown, true);
    };
  }, [shown, bar?.shownAt, hide]);

  // A bar of another profile's page is old news here.
  useEffect(() => {
    if (bar && bar.profileId !== profileId) hide();
  }, [bar, profileId, hide]);

  if (!shown || !bar) return null;
  const profile = profiles.find((p) => p.id === profileId);
  if (!profile) return null;

  const setting =
    bar.entry === undefined
      ? fieldLabel(bar.field)
      : `${fieldLabel(bar.field)}: ${entryName(bar.field, bar.entry)}`;

  /** The default takes the profile's value, the profile follows it again. */
  const changeDefault = () => {
    hide();
    if (bar.entry === undefined) {
      void runCoupling(() =>
        commands.useProfileValueAsDefault(profileId, bar.field),
      );
      return;
    }
    const entry = bar.entry;
    const layered = useSettingsStore.getState().settings?.profiles ?? [];
    const values = layered.find((p) => p.id === profileId)?.values as
      | Record<string, unknown>
      | undefined;
    const value = entriesOf(kindOf(bar.field), values?.[bar.field]).find(
      ([id]) => id === entry,
    )?.[1];
    void runCoupling(async () => {
      const set = await commands.setProfileEntry(
        { kind: "defaults" },
        bar.field,
        entry,
        (value ?? null) as JsonValue,
      );
      if (set.status === "error") return set;
      return commands.resetProfileEntry(profileId, bar.field, entry);
    });
  };

  /** Couples the value again: back to the default it had before. */
  const undo = () => {
    hide();
    const entry = bar.entry;
    void runCoupling(() =>
      entry === undefined
        ? commands.resetProfileValues(profileId, [bar.field])
        : commands.resetProfileEntry(profileId, bar.field, entry),
    );
  };

  return (
    <div
      ref={ref}
      role="status"
      className="sticky bottom-2 z-10 mx-auto flex w-full max-w-3xl flex-wrap items-center gap-2 rounded-lg border border-line bg-background px-3 py-2 text-sm shadow-md"
    >
      <Info className="size-4 shrink-0 text-logo-primary" aria-hidden="true" />
      <span className="min-w-0 flex-1">
        {t("settings.profileView.bar.onlyFor", {
          setting,
          name: displayName(profile),
        })}
      </span>
      <Button variant="secondary" size="sm" onClick={changeDefault}>
        {t("settings.profileView.bar.changeDefault")}
      </Button>
      <Button variant="ghost" size="sm" onClick={undo}>
        {t("settings.profileView.bar.undo")}
      </Button>
    </div>
  );
};
