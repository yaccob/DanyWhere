import React from "react";

/** The setup's card: a suggestion or the connection, set off from the
 * questions around it. */
export const SetupCard: React.FC<{
  title: React.ReactNode;
  icon?: React.ReactNode;
  children: React.ReactNode;
}> = ({ title, icon, children }) => (
  <div className="space-y-2 rounded-lg border border-line-strong bg-background px-4 py-3 text-start">
    <p className="m-0 flex items-center gap-2 text-sm font-semibold text-text">
      {icon}
      <span>{title}</span>
    </p>
    {children}
  </div>
);

/** A sentence of a card. */
export const CardLine: React.FC<{
  children: React.ReactNode;
  tone?: "plain" | "quiet" | "warning" | "error" | "done";
}> = ({ children, tone = "plain" }) => {
  const color = {
    plain: "text-ink-2",
    quiet: "text-ink-3",
    warning: "text-warning",
    error: "text-error",
    done: "text-moss",
  }[tone];
  return <p className={`m-0 text-sm text-pretty ${color}`}>{children}</p>;
};
