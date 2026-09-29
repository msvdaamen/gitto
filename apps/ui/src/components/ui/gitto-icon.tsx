import { createUniqueId, splitProps, type JSX } from "solid-js";

type GittoIconProps = JSX.SvgSVGAttributes<SVGSVGElement> & {
  title?: string;
};

export function GittoIcon(props: GittoIconProps) {
  const gradientId = createUniqueId();
  const [local, others] = splitProps(props, ["class", "height", "title", "width"]);

  return (
    <svg
      {...others}
      class={`block overflow-visible ${local.class ?? ""}`}
      width={local.width ?? "100%"}
      height={local.height ?? "100%"}
      viewBox="0 0 27 23"
      preserveAspectRatio="xMidYMid meet"
      role={local.title ? "img" : undefined}
      aria-hidden={local.title ? undefined : true}
    >
      {local.title && <title>{local.title}</title>}
      <defs>
        <linearGradient
          id={gradientId}
          x1="4"
          y1="2"
          x2="23"
          y2="21"
          gradientUnits="userSpaceOnUse"
        >
          <stop stop-color="#bc8aef" />
          <stop offset="1" stop-color="#7861e6" />
        </linearGradient>
      </defs>
      <g transform="rotate(-3 13.5 11.5)">
        <path
          d="M12.69 0C20.59 0 27 4.43 27 9.89C27 17.13 20.72 23 12.96 23C5.8 23 0 18.67 0 13.34C0 5.97 5.68 0 12.69 0Z"
          fill={`url(#${gradientId})`}
          style={{ filter: "drop-shadow(0 0 18px rgba(168,121,223,.28))" }}
        />
        <circle cx="8.5" cy="10.5" r="1.5" fill="#261c31" />
        <circle cx="17.5" cy="10.5" r="1.5" fill="#261c31" />
        <path
          d="M10.5 15.55C12 16.45 15 16.45 16.5 15.55"
          fill="none"
          stroke="#261c31"
          stroke-width="1.5"
          stroke-linecap="round"
        />
      </g>
    </svg>
  );
}
