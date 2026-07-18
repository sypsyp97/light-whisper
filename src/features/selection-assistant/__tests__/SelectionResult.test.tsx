import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { normalizeSelectionMath, SelectionResult } from "../SelectionResult";

describe("SelectionResult", () => {
  it("renders GFM structure instead of showing Markdown source", () => {
    const { container } = render(
      <SelectionResult
        content={[
          "## Summary",
          "",
          "**Important** detail.",
          "",
          "~~Outdated~~ wording.",
          "",
          "| Item | Value |",
          "| --- | --- |",
          "| Alpha | 42 |",
        ].join("\n")}
      />,
    );

    expect(screen.getByRole("heading", { level: 2, name: "Summary" })).toBeVisible();
    expect(screen.getByText("Important")).toHaveProperty("tagName", "STRONG");
    expect(screen.getByText("Outdated")).toHaveProperty("tagName", "DEL");
    expect(container.querySelector("table")).not.toBeNull();
  });

  it("renders dollar-delimited inline and display formulas with KaTeX", () => {
    const content = "Inline $E = mc^2$\n\n$$\n\\int_0^1 x^2 dx\n$$";
    const { container } = render(
      <SelectionResult content={content} />,
    );

    expect(normalizeSelectionMath(content)).toBe(content);
    expect(container.querySelectorAll(".katex")).toHaveLength(2);
    expect(container.querySelector(".katex-display")).not.toBeNull();
  });

  it("normalizes the bracket delimiters commonly returned by models", () => {
    const { container } = render(
      <SelectionResult
        content={"使用 \\(g_1\\) 和 \\(g_2\\)。\n\n\\[\n\\min_{\\theta} \\mathbb{E}[f_\\theta(y)]\n\\]"}
      />,
    );

    expect(container.querySelectorAll(".katex")).toHaveLength(3);
    expect(container.querySelector(".katex-display")).not.toBeNull();
  });

  it("wraps a standalone bare LaTeX optimization line as display math", () => {
    const source = "\\min_{\\theta} \\mathbb{E}_{y}[f_\\theta(y)]";
    const normalized = normalizeSelectionMath(source);
    const { container } = render(<SelectionResult content={source} />);

    expect(normalized).toBe(`$$\n${source}\n$$`);
    expect(container.querySelector(".katex-display")).not.toBeNull();
  });

  it("renders a translated LaTeX paper fragment as readable prose and math", () => {
    const source = [
      "The count follows a distribution~\\cite{maier2018medical}.",
      "\\emph{Electronic readout noise} is discussed in Section~\\ref{sec:analytic-reconstruction}.",
      "",
      "\\begin{equation}",
      "  \\label{eq:log-domain-variance}",
      "  \\mathrm{Var}[\\proj] \\approx \\frac{1}{\\mathbb{E}[N]} + \\frac{\\sigma_e^2}{\\mathbb{E}[N]^2}",
      "\\end{equation}",
    ].join("\n");
    const normalized = normalizeSelectionMath(source);
    const { container } = render(<SelectionResult content={source} />);

    expect(normalized).toContain("[Maier 2018]");
    expect(normalized).toContain("*Electronic readout noise*");
    expect(normalized).toContain("Section analytic reconstruction");
    expect(normalized).not.toContain("\\cite{");
    expect(normalized).not.toContain("\\ref{");
    expect(normalized).not.toContain("\\label{");
    expect(screen.getByText("Electronic readout noise")).toHaveProperty("tagName", "EM");
    expect(container.querySelector(".katex-display")).not.toBeNull();
    expect(container.querySelector(".katex-error")).toBeNull();
  });

  it("keeps common citation variants and nested emphasis readable", () => {
    const source = [
      "\\emph{Outer \\textbf{inner}}.",
      "\\citet{maier2018medical}; \\citep{smith2020ct};",
      "\\citeauthor{doe2021noise}; \\citeyear{doe2021noise}.",
    ].join(" ");
    const normalized = normalizeSelectionMath(source);
    const { container } = render(<SelectionResult content={source} />);

    expect(normalized).toContain("*Outer **inner***");
    expect(normalized).toContain("Maier (2018)");
    expect(normalized).toContain("[Smith 2020]");
    expect(normalized).toContain("Doe");
    expect(normalized).toContain("2021");
    expect(container.querySelector("em strong")).not.toBeNull();
  });

  it("renders source-defined math macros as readable fallback operators", () => {
    const { container } = render(
      <SelectionResult content={"$\\proj = -\\log(\\detint/\\srcint) + \\noisev$"} />,
    );

    expect(container.querySelector(".katex")).not.toBeNull();
    expect(container.querySelector(".katex-error")).toBeNull();
    expect(container.querySelector(".katex-html")).toHaveTextContent("proj");
    expect(container.querySelector(".katex-html")).toHaveTextContent("detint");
  });

  it("preserves native KaTeX commands and supports subscripted custom macros", () => {
    const source = "$\\boxed{\\frac{1}{2}} + \\left\\langle x,y\\right\\rangle + \\cov_{\\noisev}$";
    const { container } = render(<SelectionResult content={source} />);

    expect(container.querySelector(".katex-error")).toBeNull();
    expect(container.querySelector(".fbox")).not.toBeNull();
    expect(container.querySelector(".katex-html")).toHaveTextContent("cov");
    expect(container.querySelector(".katex-html")).toHaveTextContent("noisev");
  });

  it("does not override macros defined inside the source formula", () => {
    const source = "$\\newcommand{\\foo}[1]{#1^2}\\foo{x}$";
    const { container } = render(<SelectionResult content={source} />);

    expect(container.querySelector(".katex-error")).toBeNull();
    expect(container.querySelector(".katex-html")).toHaveTextContent("x2");
  });

  it("renders comma, space, and colon separated variables as inline math", () => {
    const source = "$x,y$ | $u, v$ | $x y$ | $A:B$";
    const { container } = render(<SelectionResult content={source} />);

    expect(normalizeSelectionMath(source)).toBe(source);
    expect(container.querySelectorAll(".katex")).toHaveLength(4);
    expect(container.querySelector(".katex-error")).toBeNull();
  });

  it("keeps ordinary dollar amounts out of math rendering", () => {
    const source = "Price is $5 and tax is $2; the expected range is $5–$10.";
    const normalized = normalizeSelectionMath(source);
    const { container } = render(<SelectionResult content={source} />);

    expect(normalized).toBe("Price is \\$5 and tax is \\$2; the expected range is \\$5–\\$10.");
    expect(container.querySelector(".katex")).toBeNull();
    expect(screen.getByText(source)).toBeVisible();
  });

  it("does not let a currency marker consume a later inline formula", () => {
    const source = "It costs $5 and variable $x$ is positive; energy is $E=mc^2$.";
    const normalized = normalizeSelectionMath(source);
    const { container } = render(<SelectionResult content={source} />);

    expect(normalized).toBe("It costs \\$5 and variable $x$ is positive; energy is $E=mc^2$.");
    expect(container.querySelectorAll(".katex")).toHaveLength(2);
    expect(screen.getByText(/It costs \$5 and variable/)).toBeVisible();
  });

  it("does not pair a price with formulas that follow punctuation", () => {
    const source = "It costs $5; variable:$x,y$ and pair $u, v$.";
    const normalized = normalizeSelectionMath(source);
    const { container } = render(<SelectionResult content={source} />);

    expect(normalized).toBe("It costs \\$5; variable:$x,y$ and pair $u, v$.");
    expect(container.querySelectorAll(".katex")).toHaveLength(2);
  });

  it("does not pair a Chinese price with a following formula", () => {
    const source = "价格是 $5；变量：$x$。";
    const normalized = normalizeSelectionMath(source);
    const { container } = render(<SelectionResult content={source} />);

    expect(normalized).toBe("价格是 \\$5；变量：$x$。");
    expect(container.querySelectorAll(".katex")).toHaveLength(1);
  });

  it("does not pair a price with a Unicode formula opener", () => {
    const source = "Price $5; formula $π$; vector $|x|$; root $√x$.";
    const normalized = normalizeSelectionMath(source);
    const { container } = render(<SelectionResult content={source} />);

    expect(normalized).toBe("Price \\$5; formula $π$; vector $|x|$; root $√x$.");
    expect(container.querySelectorAll(".katex")).toHaveLength(3);
  });

  it("keeps a closed numeric formula even when text follows without a space", () => {
    const source = "$5$-fold; $5$+x; $5$(x); $5$中文";
    const { container } = render(<SelectionResult content={source} />);

    expect(normalizeSelectionMath(source)).toBe(source);
    expect(container.querySelectorAll(".katex")).toHaveLength(4);
  });

  it("keeps punctuation and factorials inside closed numeric formulas", () => {
    const source = "There are $5!$ permutations; value $5,$ and ratio $5:$; variable $x$.";
    const { container } = render(<SelectionResult content={source} />);

    expect(normalizeSelectionMath(source)).toBe(source);
    expect(container.querySelectorAll(".katex")).toHaveLength(4);
  });

  it("keeps several prices literal while rendering a following formula", () => {
    const source = "Prices are $5, $10, and $5–$10; formula $A:B$.";
    const normalized = normalizeSelectionMath(source);
    const { container } = render(<SelectionResult content={source} />);

    expect(normalized).toBe("Prices are \\$5, \\$10, and \\$5–\\$10; formula $A:B$.");
    expect(container.querySelectorAll(".katex")).toHaveLength(1);
    expect(screen.getByText(/Prices are \$5, \$10, and \$5–\$10/)).toBeVisible();
  });

  it("treats a closed numeric expression as math and an unclosed amount as currency", () => {
    const source = "Index $5$ is math; price is $5.";
    const normalized = normalizeSelectionMath(source);
    const { container } = render(<SelectionResult content={source} />);

    expect(normalized).toBe("Index $5$ is math; price is \\$5.");
    expect(container.querySelectorAll(".katex")).toHaveLength(1);
  });

  it("keeps numeric medical units inside closed formulas", () => {
    const source = [
      "Dose was $5 \\mathrm{mGy}$; energy was $100 \\mathrm{keV}$.",
      "The compound unit was $5 \\mathrm{mGy\\,cm^{-1}}$.",
    ].join(" ");
    const { container } = render(<SelectionResult content={source} />);

    expect(normalizeSelectionMath(source)).toBe(source);
    expect(container.querySelectorAll(".katex")).toHaveLength(3);
    expect(container.querySelector(".katex-error")).toBeNull();
  });

  it("removes labels that were already inside explicit math delimiters", () => {
    const source = "$$\\label{eq:kept-out}x = 1$$";
    const normalized = normalizeSelectionMath(source);
    const { container } = render(<SelectionResult content={source} />);

    expect(normalized).toBe("$$\nx = 1\n$$");
    expect(container.querySelector(".katex-display")).not.toBeNull();
    expect(container.querySelector(".katex-error")).toBeNull();
  });

  it("converts alignat without leaking its column-count argument", () => {
    const source = "\\begin{alignat}{2}x &= 1 & y &= 2 \\\\ z &= 3 & w &= 4\\end{alignat}";
    const normalized = normalizeSelectionMath(source);
    const { container } = render(<SelectionResult content={source} />);

    expect(normalized).toContain("\\begin{aligned}");
    expect(normalized).not.toContain("{2}x");
    expect(container.querySelector(".katex-display")).not.toBeNull();
    expect(container.querySelector(".katex-error")).toBeNull();
  });

  it("does not turn unfinished or non-math environments into formulas", () => {
    const unfinished = "\\begin{equation}\nx = 1";
    const itemize = "\\begin{itemize}\n\\item One\n\\end{itemize}";

    expect(normalizeSelectionMath(unfinished)).toBe(unfinished);
    expect(normalizeSelectionMath(itemize)).toBe(itemize);
  });

  it("leaves an unfinished streamed formula visible until its delimiter closes", () => {
    const source = "Partial formula: $\\frac{1";
    const { container } = render(<SelectionResult content={source} />);

    expect(container.querySelector(".katex")).toBeNull();
    expect(screen.getByText(source)).toBeVisible();
  });

  it("renders a streamed formula once the closing delimiter arrives", () => {
    const partial = "Partial formula: $u, v";
    const { container, rerender } = render(<SelectionResult content={partial} />);

    expect(container.querySelector(".katex")).toBeNull();
    expect(screen.getByText(partial)).toBeVisible();

    rerender(<SelectionResult content={`${partial}$`} />);
    expect(container.querySelectorAll(".katex")).toHaveLength(1);
  });

  it("does not normalize LaTeX-looking text inside code spans or fences", () => {
    const source = "Code `$\\foo$` here.\n\n```tex\n$\\bar$\n```";
    const { container } = render(<SelectionResult content={source} />);

    expect(normalizeSelectionMath(source)).toBe(source);
    expect([...container.querySelectorAll("code")].map((code) => code.textContent)).toEqual([
      "$\\foo$",
      "$\\bar$\n",
    ]);
    expect(container.querySelector(".katex")).toBeNull();
  });

  it("does not interpret model-provided raw HTML", () => {
    const { container } = render(
      <SelectionResult content={'<img src=x onerror="alert(1)">'} />,
    );

    expect(container.querySelector("img")).toBeNull();
  });
});
