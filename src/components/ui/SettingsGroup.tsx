import React from "react";

interface SettingsGroupProps {
  title?: string;
  description?: string;
  /** DanyWhere: a line of its own under the description, e.g. what the
   * page's speech model makes of the word lists. */
  note?: React.ReactNode;
  children: React.ReactNode;
}

export const SettingsGroup: React.FC<SettingsGroupProps> = ({
  title,
  description,
  note,
  children,
}) => {
  return (
    <div className="space-y-2">
      {title && (
        <div className="px-4">
          <h2 className="text-xs font-medium text-ink-3 uppercase tracking-wide">
            {title}
          </h2>
          {description && (
            <p className="text-xs text-ink-3 mt-1">{description}</p>
          )}
          {note}
        </div>
      )}
      <div className="bg-background border border-line rounded-lg overflow-visible">
        <div className="divide-y divide-line">{children}</div>
      </div>
    </div>
  );
};
