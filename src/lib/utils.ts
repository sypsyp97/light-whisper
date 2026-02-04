/**
 * Merge CSS class names, filtering out falsy values.
 *
 * This is a lightweight alternative to `clsx` + `tailwind-merge`.
 * Pass plain strings or conditional expressions that may resolve to
 * empty strings / undefined / null -- only truthy values are kept.
 *
 * @example
 * cn("px-4", isActive && "bg-blue-500", "text-sm")
 * // => "px-4 bg-blue-500 text-sm"  (when isActive is true)
 */
export function cn(...inputs: (string | undefined | null | false)[]): string {
  return inputs.filter(Boolean).join(" ");
}
