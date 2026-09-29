export function Profile() {
  return (
    <button
      class="size-7 cursor-pointer rounded-full border border-[color-mix(in_srgb,var(--primary)_45%,var(--border))] bg-[linear-gradient(145deg,#9770d1,#557fc8)] p-0.5 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary max-[900px]:hidden"
      aria-label="Open profile"
    >
      <span class="grid size-full place-items-center rounded-full bg-[rgba(20,16,25,.35)] text-[9px] font-bold text-white">
        MV
      </span>
    </button>
  );
}
