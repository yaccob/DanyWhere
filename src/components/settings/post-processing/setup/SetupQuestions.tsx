import React from "react";
import { useTranslation } from "react-i18next";
import { Radio } from "../../../ui/Radio";
import {
  nextStepOptions,
  subscriptionOptions,
  type Machine,
  type NextStep,
  type SetupAnswers,
  type Subscription,
  type TextsMayGo,
} from "@/lib/postProcessSetup/questions";
import { memoryLabel, plansLabel } from "@/lib/postProcessSetup/labels";

const KEY = "settings.postProcessing.setup";

interface ChoiceProps {
  checked: boolean;
  disabled?: boolean;
  onSelect: () => void;
  label: string;
  hint?: string;
}

/** One answer of a question: a row the whole of which is the radio. */
const Choice: React.FC<ChoiceProps> = ({
  checked,
  disabled = false,
  onSelect,
  label,
  hint,
}) => (
  <button
    type="button"
    role="radio"
    aria-checked={checked}
    aria-disabled={disabled ? true : undefined}
    disabled={disabled}
    onClick={onSelect}
    className={`flex w-full items-start gap-3 rounded-md px-2 py-1.5 text-start transition-colors focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary ${
      disabled
        ? "cursor-not-allowed text-ink-3"
        : "cursor-pointer hover:bg-wash"
    }`}
  >
    <Radio checked={checked} className="mt-[2px]" />
    <span className="min-w-0 text-sm">
      <span className={disabled ? "" : "text-text"}>{label}</span>
      {hint && <span className="text-ink-3"> {hint}</span>}
    </span>
  </button>
);

const Question: React.FC<{ title: string; children: React.ReactNode }> = ({
  title,
  children,
}) => (
  <div className="space-y-1">
    <p className="m-0 text-sm font-semibold text-text">{title}</p>
    <div role="radiogroup" aria-label={title} className="space-y-0.5">
      {children}
    </div>
  </div>
);

interface SetupQuestionsProps {
  answers: SetupAnswers;
  machine: Machine;
  onAnswer: (next: SetupAnswers) => void;
}

const TEXTS_MAY_GO: TextsMayGo[] = ["free_services", "not_kept", "nowhere"];

/**
 * DanyWhere: the setup's questions, one after the other (sidecar qa
 * 2026-11-26_1840, section 4): where the texts may go; whether one has a
 * subscription that includes use in other apps (not after "nowhere"); how
 * it should go on (only after "not kept" without a subscription). The
 * local model comes last and is greyed out below 16 GB.
 */
export const SetupQuestions: React.FC<SetupQuestionsProps> = ({
  answers,
  machine,
  onAnswer,
}) => {
  const { t, i18n } = useTranslation();
  const textsMayGo = answers.texts_may_go ?? null;
  const subscription = answers.subscription ?? null;
  const nextStep = answers.next_step ?? null;
  const asksSubscription =
    textsMayGo === "free_services" || textsMayGo === "not_kept";
  const asksNextStep = textsMayGo === "not_kept" && subscription === "none";
  const memory = memoryLabel(machine.memoryBytes, i18n.language);

  const subscriptionLabel = (id: Subscription, plans: string[]) =>
    id === "none" ? t(`${KEY}.q2.none`) : plansLabel(plans, i18n.language);
  const nextStepLabel = (id: NextStep, disabled: boolean) =>
    disabled
      ? t(
          machine.platform === "macos"
            ? `${KEY}.q3.tooLittleMemoryMac`
            : `${KEY}.q3.tooLittleMemory`,
          { memory },
        )
      : t(`${KEY}.q3.${id}`);

  return (
    <div className="space-y-4">
      <Question title={t(`${KEY}.q1.title`)}>
        {TEXTS_MAY_GO.map((id) => (
          <Choice
            key={id}
            checked={textsMayGo === id}
            onSelect={() => onAnswer({ ...answers, texts_may_go: id })}
            label={t(`${KEY}.q1.${id}.label`)}
            hint={t(`${KEY}.q1.${id}.hint`)}
          />
        ))}
      </Question>

      {asksSubscription && (
        <Question title={t(`${KEY}.q2.title`)}>
          {subscriptionOptions().map(({ id, plans }) => (
            <Choice
              key={id}
              checked={subscription === id}
              onSelect={() => onAnswer({ ...answers, subscription: id })}
              label={subscriptionLabel(id, plans)}
            />
          ))}
        </Question>
      )}

      {asksNextStep && (
        <Question title={t(`${KEY}.q3.title`)}>
          {nextStepOptions(machine).map(({ id, disabled }) => (
            <Choice
              key={id}
              checked={nextStep === id}
              disabled={disabled}
              onSelect={() => onAnswer({ ...answers, next_step: id })}
              label={nextStepLabel(id, disabled)}
            />
          ))}
        </Question>
      )}
    </div>
  );
};
