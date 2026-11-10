import { children, type JSX, Show } from "solid-js";

interface PanelProps {
  children: JSX.Element;
  accent?: "amber" | "cyan" | "red";
  class?: string;
}

/// Card replacement: header / body / footer passed as sub-components, same
/// shape the old `Card` used so pages stay readable.
export function Panel(props: PanelProps) {
  const parts = children(() => props.children) as unknown as () => any[];

  return (
    <section class={`panel ${props.accent ? `panel--accent-${props.accent}` : ""} ${props.class ?? ""}`}>
      {parts()}
    </section>
  );
}

interface HeaderProps {
  text?: JSX.Element;
  children?: JSX.Element;
}

export function PanelHeader(props: HeaderProps) {
  return <header class="panel__hd">{props.text ?? props.children}</header>;
}

interface BodyProps {
  children: JSX.Element;
  flush?: boolean;
  scroll?: boolean;
  class?: string;
}

export function Body(props: BodyProps) {
  return (
    <div
      class={`panel__bd ${props.flush ? "panel__bd--flush" : ""} ${props.scroll ? "panel__bd--scroll" : ""} ${props.class ?? ""}`}
    >
      {props.children}
    </div>
  );
}

interface FooterProps {
  children: JSX.Element;
}

export function Footer(props: FooterProps) {
  return <footer class="panel__ft">{props.children}</footer>;
}

export function Spacer() {
  return <span class="spacer" />;
}
