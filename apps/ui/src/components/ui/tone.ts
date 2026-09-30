/** The accent colors used for badges, chips and counts. */
export type Tone = "neutral" | "purple" | "blue" | "mint" | "amber" | "coral";

/** Soft background with matching text, for small colored chips. */
export const toneClasses: Record<Tone, string> = {
  neutral: "bg-panel-raised text-muted",
  purple: "bg-primary-soft text-primary-strong",
  blue: "bg-blue-soft text-blue",
  mint: "bg-mint-soft text-mint",
  amber: "bg-amber-soft text-amber",
  coral: "bg-coral-soft text-coral",
};
