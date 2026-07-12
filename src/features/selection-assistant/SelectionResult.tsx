import ReactMarkdown from "react-markdown";
import rehypeKatex from "rehype-katex";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import "katex/dist/katex.min.css";

interface SelectionResultProps {
  content: string;
}

const BARE_DISPLAY_COMMAND = /^\\(?:begin|frac|int|lim|max|min|operatorname|prod|sum)\b/;
const CJK_TEXT = /[\u3400-\u9fff]/;

export function normalizeSelectionMath(content: string): string {
  const fencedSegments = content.split(/(```[\s\S]*?```|~~~[\s\S]*?~~~)/g);
  return fencedSegments.map((segment, segmentIndex) => {
    if (segmentIndex % 2 === 1) return segment;

    const codeSpans: string[] = [];
    let normalized = segment.replace(/`+[^`\n]*`+/g, (code) => {
      const placeholder = `LIGHTWHISPERCODESPAN${codeSpans.length}END`;
      codeSpans.push(code);
      return placeholder;
    });

    normalized = normalized
      .replace(/\\\[([\s\S]*?)\\\]/g, (_, math: string) => `\n$$\n${math.trim()}\n$$\n`)
      .replace(/\\\(([\s\S]*?)\\\)/g, (_, math: string) => `$${math.trim()}$`);

    let insideDisplayMath = false;
    normalized = normalized.split("\n").flatMap((line) => {
      const trimmed = line.trim();
      if (trimmed === "$$") {
        insideDisplayMath = !insideDisplayMath;
        return [line];
      }
      if (insideDisplayMath || !trimmed || CJK_TEXT.test(trimmed)) return [line];
      const commandCount = trimmed.match(/\\[A-Za-z]+/g)?.length ?? 0;
      const looksLikeBareDisplay = BARE_DISPLAY_COMMAND.test(trimmed)
        || (commandCount >= 2 && /[_^=]/.test(trimmed));
      return looksLikeBareDisplay ? ["$$", trimmed, "$$"] : [line];
    }).join("\n");

    return codeSpans.reduce(
      (text, code, index) => text.replace(`LIGHTWHISPERCODESPAN${index}END`, code),
      normalized,
    );
  }).join("");
}

export function SelectionResult({ content }: SelectionResultProps) {
  return (
    <div className="selection-result-content selection-markdown">
      <ReactMarkdown
        remarkPlugins={[remarkGfm, remarkMath]}
        rehypePlugins={[[rehypeKatex, { throwOnError: false, strict: "ignore" }]]}
        skipHtml
        components={{
          a: ({ children, ...props }) => (
            <a {...props} target="_blank" rel="noreferrer">
              {children}
            </a>
          ),
        }}
      >
        {normalizeSelectionMath(content)}
      </ReactMarkdown>
    </div>
  );
}
