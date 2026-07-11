// deno-lint-ignore-file no-explicit-any
import { createSignal, type JSX } from "solid-js";
import { Colour, handleAsyncClick } from "@lib";

type LinkOrActionProps = {
  "on:click": Exclude<JSX.HTMLElementTags["button"]["on:click"], undefined>;
};

type Props =
  & JSX.HTMLElementTags["button"]
  & LinkOrActionProps
  & {
    colour: Colour;
  };

export function Button(props: Props) {
  const [working, setWorking] = createSignal(false);

  const onClick = handleAsyncClick(props["on:click"] as any, setWorking);

  return (
    <button
      {...props}
      classList={{
        ...props.classList,
        btn: true,
        ["btn-" + props.colour]: true,
      }}
      disabled={props.disabled || working()}
      on:click={onClick}
    />
  );
}
