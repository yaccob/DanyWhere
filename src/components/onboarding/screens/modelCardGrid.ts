/**
 * The grid of a model card's "Accuracy" and "Speed" lines in onboarding: the
 * label column, then the grades or the speed bars. Kept as a literal class so
 * Tailwind finds it; modelCardGrid.test.ts checks that a fixed label column
 * fits the labels of every locale.
 *
 * DanyWhere: the label column is as wide as its longest label. Handy gives it
 * 84px, which German "Geschwindigkeit" and Dutch "Nauwkeurigheid" overrun.
 * Every card shows the same two labels, so the column stays the same width
 * from card to card.
 */
export const CARD_LINES_COLUMNS = "grid-cols-[max-content_1fr]";
