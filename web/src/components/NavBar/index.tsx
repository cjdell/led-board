import { For } from "solid-js";
import "./style.scss";
import { RouteDefinition } from "@lib";

interface Props {
  routes: readonly RouteDefinition[];
  pathname: string;
}

export function NavBar(props: Props) {
  const isSelected = (path: string) => {
    if (props.pathname.substring(1).includes("/")) {
      return path.split("/")[1] === props.pathname.split("/")[1];
    } else {
      return path === props.pathname;
    }
  };

  const onNavClick = () => {
    globalThis.document.querySelector(".container > *")!.scrollTo(0, 0);
  };

  return (
    <nav class="NavBar" on:click={onNavClick}>
      <For each={props.routes.filter((r) => !r.path.includes(":"))}>
        {(route) => (
          <a classList={{ "NavBar__link": true, "NavBar__link--selected": isSelected(route.path) }} href={route.path}>
            {route.label}
          </a>
        )}
      </For>
    </nav>
  );
}
