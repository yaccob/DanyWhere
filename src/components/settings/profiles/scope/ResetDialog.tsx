// DanyWhere: the question before more than one own value is coupled again
// (P8, sketch "zuruecksetzen"): it names the values that follow the
// defaults again.
import React from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";

interface ResetDialogProps {
  open: boolean;
  title: string;
  /** The names of the values that follow the defaults again. */
  values: string[];
  /** A last line, e.g. that name and applications stay. */
  note?: string;
  busy?: boolean;
  onConfirm: () => void;
  onClose: () => void;
}

export const ResetDialog: React.FC<ResetDialogProps> = ({
  open,
  title,
  values,
  note,
  busy = false,
  onConfirm,
  onClose,
}) => {
  const { t } = useTranslation();
  return (
    <Dialog
      open={open}
      title={title}
      closeLabel={t("common.close")}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
      contentFades={false}
      footer={
        <>
          <Button variant="secondary" size="md" onClick={onClose}>
            {t("settings.profiles.cancel")}
          </Button>
          <Button
            variant="primary"
            size="md"
            disabled={busy}
            onClick={onConfirm}
          >
            {t("settings.profileView.reset")}
          </Button>
        </>
      }
    >
      <div className="space-y-2 text-sm">
        <p>{t("settings.profileView.dialog.values")}</p>
        <ul className="list-disc ps-5">
          {values.map((value, index) => (
            <li key={`${index}-${value}`}>{value}</li>
          ))}
        </ul>
        {note && <p className="text-ink-2">{note}</p>}
      </div>
    </Dialog>
  );
};
