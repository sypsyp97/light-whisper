import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "@/i18n";
import type { ModelStage } from "@/hooks/useModelStatus";
import StatusIndicator from "./StatusIndicator";

beforeEach(async () => {
  await i18n.changeLanguage("en");
});

function renderStatus(stage: ModelStage) {
  const downloadModels = vi.fn();
  const cancelDownload = vi.fn();
  return {
    ...render(<StatusIndicator
      stage={stage}
      isReady={false}
      isStarting={false}
      isRecording={false}
      isProcessing={false}
      device="cpu"
      gpuName={null}
      downloadProgress={0}
      downloadMessage={null}
      isDownloading={false}
      downloadModels={downloadModels}
      cancelDownload={cancelDownload}
    />),
    downloadModels,
    cancelDownload,
  };
}


const NEED_REPAIR = "need_repair" as ModelStage;

describe("StatusIndicator model integrity repair", () => {
  it("offers an enabled, accessible Repair action that starts the supplied repair download", () => {
    const { downloadModels, cancelDownload } = renderStatus(NEED_REPAIR);
    const repair = screen.getByRole("button", { name: /repair/i });

    expect(repair).toBeEnabled();
    fireEvent.click(repair);

    expect(downloadModels).toHaveBeenCalledOnce();
    expect(cancelDownload).not.toHaveBeenCalled();
  });

  it("explains repair without a progress bar, spinner, or loading/preparing wording", () => {
    const { container } = renderStatus(NEED_REPAIR);

    expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
    expect(container.querySelectorAll(".animate-spin")).toHaveLength(0);
    expect(screen.queryAllByText(/loading|preparing/i)).toHaveLength(0);
    expect(container.querySelector("[aria-live='polite']"))
      .toHaveTextContent(/repair|corrupt|damaged/i);
  });

  it("keeps the existing missing-model download action", () => {
    const { downloadModels } = renderStatus("need_download");
    fireEvent.click(screen.getByRole("button", { name: /start download/i }));

    expect(downloadModels).toHaveBeenCalledOnce();
    expect(screen.queryByRole("button", { name: /repair/i })).not.toBeInTheDocument();
  });
});
