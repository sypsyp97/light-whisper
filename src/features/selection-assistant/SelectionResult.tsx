import MarkdownContent from "@/components/MarkdownContent";

export { normalizeSelectionMath } from "@/components/MarkdownContent";

export function SelectionResult({ content }: { content: string }) {
  return <MarkdownContent content={content} className="selection-result-content" />;
}
