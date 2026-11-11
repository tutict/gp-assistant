const LEADING_WHITESPACE = /^\s/u;
const TRAILING_ELLIPSIS = /(?:…+|\.{3,})$/u;

/** Keep information added by the summary, including summaries from older caches. */
export function newsSummary(title?: string | null, value?: string | null): string {
  const heading = typeof title === "string" ? title.trim() : "";
  let text = typeof value === "string" ? value.trim() : "";
  if (!heading || !text) return text;
  if (!text.split(heading).join("").trim()) return "";

  while (text.startsWith(heading)) {
    const rest = text.slice(heading.length);
    if (rest && !LEADING_WHITESPACE.test(rest)) break;
    text = rest.trimStart();
  }

  // Old excerpts may end halfway through the second copy of a long headline.
  const fragment = text.replace(TRAILING_ELLIPSIS, "").trim();
  if (fragment !== text && (!fragment || heading.startsWith(fragment))) return "";
  return text;
}
