import { useMemo } from "react";
import ReactMarkdown from "react-markdown";
import rehypeKatex from "rehype-katex";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import "katex/dist/katex.min.css";

interface SelectionResultProps {
  content: string;
}

const BARE_DISPLAY_COMMAND = /^\\(?:d?frac|tfrac|sqrt|i{1,3}nt|oint|lim|max|min|operatorname|prod|sum)\b/;
const CJK_TEXT = /[\u3400-\u9fff]/;
const ALIGNAT_ENVIRONMENT = /\\begin\{alignat(\*?)\}\{\d+\}([\s\S]*?)\\end\{alignat\1\}/g;
const DISPLAY_ENVIRONMENT = /\\begin\{(equation\*?|displaymath|align\*?|flalign\*?|gather\*?|multline\*?)\}([\s\S]*?)\\end\{\1\}/g;
const DISPLAY_DOLLAR_MATH = /(?<!\\)\$\$([\s\S]*?)(?<!\\)\$\$/g;
const FENCED_CODE_SEGMENT = /(```[\s\S]*?```|~~~[\s\S]*?~~~)/g;
const MATH_SEGMENT = /(?<!\\)\$\$[\s\S]*?(?<!\\)\$\$|(?<!\\)\$(?!\$)[^\n]*?(?<!\\)\$(?!\$)/g;
const MAX_FALLBACK_MACROS = 64;
const MAX_COMMAND_NAME_CHARS = 40;

function placeholder(index: number): string {
  return `LIGHTWHISPERPROTECTEDSEGMENT${index}END`;
}

function mapOutsideCode(content: string, transform: (segment: string) => string): string {
  return content.split(FENCED_CODE_SEGMENT).map((segment, segmentIndex) => {
    if (segmentIndex % 2 === 1) return segment;

    const codeSpans: string[] = [];
    const protectedSegment = segment.replace(/`+[^`\n]*`+/g, (code) => {
      const marker = `LIGHTWHISPERCODESPAN${codeSpans.length}END`;
      codeSpans.push(code);
      return marker;
    });
    return codeSpans.reduce(
      (text, code, index) => text.replace(`LIGHTWHISPERCODESPAN${index}END`, () => code),
      transform(protectedSegment),
    );
  }).join("");
}

function replaceLeafCommands(
  content: string,
  commands: string,
  replacement: (body: string) => string,
): string {
  const pattern = new RegExp(`\\\\(?:${commands})\\*?\\{([^{}]*)\\}`, "g");
  let current = content;
  for (let depth = 0; depth < 8; depth += 1) {
    let changed = false;
    const next = current.replace(pattern, (_, body: string) => {
      changed = true;
      return replacement(body);
    });
    current = next;
    if (!changed) break;
  }
  return current;
}

function humanizeLatexKey(value: string): string {
  const trimmed = value.trim();
  const year = trimmed.match(/(?:19|20)\d{2}/);
  const beforeYear = year?.index === undefined ? trimmed : trimmed.slice(0, year.index);
  const readableName = beforeYear
    .replace(/(?:et[_-]?al)$/i, "")
    .replace(/([a-z])([A-Z])/g, "$1 $2")
    .replace(/[_:.-]+/g, " ")
    .trim()
    .replace(/\b\w/g, (letter) => letter.toUpperCase());
  if (readableName && year) return `${readableName} ${year[0]}`;
  return trimmed.replace(/[_:.-]+/g, " ").trim() || value;
}

function humanizeReferenceLabel(value: string): string {
  const withoutKind = value.trim().replace(/^[a-z]+:/i, "");
  return withoutKind.replace(/[_-]+/g, " ").trim() || value;
}

function citationAuthor(value: string): string {
  return humanizeLatexKey(value).replace(/\s+(?:19|20)\d{2}$/, "");
}

function citationYear(value: string): string {
  return value.match(/(?:19|20)\d{2}/)?.[0] ?? humanizeLatexKey(value);
}

function normalizeLatexTextCommands(content: string): string {
  let normalized = content
    .replace(/~?\\citeyear\*?(?:\[[^\]]*\]){0,2}\{([^{}]+)\}/g, (_, keys: string) => (
      ` ${keys.split(",").map(citationYear).join("; ")}`
    ))
    .replace(/~?\\citeauthor\*?(?:\[[^\]]*\]){0,2}\{([^{}]+)\}/g, (_, keys: string) => (
      ` ${keys.split(",").map(citationAuthor).join("; ")}`
    ))
    .replace(/~?\\citet\*?(?:\[[^\]]*\]){0,2}\{([^{}]+)\}/g, (_, keys: string) => {
      const citations = keys.split(",").map((key) => {
        const author = citationAuthor(key);
        const year = key.match(/(?:19|20)\d{2}/)?.[0];
        return year ? `${author} (${year})` : humanizeLatexKey(key);
      });
      return ` ${citations.join("; ")}`;
    })
    .replace(/~?\\citealp\*?(?:\[[^\]]*\]){0,2}\{([^{}]+)\}/g, (_, keys: string) => (
      ` ${keys.split(",").map(humanizeLatexKey).join("; ")}`
    ))
    .replace(/~?\\(?:cite|citep)\*?(?:\[[^\]]*\]){0,2}\{([^{}]+)\}/g, (_, keys: string) => {
      const citations = keys.split(",").map(humanizeLatexKey).filter(Boolean);
      return citations.length ? ` [${citations.join("; ")}]` : "";
    })
    .replace(/~?\\eqref\{([^{}]+)\}/g, (_, label: string) => ` (${humanizeReferenceLabel(label)})`)
    .replace(/~?\\(?:ref|autoref|cref|Cref)\{([^{}]+)\}/g, (_, label: string) => ` ${humanizeReferenceLabel(label)}`)
    .replace(/\\label\{[^{}]*\}/g, "")
    .replace(/\\(?:section|chapter)\*?\{([^{}]+)\}/g, "\n## $1\n")
    .replace(/\\subsection\*?\{([^{}]+)\}/g, "\n### $1\n")
    .replace(/\\subsubsection\*?\{([^{}]+)\}/g, "\n#### $1\n")
    .replace(/\\paragraph\*?\{([^{}]+)\}/g, "\n**$1** ");

  for (let depth = 0; depth < 8; depth += 1) {
    const previous = normalized;
    normalized = replaceLeafCommands(normalized, "emph|textit", (body) => `*${body}*`);
    normalized = replaceLeafCommands(normalized, "textbf", (body) => `**${body}**`);
    normalized = replaceLeafCommands(normalized, "texttt", (body) => `\`${body.replace(/`/g, "\\`")}\``);
    normalized = replaceLeafCommands(normalized, "underline|mbox|text", (body) => body);
    if (normalized === previous) break;
  }

  return normalized.replace(/\\([%&#_{}])/g, "$1");
}

function normalizeDisplayEnvironments(content: string): string {
  const aligned = content.replace(ALIGNAT_ENVIRONMENT, (_, _star: string, body: string) => {
    const cleanBody = body.replace(/\\label\{[^{}]*\}/g, "").trim();
    return `\n$$\n\\begin{aligned}\n${cleanBody}\n\\end{aligned}\n$$\n`;
  });
  return aligned.replace(DISPLAY_ENVIRONMENT, (_, environment: string, body: string) => {
    const cleanBody = body.replace(/\\label\{[^{}]*\}/g, "").trim();
    const baseEnvironment = environment.replace(/\*$/, "");
    let math = cleanBody;
    if (["align", "alignat", "flalign", "multline"].includes(baseEnvironment)) {
      math = `\\begin{aligned}\n${cleanBody}\n\\end{aligned}`;
    } else if (baseEnvironment === "gather") {
      math = `\\begin{gathered}\n${cleanBody}\n\\end{gathered}`;
    }
    return `\n$$\n${math}\n$$\n`;
  });
}

function isEscapedAt(content: string, index: number): boolean {
  let backslashes = 0;
  for (let cursor = index - 1; cursor >= 0 && content[cursor] === "\\"; cursor -= 1) {
    backslashes += 1;
  }
  return backslashes % 2 === 1;
}

function nextSingleDollar(content: string, start: number): number {
  for (let index = start; index < content.length; index += 1) {
    if (
      content[index] === "$"
      && !isEscapedAt(content, index)
      && content[index - 1] !== "$"
      && content[index + 1] !== "$"
    ) return index;
  }
  return -1;
}

function isLikelyCurrencyOpening(content: string, index: number): boolean {
  if (!/\d/.test(content[index + 1] ?? "")) return false;
  const closing = nextSingleDollar(content, index + 1);
  if (closing < 0) return true;

  const body = content.slice(index + 1, closing);
  const afterClosing = content[closing + 1] ?? "";
  if (/\s$/.test(body)) return true;
  const nextLooksLikeMath = Boolean(afterClosing)
    && !/[\s,.;:!?，。：；！？)\]}]/.test(afterClosing);
  return nextLooksLikeMath && /[,.;:?，。：；！？/\-–—]$/.test(body);
}

function escapeLikelyCurrencyDollars(content: string): string {
  let escaped = "";
  for (let index = 0; index < content.length; index += 1) {
    const isSingleDollar = content[index] === "$"
      && !isEscapedAt(content, index)
      && content[index - 1] !== "$"
      && content[index + 1] !== "$";
    escaped += isSingleDollar && isLikelyCurrencyOpening(content, index) ? "\\$" : content[index];
  }
  return escaped;
}

function protectMath(content: string): { text: string; segments: string[] } {
  const segments: string[] = [];
  let text = content.replace(DISPLAY_DOLLAR_MATH, (
    math,
    body: string,
    offset: number,
    source: string,
  ) => {
    const marker = placeholder(segments.length);
    const prefix = offset === 0 || source[offset - 1] === "\n" ? "" : "\n\n";
    const end = offset + math.length;
    const suffix = end === source.length || source[end] === "\n" ? "" : "\n\n";
    const cleanBody = body.replace(/\\label\{[^{}]*\}/g, "").trim();
    segments.push(`${prefix}$$\n${cleanBody}\n$$${suffix}`);
    return marker;
  });

  text = escapeLikelyCurrencyDollars(text);

  let cursor = 0;
  let protectedText = "";
  while (cursor < text.length) {
    const opening = nextSingleDollar(text, cursor);
    if (opening < 0) {
      protectedText += text.slice(cursor);
      break;
    }
    const closing = nextSingleDollar(text, opening + 1);
    if (closing < 0) {
      protectedText += `${text.slice(cursor, opening)}\\$${text.slice(opening + 1)}`;
      break;
    }
    const body = text.slice(opening + 1, closing);
    const marker = placeholder(segments.length);
    segments.push(`$${body.replace(/\\label\{[^{}]*\}/g, "")}$`);
    protectedText += text.slice(cursor, opening) + marker;
    cursor = closing + 1;
  }

  text = protectedText;
  return { text, segments };
}

function restoreProtectedSegments(content: string, segments: string[]): string {
  return segments.reduce(
    (text, segment, index) => text.replace(placeholder(index), () => segment),
    content,
  );
}

function addFallbackMacroDefinitions(content: string): string {
  return mapOutsideCode(content, (segment) => segment.replace(MATH_SEGMENT, (math) => {
    const delimiter = math.startsWith("$$") ? "$$" : "$";
    const body = math.slice(delimiter.length, -delimiter.length);
    const sourceDefined = new Set<string>();
    for (const match of body.matchAll(
      /\\(?:newcommand|renewcommand|providecommand)\*?\s*(?:\{\s*)?\\([A-Za-z]+)/g,
    )) sourceDefined.add(match[1]);
    for (const match of body.matchAll(/\\(?:def|gdef|edef|xdef)\s*\\([A-Za-z]+)/g)) {
      sourceDefined.add(match[1]);
    }

    const commands = new Set<string>();
    for (const match of body.matchAll(/\\([A-Za-z]+)(?![A-Za-z])/g)) {
      const command = match[1];
      if (
        command
        && command.length <= MAX_COMMAND_NAME_CHARS
        && !sourceDefined.has(command)
      ) commands.add(command);
      if (commands.size >= MAX_FALLBACK_MACROS) break;
    }
    const definitions = [...commands]
      .map((command) => `\\providecommand{\\${command}}{\\operatorname{${command}}}`)
      .join("");
    return `${delimiter}${definitions}${body}${delimiter}`;
  }));
}

export function normalizeSelectionMath(content: string): string {
  return mapOutsideCode(content, (segment) => {
    let normalized = segment;
    normalized = normalizeDisplayEnvironments(normalized)
      .replace(/\\\[([\s\S]*?)\\\]/g, (_, math: string) => `\n$$\n${math.trim()}\n$$\n`)
      .replace(/\\\(([\s\S]*?)\\\)/g, (_, math: string) => `$${math.trim()}$`);

    const protectedMath = protectMath(normalized);
    normalized = normalizeLatexTextCommands(protectedMath.text);

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
        || (commandCount >= 2 && /[_^=+*/<>-]/.test(trimmed));
      return looksLikeBareDisplay ? ["$$", trimmed, "$$"] : [line];
    }).join("\n");

    normalized = restoreProtectedSegments(normalized, protectedMath.segments);

    return normalized;
  });
}

export function SelectionResult({ content }: SelectionResultProps) {
  const normalized = useMemo(() => normalizeSelectionMath(content), [content]);
  const renderable = useMemo(() => addFallbackMacroDefinitions(normalized), [normalized]);

  return (
    <div className="selection-result-content selection-markdown">
      <ReactMarkdown
        remarkPlugins={[remarkGfm, remarkMath]}
        rehypePlugins={[[rehypeKatex, {
          throwOnError: false,
          strict: "ignore",
        }]]}
        skipHtml
        components={{
          a: ({ children, ...props }) => (
            <a {...props} target="_blank" rel="noreferrer">
              {children}
            </a>
          ),
        }}
      >
        {renderable}
      </ReactMarkdown>
    </div>
  );
}
