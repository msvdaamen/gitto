export function Profile() {
  return (
    <button
      class="size-7 cursor-pointer rounded-full border border-[color-mix(in_srgb,var(--primary)_45%,var(--border))] bg-[linear-gradient(145deg,#9770d1,#557fc8)] p-0.5 focus-ring max-md:hidden"
      aria-label="Open profile"
    >
      <span class="grid size-full place-items-center rounded-full bg-[rgba(20,16,25,.35)] text-[11.5px] font-bold text-white">
        MV
      </span>
    </button>
  );
}
