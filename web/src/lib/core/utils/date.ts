import { format } from "date-fns";
import { enGB } from "date-fns/locale";
import * as v from "valibot";

export function formatDateTime(date: Date) {
  return format(date, "PPp", { locale: enGB });
}

export function formatDate(date: Date) {
  return format(date, "P", { locale: enGB });
}

/*
2016-04-14T10:10:11Z
2016-04-14T10:10:11.123Z
*/
export const IsoDateTimeRegExp =
  /^(?:19|20)\d{2}-(?:0[1-9]|1[0-2])-(?:0[1-9]|[12]\d|3[01])T(?:[01]\d|2[0-3]):[0-5]\d:[0-5]\d(|.\d{3})(?:Z)$/;

/*
2016-04-14
*/
export const IsoDateRegExp = /^(?:19|20)\d{2}-(?:0[1-9]|1[0-2])-(?:0[1-9]|[12]\d|3[01])$/;

export const IsoDateSchema = v.pipe(v.string(), v.isoDate(), v.transform((s) => s as IsoDateLiteral));

type Year = number;
type Month = "01" | "02" | "03" | "04" | "05" | "06" | "07" | "08" | "09" | "10" | "11" | "12";
type Day =
  | "01"
  | "02"
  | "03"
  | "04"
  | "05"
  | "06"
  | "07"
  | "08"
  | "09"
  | "10"
  | "11"
  | "12"
  | "13"
  | "14"
  | "15"
  | "16"
  | "17"
  | "18"
  | "19"
  | "20"
  | "21"
  | "22"
  | "23"
  | "24"
  | "25"
  | "26"
  | "27"
  | "28"
  | "29"
  | "30"
  | "31";

export type IsoDateLiteral = `${Year}-${Month}-${Day}`;

export type IsoDate = v.InferOutput<typeof IsoDateSchema>;

export function fromIsoDate(isoDate: IsoDate) {
  isoDate = v.parse(IsoDateSchema, isoDate);

  const date = new Date(Date.UTC(
    parseInt(isoDate.substring(0, 4)), // year
    parseInt(isoDate.substring(5, 7)) - 1, // month (0-indexed)
    parseInt(isoDate.substring(8, 10)), // day
  ));

  return date;
}

export function toIsoDate(date: Date) {
  return date.toJSON().split("T")[0];
}
