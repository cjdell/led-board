import { assert, type ElementOf } from "ts-essentials";

export function assertError(err: unknown): asserts err is Error {
  assert(err instanceof Error, "Error is not an instance of `Error`");
}

export function assertUnreachable(x: never): never {
  console.error("assertUnreachable:", x);

  throw new Error(`An unreachable event has occurred: ${String(x)} / ${typeof x}`);
}

export function includes<L extends readonly unknown[]>(t: unknown, list: L): t is ElementOf<L> {
  return list.includes(t);
}

export function keys<T extends object>(obj: T) {
  return Object.keys(obj) as unknown as readonly (keyof T)[];
}

export function stringKeys<T extends object>(obj: T) {
  return Object.keys(obj).filter((k) => typeof k === "string") as unknown as readonly Extract<keyof T, string>[];
}

export type PropsOf<TComponent> = TComponent extends (props: infer T) => void ? T : never;

export function titleCase(str: string) {
  str = str.toLowerCase();

  const str2 = str.split(" ");

  for (let i = 0; i < str2.length; i++) {
    str2[i] = str2[i].charAt(0).toUpperCase() + str2[i].slice(1);
  }

  return str2.join(" ");
}

export function humanise(inputString: string) {
  const formattedString = inputString.replace(/[-_]/g, " ");

  const finalFormattedString = formattedString.replace(/([a-z])([A-Z])/g, "$1 $2");

  return finalFormattedString.replace(/\b\w/g, (match) => match.toUpperCase());
}

export function camelToPascal(camelCaseString: string) {
  return camelCaseString.charAt(0).toUpperCase() + camelCaseString.slice(1);
}

export const isArray = (t: unknown): t is Array<unknown> => {
  return t instanceof Array;
};

export const isNotNull = <T>(t: T): t is Exclude<T, null> => {
  return t !== null;
};

export const isNotNullOrUndefined = <T>(t: T): t is Exclude<T, null | undefined> => {
  return t !== null && t !== undefined;
};

export function sleep(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

type PickPrefix<S extends string, P extends string> = S extends `${P}${string}` ? S : never;

export function pickPrefix<TObj extends object, TPrefix extends string>(obj: TObj, prefix: TPrefix) {
  return Object.fromEntries(Object.entries(obj).filter(([e]) => e.startsWith(prefix))) as Pick<
    TObj,
    PickPrefix<Extract<keyof TObj, string>, TPrefix>
  >;
}

export function pick<TObj extends object, TProp extends keyof TObj>(obj: TObj, props: readonly TProp[]) {
  return Object.fromEntries(
    Object.entries(obj).filter(
      ([e]) => (props as readonly string[]).includes(e),
    ),
  ) as Pick<TObj, TProp>;
}

export function omit<TObj extends object, TProp extends keyof TObj>(obj: TObj, props: readonly TProp[]) {
  return Object.fromEntries(
    Object.entries(obj).filter(
      ([e]) => !(props as readonly string[]).includes(e),
    ),
  ) as Omit<TObj, TProp>;
}

export type KeysOfValue<T, V> = { [K in keyof T]-?: T[K] extends V ? K : never }[keyof T];

export type PickOfValue<T, V> = Pick<T, KeysOfValue<T, V>>;

const AwaitInterval = 10;

export async function awaitValue<T extends object>(label: string, test: () => T | undefined, maxWait = 60_000): Promise<T> {
  let t = 0;

  // eslint-disable-next-line no-constant-condition
  while (true) {
    const v = test();
    if (v !== undefined) return v;
    if (t >= 5_000 && t % 1_000 === 0) console.warn("Stll waiting for value:", label, t);
    if (t >= maxWait) {
      const msg = `Await for "${label}" exceeded maximum wait time of ${maxWait / 1000} seconds`;
      console.error(msg);
      throw new Error(msg);
    }
    await sleep(AwaitInterval);
    t += AwaitInterval;
  }
}

export async function awaitTrue(label: string, test: () => boolean | Promise<boolean>, maxWait = 60_000): Promise<void> {
  let t = 0;

  // console.time(`Await True: ${label}`);

  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition, no-constant-condition
  while (true) {
    const v = await test();
    if (v) {
      // console.timeEnd(`Await True: ${label}`);
      return;
    }
    if (t >= 5_000 && t % 1_000 === 0) console.warn("Stll waiting for true:", label, t);
    if (t >= maxWait) {
      const msg = `Await for "${label}" exceeded maximum wait time of ${maxWait / 1000} seconds`;
      console.error(msg);
      throw new Error(msg);
    }
    await sleep(AwaitInterval);
    t += AwaitInterval;
  }
}

export function objectsEqual<T>(a: T, b: NoInfer<T>): boolean {
  throw new Error("objectsEqual");
}

export function isIpAddress(host: string) {
  if (
    /^(25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.(25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.(25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.(25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)$/
      .test(host)
  ) {
    return true;
  }
  return false;
}
