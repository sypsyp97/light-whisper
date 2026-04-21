import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Modal } from "@/components/ui/Modal";

describe("Modal", () => {
  it("does not render content when open is false", () => {
    render(
      <Modal open={false} onClose={vi.fn()} title="Hello" data-testid="modal-correction-rules">
        <p>Body</p>
      </Modal>,
    );
    expect(screen.queryByText("Body")).not.toBeInTheDocument();
  });

  it("renders title as h2 and body when open", () => {
    render(
      <Modal open onClose={vi.fn()} title="Correction Rules" data-testid="modal-correction-rules">
        <p>Body</p>
      </Modal>,
    );
    expect(screen.getByRole("heading", { level: 2, name: "Correction Rules" })).toBeInTheDocument();
    expect(screen.getByText("Body")).toBeInTheDocument();
  });

  it("closes on Escape", async () => {
    const onClose = vi.fn();
    render(
      <Modal open onClose={onClose} title="Title" data-testid="modal-correction-rules">
        <p>Body</p>
      </Modal>,
    );
    await userEvent.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalled();
  });

  it("exposes the passed data-testid on the modal root", () => {
    render(
      <Modal open onClose={vi.fn()} title="Title" data-testid="modal-add-provider">
        <p>Body</p>
      </Modal>,
    );
    expect(screen.getByTestId("modal-add-provider")).toBeInTheDocument();
  });
});
