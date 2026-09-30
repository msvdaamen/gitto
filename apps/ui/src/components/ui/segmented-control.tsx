import { cn } from "cn";

export function SegmentedControl(props: {
  value: string;
  options: string[];
  onChange: (value: string) => void;
}) {
  return (
    <div class="grid grid-cols-2 rounded-[7px] border border-border-soft bg-bg p-[3px] max-[900px]:flex max-[900px]:justify-center">
      {props.options.map((option) => (
        <button
          class={cn(
            "h-[25px] cursor-pointer rounded-[5px] border-0 bg-transparent text-[10.5px] text-muted max-[900px]:hidden max-[900px]:first:block max-[900px]:first:w-[27px] max-[900px]:first:text-0 max-[900px]:first:before:content-['≡'] max-[900px]:first:before:text-[15px]",
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
