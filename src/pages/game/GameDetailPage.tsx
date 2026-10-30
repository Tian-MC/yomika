import { ArrowLeft, BookOpen, Gamepad2, HardDrive, Languages, MonitorPlay, Save, TerminalSquare, Undo2, Wine } from "lucide-react";
import { useEffect } from "react";
import { Link, useBlocker, useParams, useSearchParams } from "react-router-dom";
import { BlurredImage } from "../../components/BlurredImage";
import { Cover } from "../../components/Cover";
import { PlayControls } from "../../components/PlayControls";
import { Button } from "../../components/ui/Button";
import { Badge, ErrorState, LoadingBlock } from "../../components/ui/feedback";
import { ConfirmDialog } from "../../components/ui/Modal";
import { Tabs, tabPanelProps } from "../../components/ui/Tabs";
import type { Game } from "../../contract";
import { useI18n } from "../../i18n";
import { coverSrc, shouldBlur } from "../../lib/cover";
import { formatElapsed, hasCjk } from "../../lib/format";
import { useCommand, useNow } from "../../lib/hooks";
import { useAppSettings, useLibrary, useSessions } from "../../state/app";
import { useToasts } from "../../state/toasts";
import { CompatibilityTab } from "./CompatibilityTab";
import { GraphicsTab } from "./GraphicsTab";
import { LaunchTab } from "./LaunchTab";
import { OverviewTab } from "./OverviewTab";
import { SavesTab } from "./SavesTab";
import { useGameSettingsForm } from "./settingsForm";
import { TranslationTab } from "./TranslationTab";

export type GameTab = "overview" | "compatibility" | "graphics" | "translation" | "saves" | "launch";
const TABS: GameTab[] = ["overview", "compatibility", "graphics", "translation", "saves", "launch"];
const SETTINGS_TABS: GameTab[] = ["compatibility", "graphics", "translation"];

export function GameDetailPage() {
  const { id } = useParams();
  const gameId = Number(id);
  if (!Number.isInteger(gameId) || gameId <= 0) return <InvalidGame />;
  return <GameDetail key={gameId} gameId={gameId} />;
}

function InvalidGame() {
  const { t } = useI18n();
  return (
    <div className="page">
      <ErrorState error={{ code: "notFound", message: t("game.invalidId"), details: null }} />
    </div>
  );
}

function GameDetail({ gameId }: { gameId: number }) {
  const { t } = useI18n();
  const toasts = useToasts();
  const library = useLibrary();
  const { appSettings } = useAppSettings();
  const { activeFor } = useSessions();
  const gameQ = useCommand("library_get_game", { id: gameId });
  const game = gameQ.data;
  const [params, setParams] = useSearchParams();
  const tabParam = params.get("tab") as GameTab | null;
  const tab: GameTab = tabParam && TABS.includes(tabParam) ? tabParam : "overview";
  const setTab = (v: GameTab) => {
    const next = new URLSearchParams(params);
    if (v === "overview") next.delete("tab");
    else next.set("tab", v);
    setParams(next, { replace: true });
  };

  const onGameUpdated = (g: Game) => {
    gameQ.mutate(g);
    library.upsert(g);
  };

  const form = useGameSettingsForm(gameId, game, onGameUpdated, t);
  const session = activeFor(gameId);
  const now = useNow(1000, session?.state === "running");

  // Keep this page in sync when a session ends (playtime/last played changed).
  const libGame = library.byId(gameId);
  useEffect(() => {
    if (libGame && game && (libGame.playtimeSeconds !== game.playtimeSeconds || libGame.lastPlayedAt !== game.lastPlayedAt)) {
      gameQ.mutate((g) => (g ? { ...g, playtimeSeconds: libGame.playtimeSeconds, lastPlayedAt: libGame.lastPlayedAt, status: libGame.status } : libGame));
    }
  }, [libGame, game, gameQ]);

  const doSave = async () => {
    if (Object.keys(form.errors).length > 0) {
      toasts.push({ kind: "warning", title: t("form.fixErrors", { n: Object.keys(form.errors).length }) });
      return false;
    }
    const ok = await form.save();
    if (ok) toasts.success(t("form.saved"));
    return ok;
  };

  const blocker = useBlocker(({ currentLocation, nextLocation }) => form.dirty && currentLocation.pathname !== nextLocation.pathname);

  useEffect(() => {
    if (!form.dirty) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "s" && (e.ctrlKey || e.metaKey)) {
        e.preventDefault();
        void doSave();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });


  if (gameQ.error && !game) {
    return (
      <div className="page">
        <BackLink />
        <ErrorState error={gameQ.error} onRetry={() => void gameQ.reload()} />
      </div>
    );
  }
  if (!game) return <LoadingBlock />;

  const errorCount = (prefixes: string[]) => Object.keys(form.errors).filter((k) => prefixes.some((p) => k.startsWith(p))).length;
  const tabErrors: Partial<Record<GameTab, number>> = {
    compatibility: errorCount(["runtime.", "files."]),
    graphics: errorCount(["graphics."]),
    translation: errorCount(["translation."]),
  };
  const icons = { overview: BookOpen, compatibility: Wine, graphics: MonitorPlay, translation: Languages, saves: HardDrive, launch: TerminalSquare } as const;
  const backdrop = coverSrc(game.metadata);
  const blurBackdrop = shouldBlur(game.metadata.coverSexual, appSettings?.vndb.blurSexualThreshold);
  const base = `game-${gameId}`;

  return (
    <div className="page game-page">
      <div className="hero">
        {backdrop && <div className={blurBackdrop ? "hero-backdrop is-blurred" : "hero-backdrop"} aria-hidden>
          <BlurredImage key={backdrop} src={backdrop} blur={blurBackdrop ? 70 : 40} saturation={blurBackdrop ? 0.9 : 1} eager />
        </div>}
        <div className="hero-inner">
          <BackLink />
          <div className="hero-content">
            <Cover title={game.title} originalTitle={game.originalTitle} metadata={game.metadata} size="lg" revealable eager />
            <div className="hero-text">
              <h1 className="hero-title">{game.title}</h1>
              {game.originalTitle && game.originalTitle !== game.title && (
                <p className="hero-orig" lang={hasCjk(game.originalTitle) ? "ja" : undefined}>
                  {game.originalTitle}
                </p>
              )}
              {game.metadata.developers.length > 0 && <p className="hero-dev">{game.metadata.developers.join(" · ")}</p>}
              <div className="hero-actions">
                <PlayControls game={game} size="lg" />
                {session && (
                  <Badge tone={session.state === "running" ? "success" : "warning"} icon={Gamepad2}>
                    {session.state === "running" ? t("session.runningFor", { time: formatElapsed(now / 1000 - session.startedAt) }) : t(`session.state.${session.state}`)}
                  </Badge>
                )}
              </div>
            </div>
          </div>
        </div>
      </div>

      <div className="game-tabs-wrap">
        <Tabs<GameTab>
          label={t("game.sections")}
          idBase={base}
          value={tab}
          onChange={setTab}
          tabs={TABS.map((v) => ({
            value: v,
            label: t(`game.tab.${v}`),
            icon: icons[v],
            disabled: v === "translation",
            badge: tabErrors[v] ? (
              <span className="tab-badge error" aria-label={t("form.errorsInTab", { n: tabErrors[v] ?? 0 })}>
                {tabErrors[v]}
              </span>
            ) : form.dirty && SETTINGS_TABS.includes(v) ? (
              <span className="tab-dot" aria-hidden />
            ) : null,
          }))}
        />
      </div>

      <div className="game-tab-panel" {...tabPanelProps(base, tab)}>
        {tab === "overview" && <OverviewTab game={game} onGameUpdated={onGameUpdated} />}
        {tab === "saves" && <SavesTab game={game} />}
        {tab === "launch" && <LaunchTab game={game} settingsDirty={form.dirty} />}
        {SETTINGS_TABS.includes(tab) &&
          (form.loadError && !form.draft ? (
            <ErrorState error={form.loadError} onRetry={() => void form.reload()} />
          ) : !form.draft ? (
            <LoadingBlock />
          ) : tab === "compatibility" ? (
            <CompatibilityTab game={game} form={form} />
          ) : tab === "graphics" ? (
            <GraphicsTab form={form} />
          ) : (
            <TranslationTab game={game} form={form} />
          ))}
      </div>

      {(form.dirty || form.saving) && (
        <div className="save-bar" role="region" aria-label={t("form.unsavedRegion")}>
          <div className="save-bar-text">
            <strong>{t("form.unsaved")}</strong>
            {Object.keys(form.errors).length > 0 && <span className="text-danger">{t("form.problems", { n: Object.keys(form.errors).length })}</span>}
            {form.saveError && <span className="text-danger">{form.saveError.message}</span>}
          </div>
          <div className="row gap-sm">
            <Button variant="ghost" icon={Undo2} onClick={form.revert} disabled={form.saving}>
              {t("form.revert")}
            </Button>
            <Button variant="primary" icon={Save} onClick={() => void doSave()} loading={form.saving} disabled={Object.keys(form.errors).length > 0}>
              {form.saving ? t("form.saving") : t("form.save")}
            </Button>
          </div>
        </div>
      )}

      <ConfirmDialog
        open={blocker.state === "blocked"}
        title={t("form.leaveTitle")}
        confirmLabel={t("form.leaveDiscard")}
        variant="danger"
        onConfirm={() => blocker.proceed?.()}
        onCancel={() => blocker.reset?.()}
      >
        <p>{t("form.leaveText")}</p>
      </ConfirmDialog>
    </div>
  );
}

function BackLink() {
  const { t } = useI18n();
  return (
    <Link to="/" className="back-link">
      <ArrowLeft size={16} aria-hidden /> {t("nav.library")}
    </Link>
  );
}
