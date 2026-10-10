import type { VersionChanges } from "@gitto/system/types";
import { createSignal } from "solid-js";

import { rpc } from "@/lib/rpc";

/** What the What's new dialog shows; `null` while it's closed. */
export interface WhatsNewShown {
  versions: VersionChanges[];
  /** Whether it's what changed since the user last saw, or every version, as asked for. */
  unseen: boolean;
}

const [shown, setShown] = createSignal<WhatsNewShown | null>(null);

/** The What's new dialog, opened after an update, or from the app's version in the footer. */
export const whatsNew = {
  shown,

  /** Opens it with what changed since the last version the user saw, if anything did. */
  async showUnseen() {
    try {
      const versions = await rpc.system.changelog.unseen();
      if (versions.length) setShown({ versions, unseen: true });
    } catch (error) {
      console.error("Couldn't read what's new", error);
    }
  },

  /** Opens it with every version's changes. */
  async showAll() {
    try {
      setShown({ versions: await rpc.system.changelog.all(), unseen: false });
    } catch (error) {
      console.error("Couldn't read what's new", error);
    }
  },

  /** Closes it, and remembers this version's changes as seen. */
  close() {
    setShown(null);
    rpc.system.changelog.seen().catch((error: unknown) => {
      console.error("Couldn't remember what's new as seen", error);
    });
  },
};
