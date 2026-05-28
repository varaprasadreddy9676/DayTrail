export type RangePreset = "today" | "yesterday" | "last7" | "thisMonth" | "custom";

export function formatLocalDateInput(date: Date) {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

export function dateRangeForPreset(preset: Exclude<RangePreset, "custom">, now = new Date()) {
  const start = new Date(now);
  start.setHours(0, 0, 0, 0);
  const end = new Date(start);

  if (preset === "yesterday") {
    start.setDate(start.getDate() - 1);
    end.setDate(end.getDate() - 1);
  } else if (preset === "last7") {
    start.setDate(start.getDate() - 6);
  } else if (preset === "thisMonth") {
    start.setDate(1);
  }

  return {
    fromDate: formatLocalDateInput(start),
    toDate: formatLocalDateInput(end),
  };
}

export function rangePresetLabel(preset: RangePreset) {
  switch (preset) {
    case "today":
      return "Today";
    case "yesterday":
      return "Yesterday";
    case "last7":
      return "Last 7 days";
    case "thisMonth":
      return "This month";
    case "custom":
      return "Custom range";
  }
}
