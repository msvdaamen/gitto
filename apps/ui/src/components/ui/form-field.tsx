import { Field, FieldLabel } from "@ui-sdk/solid/field";
import { Input } from "@ui-sdk/solid/input";
import { Textarea } from "@ui-sdk/solid/textarea";
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
    <Field class="mb-2.5 flex flex-col gap-[5px] text-[11.5px] text-muted">
      <FieldLabel class="flex justify-between">
        {props.label}
        <Show when={props.hint}>
          <small class="text-faint">{props.hint}</small>
        </Show>
      </FieldLabel>
      <Show
        when={props.rows}
        fallback={
          <Input
            class={controlClass}
            placeholder={props.placeholder}
            value={props.value}
            onValueChange={(value) => props.onChange(value)}
            readonly={props.readOnly}
          />
        }
      >
        <Textarea
          class={controlClass}
          placeholder={props.placeholder}
          rows={props.rows}
          value={props.value}
          onValueChange={(value) => props.onChange(value)}
          readonly={props.readOnly}
        />
      </Show>
    </Field>
  );
}
