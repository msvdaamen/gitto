import { cn } from "cn";

export function SegmentedControl(props: {
  value: string;
  options: string[];
  onChange: (value: string) => void;
}) {
  return (
    <div class="grid grid-cols-2 rounded-[7px] border border-border-soft bg-bg p-[3px]">
      {props.options.map((option) => (
        <button
          class={cn(
            "h-[25px] cursor-pointer rounded-[5px] border-0 bg-transparent text-[13px] text-muted",
            props.value === option && "bg-panel-hover text-text shadow-[0_1px_3px_rgba(0,0,0,.15)]",
          )}
          onClick={() => props.onChange(option)}
        >
          {option}
        </button>
      ))}
    </div>
  );
}
