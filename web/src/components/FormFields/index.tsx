import { children, createMemo, For, type JSXElement } from "solid-js";

interface Props {
  formFieldClass?: string;
  children: JSXElement;
}

export function FormFields(props: Props) {
  const components = children(() => props.children) as unknown as () => FieldProps[];

  const fields = createMemo(() => {
    let parts = components();
    if (!Array.isArray(parts)) parts = [parts];
    return parts;
  });

  return (
    <div class="grid gap-3">
      <For each={fields()}>
        {(field) => (
          field.raw ? field.children : (
            <div class={`form-group mb-0 ${props.formFieldClass ?? "g-col-12"}`}>
              {!field.icon && <label for={field.id}>{field.title}</label>}

              <div classList={{ "input-group": true, "has-validation": field.messages.length > 0 }}>
                {field.icon && <span class="input-group-text">{field.icon}</span>}
                {field.children}
                <For each={field.messages}>{(m) => <div class="invalid-feedback">{m}</div>}</For>
              </div>
              {field.description && <div class="form-text">{field.description}</div>}
            </div>
          )
        )}
      </For>
    </div>
  );
}

interface FieldProps {
  title: string;
  id: string;
  icon?: string;
  description?: string;
  messages: string[];
  raw?: boolean;
  children: JSXElement;
}

FormFields.Field = (props: FieldProps) => {
  return <>{props}</>;
};
