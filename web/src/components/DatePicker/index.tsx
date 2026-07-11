import { createEffect, createMemo, createSignal, For } from "solid-js";

interface Props {
  value: Date | undefined;
  onChange: (date: Date) => void;
}

export function DatePicker(props: Props) {
  const [currentDate, setCurrentDate] = createSignal(new Date());

  createEffect(() => {
    if (props.value) setCurrentDate(props.value);
  });

  const monthNames = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
  ];

  const dayNames = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];

  const month = createMemo(() => currentDate().getMonth());
  const year = createMemo(() => currentDate().getFullYear());

  const daysInMonth = createMemo(() => {
    return new Date(Date.UTC(year(), month() + 1, 0)).getDate();
  });

  const firstDayOfMonth = createMemo(() => {
    return new Date(Date.UTC(year(), month(), 1)).getDay();
  });

  const daysArray = createMemo(() => {
    const days = [];
    const daysInPrevMonth = new Date(Date.UTC(year(), month(), 0)).getDate();

    // Previous month days
    for (let i = firstDayOfMonth() - 1; i >= 0; i--) {
      const day = daysInPrevMonth - i;
      days.push({
        day,
        isCurrentMonth: false,
        date: new Date(Date.UTC(year(), month() - 1, day)),
      });
    }

    // Current month days
    for (let i = 1; i <= daysInMonth(); i++) {
      days.push({
        day: i,
        isCurrentMonth: true,
        date: new Date(Date.UTC(year(), month(), i)),
      });
    }

    // Next month days
    const totalCells = 42; // 6 weeks
    const nextMonthDays = totalCells - days.length;
    for (let i = 1; i <= nextMonthDays; i++) {
      days.push({
        day: i,
        isCurrentMonth: false,
        date: new Date(Date.UTC(year(), month() + 1, i)),
      });
    }

    return days;
  });

  // const formattedDate = createMemo(() => {
  //   if (!selectedDate()) return "";
  //   return selectedDate().toLocaleDateString("en-US", {
  //     month: "short",
  //     day: "numeric",
  //     year: "numeric",
  //   });
  // });

  const handleDateSelect = (date: Date) => {
    console.log("handleDateSelect", date);
    props.onChange(date);
    // setIsOpen(false);
    // if (props.onChange) props.onChange(date);
  };

  const navigateMonth = (direction: number) => {
    setCurrentDate((prev) => {
      const newDate = new Date(prev);
      newDate.setMonth(prev.getMonth() + direction);
      return newDate;
    });
  };

  // const handleClickOutside = (e: PointerEvent) => {
  //   assert(e.target instanceof Element, "Element");

  //   if (!e.target.closest(".date-picker-container")) {
  //     setIsOpen(false);
  //   }
  // };

  // onMount(() => {
  //   document.addEventListener("click", handleClickOutside);
  //   return () => document.removeEventListener("click", handleClickOutside);
  // });

  return (
    <div class="date-picker">
      <div class="header">
        <button type="button" onClick={() => navigateMonth(-1)}>{"<"}</button>
        <span class="month-year">
          {monthNames[month()]} {year()}
        </span>
        <button type="button" onClick={() => navigateMonth(1)}>{">"}</button>
      </div>

      <div class="weekdays">
        <For each={dayNames}>
          {(day) => <span>{day}</span>}
        </For>
      </div>

      <div class="days-grid">
        <For each={daysArray()}>
          {(day) => (
            <button
              type="button"
              class={`day ${day.isCurrentMonth ? "current-month" : "other-month"} ${
                props.value && props.value.toDateString() === day.date.toDateString() ? "selected" : ""
              }`}
              onClick={() => handleDateSelect(day.date)}
            >
              {day.day}
            </button>
          )}
        </For>
      </div>
    </div>
  );
}
