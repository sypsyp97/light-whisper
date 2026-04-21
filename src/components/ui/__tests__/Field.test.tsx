import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { Field } from "@/components/ui/Field";

describe("Field", () => {
  it("renders label text", () => {
    render(
      <Field label="Microphone">
        <input />
      </Field>,
    );
    expect(screen.getByText("Microphone")).toBeInTheDocument();
  });

  it("renders hint text when provided", () => {
    render(
      <Field label="Microphone" hint="Used for recording only">
        <input />
      </Field>,
    );
    expect(screen.getByText("Used for recording only")).toBeInTheDocument();
  });

  it("renders error text when provided", () => {
    render(
      <Field label="Microphone" error="Device unavailable">
        <input />
      </Field>,
    );
    expect(screen.getByText("Device unavailable")).toBeInTheDocument();
  });

  it("forwards data-testid", () => {
    render(
      <Field label="Microphone" data-testid="ui-field">
        <input />
      </Field>,
    );
    expect(screen.getByTestId("ui-field")).toBeInTheDocument();
  });
});
