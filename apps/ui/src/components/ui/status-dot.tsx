type Props = {
  color: string;
};

export function StatusDot(props: Props) {
  return <i class="size-1.5 rounded-full" style={{ "background-color": props.color }} />;
}
