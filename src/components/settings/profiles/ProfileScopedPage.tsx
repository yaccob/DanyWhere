// DanyWhere: a page of a profile (Profile settings › Work › Advanced; qa
// 2026-11-19_1711 sketch 2, P4, P7.3, P8): the same page as under the
// defaults, in the profile's scope, so every setting shows its chain and
// state; "Page to Defaults" on top, the legend below, the quiet bar of P7.3.
import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { Link2, Link2Off, RotateCcw } from "lucide-react";
import { commands } from "@/bindings";
import { useProfiles } from "@/hooks/useProfiles";
import { useSettingsStore } from "@/stores/settingsStore";
import { pageLabelKey, type DefaultsPage } from "@/lib/navigation";
import { pageOfValue, resetPlan } from "@/lib/profileFields";
import { ownValues, type LayeredProfile } from "@/lib/profileScope";
import { Button } from "@/components/ui/Button";
import { SettingsScopeProvider } from "./scope/SettingsScope";
import { DecoupleBar } from "./scope/DecoupleBar";
import { ResetDialog } from "./scope/ResetDialog";
import { runCoupling } from "./scope/profileWriter";
import { useValueLabels } from "./scope/useValueLabels";

interface ProfileScopedPageProps {
  profileId: string;
  page: DefaultsPage;
  /** The page fills the window (Models): no legend under it. */
  fills?: boolean;
  children: React.ReactNode;
}

/** The legend of the chains (sketch 2). */
const Legend: React.FC = () => {
  const { t } = useTranslation();
  return (
    <div className="max-w-3xl w-full mx-auto px-4 space-y-1 text-xs text-ink-3">
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1">
        <span className="inline-flex items-center gap-1">
          <Link2 className="size-3.5" aria-hidden="true" />
          {t("settings.profileView.legend.coupled")}
        </span>
        <span className="inline-flex items-center gap-1">
          <Link2Off className="size-3.5 text-logo-primary" aria-hidden="true" />
          {t("settings.profileView.legend.own")}
        </span>
      </div>
      <p>{t("settings.profileView.legend.hint")}</p>
    </div>
  );
};

export const ProfileScopedPage: React.FC<ProfileScopedPageProps> = ({
  profileId,
  page,
  fills = false,
  children,
}) => {
  const { t } = useTranslation();
  const { profiles, displayName } = useProfiles();
  const { ownValueLabel } = useValueLabels();
  const layered = useSettingsStore(
    (s) => (s.settings?.profiles ?? []) as unknown as readonly LayeredProfile[],
  );
  const [asking, setAsking] = useState(false);
  const [busy, setBusy] = useState(false);
  const profile = profiles.find((p) => p.id === profileId);
  const stored = layered.find((p) => p.id === profileId);
  if (!profile || !stored) return null;

  const own = ownValues(stored);
  const plan = resetPlan(own, page);
  const named = own
    .filter((value) => pageOfValue(value.field, value.entry) === page)
    .map(ownValueLabel);

  const reset = async () => {
    setBusy(true);
    try {
      await runCoupling(async () => {
        if (plan.fields.length > 0) {
          const result = await commands.resetProfileValues(
            profileId,
            plan.fields,
          );
          if (result.status === "error") return result;
        }
        for (const { field, entry } of plan.entries) {
          const result = await commands.resetProfileEntry(
            profileId,
            field,
            entry,
          );
          if (result.status === "error") return result;
        }
        return { status: "ok" as const, data: null };
      });
    } finally {
      setBusy(false);
      setAsking(false);
    }
  };

  return (
    <SettingsScopeProvider scope={{ kind: "profile", id: profileId }}>
      <div className="max-w-3xl w-full mx-auto px-4 flex flex-none items-center justify-end">
        <Button
          variant="secondary"
          size="sm"
          disabled={plan.count === 0 || busy}
          onClick={() => (plan.count > 1 ? setAsking(true) : void reset())}
        >
          <span className="inline-flex items-center gap-1">
            <RotateCcw className="size-3.5" aria-hidden="true" />
            {t("settings.profileView.page.reset")}
          </span>
        </Button>
      </div>
      {children}
      {!fills && <Legend />}
      <DecoupleBar profileId={profileId} />
      <ResetDialog
        open={asking}
        title={t("settings.profileView.page.resetTitle", {
          page: t(pageLabelKey(page)),
          name: displayName(profile),
        })}
        values={named}
        busy={busy}
        onConfirm={() => void reset()}
        onClose={() => setAsking(false)}
      />
    </SettingsScopeProvider>
  );
};
