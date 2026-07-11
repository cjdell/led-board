// deno-lint-ignore-file no-explicit-any
import { assertUnreachable, getFieldInfo } from "@lib";
import { format } from "date-fns";
import { enGB } from "date-fns/locale";
import { For } from "solid-js";
import * as v from "valibot";
import { DateInput } from "../DateInput/index.tsx";
import { FormFields } from "../FormFields/index.tsx";
import { Select } from "../Select/index.tsx";
import { TextInput } from "../TextInput/index.tsx";
import { NestedMagicFields } from "./nested.tsx";

interface Props<TSchema extends v.ObjectSchema<any, any>, TData extends v.InferInput<TSchema>> {
  schema: TSchema;
  /** Partials are intentionally allowed here to make initialisation easier */
  data: Partial<TData>;
  validation: boolean;
  formFieldClass?: string;

  onChange: (data: Partial<TData>) => void;
}

export function MagicFields<
  TSchema extends v.ObjectSchema<any, any>,
  TData extends v.InferInput<TSchema>,
>(props: Props<TSchema, TData>) {
  const fieldsNames = Object.keys(props.schema.entries) as unknown as readonly Extract<keyof TData, string>[];

  const getValidationMessages = (fieldName: keyof TData) => {
    if (!props.validation) return [];

    const validation = v.safeParse(props.schema, props.data);
    const issues = validation.issues?.filter((i): i is v.BaseIssue<any> => "path" in i && "message" in i);

    return issues?.filter((i) => i.path?.length === 1 && i.path[0].key === fieldName).map((i) => i.message) ?? [];
  };

  const onFieldChange = (fieldName: Extract<keyof TData, string>, value: string | unknown[] | undefined | null) => {
    props.onChange({
      ...props.data,
      [fieldName]: value,
    });
  };

  return (
    <FormFields formFieldClass={props.formFieldClass}>
      <For each={fieldsNames}>
        {(fieldName) => {
          const { metadata, title, inputType, options, description, arrayItemSchema } = getFieldInfo(
            props.schema,
            fieldName,
          );

          const value = () => props.data[fieldName];

          const readonly = metadata?.readonly ?? false;

          return (
            <FormFields.Field
              id={fieldName}
              title={title}
              description={description}
              icon={metadata?.icon}
              raw={inputType === "array"}
              messages={getValidationMessages(fieldName)}
            >
              {inputType === "text" || inputType === "email" || inputType === "password" || inputType === "textarea"
                ? (
                  <TextInput
                    type={inputType}
                    id={fieldName}
                    isInvalid={getValidationMessages(fieldName).length > 0}
                    placeholder={title}
                    value={value()}
                    readonly={readonly}
                    onChange={(v) => onFieldChange(fieldName, v)}
                  />
                )
                : inputType === "datetime" // TODO
                ? (
                  <TextInput
                    type="text"
                    id={fieldName}
                    isInvalid={getValidationMessages(fieldName).length > 0}
                    placeholder={title}
                    value={(typeof value() === "object") ? format(value() as Date, "PPp", { locale: enGB }) : ""}
                    readonly
                    onChange={(v) => onFieldChange(fieldName, v)}
                  />
                )
                : inputType === "date"
                ? (
                  <DateInput
                    id={fieldName}
                    isInvalid={getValidationMessages(fieldName).length > 0}
                    placeholder={title}
                    value={value()}
                    readonly
                    onChange={(v) => onFieldChange(fieldName, v)}
                  />
                )
                : inputType === "select"
                ? (
                  <Select
                    id={fieldName}
                    isInvalid={getValidationMessages(fieldName).length > 0}
                    placeholder={title}
                    value={value()}
                    options={options}
                    allowNull
                    onChange={(v) => onFieldChange(fieldName, v)}
                  />
                )
                : inputType === "array"
                ? (
                  <>
                    <h6 class="g-col-12 m-0">{title}</h6>
                    <NestedMagicFields
                      items={value()}
                      itemSchema={arrayItemSchema!}
                      validation={props.validation}
                      onChange={(items) => onFieldChange(fieldName, items)}
                    />
                  </>
                )
                : inputType === "lookup"
                ? (
                  "Lookup"
                )
                : assertUnreachable(inputType)}
            </FormFields.Field>
          );
        }}
      </For>
    </FormFields>
  );
}
