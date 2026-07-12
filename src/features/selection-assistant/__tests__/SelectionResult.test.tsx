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
          "| Item | Value |",
          "| --- | --- |",
          "| Alpha | 42 |",
        ].join("\n")}
      />,
    );

    expect(screen.getByRole("heading", { level: 2, name: "Summary" })).toBeVisible();
    expect(screen.getByText("Important")).toHaveProperty("tagName", "STRONG");
    expect(container.querySelector("table")).not.toBeNull();
  });

  it("renders dollar-delimited inline and display formulas with KaTeX", () => {
    const { container } = render(
      <SelectionResult content={"Inline $E = mc^2$\n\n$$\n\\int_0^1 x^2 dx\n$$"} />,
    );

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

  it("does not normalize LaTeX-looking text inside code spans or fences", () => {
    const source = "`\\(x\\)`\n\n```tex\n\\[y\\]\n```";
    expect(normalizeSelectionMath(source)).toBe(source);
  });

  it("does not interpret model-provided raw HTML", () => {
    const { container } = render(
      <SelectionResult content={'<img src=x onerror="alert(1)">'} />,
    );

    expect(container.querySelector("img")).toBeNull();
  });
});
