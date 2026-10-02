import { TextField } from "@kobalte/core/text-field";
import { Show } from "solid-js";

const controlClass =
  "w-full resize-y rounded-md border border-border bg-bg px-[9px] py-2 text-[12px] text-text outline-0 placeholder:text-faint focus:border-primary focus:shadow-[0_0_0_3px_var(--primary-soft)]";

/** A labelled text field for forms; a textarea when given `rows`. */
export function FormField(props: {
  label: string;
  hint?: string;
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  rows?: number;
  /** Keeps focus, unlike disabling it, e.g. to type the next message once a commit is done. */
  readOnly?: boolean;
}) {
  return (
    <TextField
      class="mb-2.5 flex flex-col gap-[5px] text-[11.5px] text-muted"
      value={props.value}
      onChange={props.onChange}
      readOnly={props.readOnly}
    >
      <TextField.Label class="flex justify-between">
        {props.label}
        <Show when={props.hint}>
          <small class="text-faint">{props.hint}</small>
        </Show>
      </TextField.Label>
      <Show
        when={props.rows}
        fallback={<TextField.Input class={controlClass} placeholder={props.placeholder} />}
      >
        <TextField.TextArea
          class={controlClass}
          placeholder={props.placeholder}
          rows={props.rows}
        />
      </Show>
    </TextField>
  );
}
