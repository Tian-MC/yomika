import { Play, Skull, Square } from "lucide-react";
import { useState } from "react";
import type { Game } from "../contract";
import { useT } from "../i18n";
import { call } from "../lib/api";
import { useSessions } from "../state/app";
import { useToasts } from "../state/toasts";
import { Button, IconButton } from "./ui/Button";

/** Play / Stop / Force stop with live session state. */
export function PlayControls({ game, size = "md", compact }: { game: Game; size?: "sm" | "md" | "lg"; compact?: boolean }) {
  const t = useT();
  const toasts = useToasts();
  const { activeFor, upsert } = useSessions();
  const session = activeFor(game.id);
  const [busy, setBusy] = useState(false);

  const play = async () => {
    setBusy(true);
    try {
      const s = await call("session_launch", { gameId: game.id });
      upsert(s);
    } catch (e) {
      toasts.error(e, t("session.launchFailed", { title: game.title }));
    } finally {
      setBusy(false);
    }
  };

  const stop = async (force: boolean) => {
    if (!session) return;
    try {
      await call("session_stop", { sessionId: session.id, force });
    } catch (e) {
      toasts.error(e, t("session.stopFailed"));
    }
  };

  if (!session) {
    if (compact) return <IconButton icon={Play} variant="primary" label={t("session.playTitle", { title: game.title })} onClick={() => void play()} disabled={busy} className="play-fab" />;
    return (
      <Button variant="primary" size={size} icon={Play} loading={busy} onClick={() => void play()}>
        {t("session.play")}
      </Button>
    );
  }

  if (compact) {
    return <IconButton icon={Square} variant="danger" label={t("session.stopTitle", { title: game.title })} onClick={() => void stop(false)} disabled={session.state === "stopping"} className="play-fab" />;
  }

  return (
    <div className="row gap-sm">
      <Button variant="danger" size={size} icon={Square} onClick={() => void stop(false)} loading={session.state === "stopping"} disabled={session.state === "stopping"}>
        {session.state === "starting" ? t("session.state.starting") : session.state === "stopping" ? t("session.state.stopping") : t("session.stop")}
      </Button>
      <Button variant="ghost" size={size} icon={Skull} onClick={() => void stop(true)} title={t("session.forceStopHint")}>
        {t("session.forceStop")}
      </Button>
    </div>
  );
}
