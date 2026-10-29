import { GraphicsEditor } from "../../components/GraphicsEditor";
import { useAppSettings } from "../../state/app";
import type { GameSettingsForm } from "./settingsForm";

export function GraphicsTab({ form }: { form: GameSettingsForm }) {
  const { appSettings } = useAppSettings();
  const draft = form.draft!;
  const runnerId = draft.runtime.runner?.id ?? appSettings?.defaultRunner?.id ?? null;
  return <GraphicsEditor value={draft.graphics} onChange={(graphics) => form.setDraft((d) => ({ ...d, graphics }))} errors={form.errors} runnerId={runnerId} errorPrefix="graphics" />;
}
