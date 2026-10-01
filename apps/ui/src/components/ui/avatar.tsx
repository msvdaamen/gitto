import { cn } from "cn";
import { Show } from "solid-js";

import { useAvatar, type AvatarAuthor } from "@/git/avatars";

/** The initials on a coloured circle, covered by the author's profile picture if they have one. */
export function Avatar(props: {
  initials: string;
  author?: AvatarAuthor;
  color?: string;
  size?: "sm" | "md";
}) {
  const avatar = useAvatar(() => props.author);

  return (
    <span
      class={cn(
        "relative grid shrink-0 place-items-center overflow-hidden rounded-full font-[720] text-white shadow-[inset_0_0_0_1px_rgba(255,255,255,.15)]",
        props.size === "md" ? "size-[34px] text-[10px]" : "size-[23px] text-[8px]",
      )}
      style={{ "background-color": props.color ?? "#8c65cf" }}
    >
      {props.initials}
      <Show when={avatar.url()}>
        {(url) => (
          <img
            src={url()}
            alt=""
            class="absolute inset-0 size-full object-cover"
            onError={avatar.onError}
          />
        )}
      </Show>
    </span>
  );
}
