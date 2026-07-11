interface Props {
  id: string;
  type: "text" | "email" | "password" | "textarea" | "date";
  isInvalid: boolean;
  placeholder: string;
  readonly: boolean;
  value: string | null | undefined;

  onChange: (value: string) => void;
}

export function TextInput(props: Props) {
  if (props.type === "textarea") {
    return (
      <textarea
        id={props.id}
        classList={{
          "form-control": true,
          "is-invalid": props.isInvalid,
          "value-null": props.value === null,
          "value-undefined": props.value === undefined,
        }}
        placeholder={props.placeholder}
        value={typeof props.value === "string" ? props.value : ""}
        readonly={props.readonly}
        on:change={(e) => props.onChange(e.currentTarget.value)}
      />
    );
  } else {
    return (
      <input
        type={props.type}
        id={props.id}
        classList={{
          "form-control": true,
          "is-invalid": props.isInvalid,
          "value-null": props.value === null,
          "value-undefined": props.value === undefined,
        }}
        placeholder={props.placeholder}
        value={typeof props.value === "string" ? props.value : ""}
        autocomplete={props.type === "password" ? "new-password" : "off"}
        readonly={props.readonly}
        on:change={(e) => props.onChange(e.currentTarget.value)}
      />
    );
  }
}
