import GitBranch from "lucide-solid/icons/git-branch";
import type { Component, JSX } from "solid-js";

import { Button } from "@/components/ui/button";
import { FormField } from "@/components/ui/form-field";

/** A popover's or dialog's title or description, so it's announced as theirs. */
type Labelling = Component<{ class: string; children: JSX.Element }>;

/**
 * The form that names a new branch. Its title and description are drawn by the popover or dialog
 * it's in.
 */
export function BranchForm(props: {
  title: Labelling;
  description: Labelling;
  /** What it's made from, and what comes along, e.g. "From main.": the description. */
  summary: string;
  /** Can't be submitted for now, e.g. while something it'd wait for is running. */
  disabled?: boolean;
  name: string;
  onNameChange: (name: string) => void;
  /** Called with the name, trimmed, once it isn't empty. */
  onSubmit: (name: string) => void;
}) {
  return (
    <form
      onSubmit={(event) => {
        event.preventDefault();
        const trimmed = props.name.trim();
        if (trimmed && !props.disabled) props.onSubmit(trimmed);
      }}
    >
      <props.title class="m-0 text-[12.5px] font-[680]">New branch</props.title>
      <props.description class="m-0 mt-1 mb-2.5 text-[11.5px] leading-[1.45] text-muted">
        {props.summary}
      </props.description>
      <FormField
        label="Name"
        placeholder="feature/my-change"
        value={props.name}
        onChange={(name) => props.onNameChange(name)}
      />
      <Button
        type="submit"
        variant="primary"
        icon={GitBranch}
        disabled={!props.name.trim() || props.disabled}
        class="h-8 w-full gap-[7px] rounded-[7px] text-[12.5px] font-[680] shadow-none"
      >
        Create branch
      </Button>
    </form>
  );
}
