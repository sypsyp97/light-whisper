import MarkdownContent from "@/components/MarkdownContent";

export { normalizeSelectionMath } from "@/components/MarkdownContent";

export function SelectionResult({ content, streaming }: { content: string; streaming?: boolean }) {
  return <MarkdownContent content={content} streaming={streaming} className="selection-result-content" />;
}
