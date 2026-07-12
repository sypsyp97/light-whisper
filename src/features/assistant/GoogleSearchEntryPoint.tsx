import { useEffect, useRef } from "react";

const BLOCKED_ELEMENTS = "script,iframe,object,embed,form,input,button,textarea,meta,link,base";

export function sanitizeGoogleSearchEntryPointHtml(html: string): string {
  if (!html.trim() || html.length > 64_000) return "";
  const document = new DOMParser().parseFromString(html, "text/html");
  document.querySelectorAll(BLOCKED_ELEMENTS).forEach((element) => element.remove());
  document.querySelectorAll("*").forEach((element) => {
    for (const attribute of Array.from(element.attributes)) {
      const name = attribute.name.toLowerCase();
      if (name.startsWith("on") || name === "srcdoc") {
        element.removeAttribute(attribute.name);
      }
    }
    if (element instanceof HTMLAnchorElement) {
      try {
        const url = new URL(element.href);
        if (url.protocol !== "https:") element.removeAttribute("href");
      } catch {
        element.removeAttribute("href");
      }
      element.removeAttribute("target");
    }
  });
  const styles = Array.from(document.head.querySelectorAll("style"))
    .map((style) => style.outerHTML)
    .join("");
  return `${styles}${document.body.innerHTML}`;
}

export default function GoogleSearchEntryPoint({
  html,
  label,
  onOpen,
}: {
  html: string;
  label: string;
  onOpen(url: string): void;
}) {
  const hostRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const root = host.shadowRoot ?? host.attachShadow({ mode: "open" });
    root.innerHTML = sanitizeGoogleSearchEntryPointHtml(html);
    const handleClick = (event: Event) => {
      const anchor = event.composedPath().find(
        (node): node is HTMLAnchorElement => node instanceof HTMLAnchorElement,
      );
      if (!anchor?.href) return;
      event.preventDefault();
      event.stopPropagation();
      onOpen(anchor.href);
    };
    root.addEventListener("click", handleClick);
    return () => root.removeEventListener("click", handleClick);
  }, [html, onOpen]);

  return <div ref={hostRef} className="subtitle-google-search-entry" aria-label={label} />;
}
