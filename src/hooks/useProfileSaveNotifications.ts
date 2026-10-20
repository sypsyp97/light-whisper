import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { getProfileSaveStatus, retryProfileSave, type ProfileSaveStatus } from "@/api/tauri";
import i18n from "@/i18n";

const SAVE_ERROR_TOAST = "profile-save-error";

export function useProfileSaveNotifications(): void {
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    let revision = -1;
    let retrying = false;

    const showError = () => {
      toast.error(i18n.t("toast.profileSaveFailed"), {
        id: SAVE_ERROR_TOAST,
        duration: Infinity,
        dismissible: false,
        action: {
          label: i18n.t("common.retry"),
          onClick: (event) => {
            // Sonner otherwise dismisses the error before the write has completed.
            event.preventDefault();
            if (disposed || retrying) return;
            retrying = true;
            const retryRevision = revision;
            return retryProfileSave().catch(() => {
              if (!disposed && revision === retryRevision) showError();
            }).finally(() => { retrying = false; });
          },
        },
      });
    };
    const applyStatus = (status: ProfileSaveStatus) => {
      if (disposed || status.revision <= revision) return;
      revision = status.revision;
      if (status.state === "error") showError();
      else if (status.state === "saved") toast.dismiss(SAVE_ERROR_TOAST);
    };

    void (async () => {
      try {
        const stop = await listen<ProfileSaveStatus>("profile-save-status", ({ payload }) => {
          applyStatus(payload);
        });
        if (disposed) {
          stop();
          return;
        }
        unlisten = stop;
      } catch (error) {
        console.error("Profile save status subscription failed:", error);
      }
      if (disposed) return;
      try {
        applyStatus(await getProfileSaveStatus());
      } catch (error) {
        if (!disposed) console.error("Profile save status unavailable:", error);
      }
    })();
    return () => {
      disposed = true;
      unlisten?.();
      toast.dismiss(SAVE_ERROR_TOAST);
    };
  }, []);
}
