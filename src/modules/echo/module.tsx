import type { ModuleContribution, ModulePageProps } from "../../shared/contracts";
import type { EchoService } from "../../shared/contracts";
import { EchoView } from "./EchoView";
import { getEchoPort } from "./tauri";

function EchoPage(_props: ModulePageProps) {
  return <EchoView port={getEchoPort()} />;
}

export const echoContribution: ModuleContribution = {
  id: "echo",
  label: "Echo",
  description: "Historique local du presse-papiers",
  icon: "▤",
  Page: EchoPage,
};

/** Restricted cross-module interface; it returns previews only, never raw clipboard contents. */
export const echoService: EchoService = {
  async search(query, options) {
    return getEchoPort().search(query, options);
  },
  copy: id => getEchoPort().copyEntry(id),
  async status() {
    const snapshot = await getEchoPort().getSnapshot();
    return {
      enabled: snapshot.available && snapshot.preferences.monitoringEnabled,
      paused: !snapshot.preferences.monitoringEnabled,
      itemCount: snapshot.entries.length,
    };
  },
};
