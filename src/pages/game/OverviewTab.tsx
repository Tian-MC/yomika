import { ExternalLink, FolderOpen, Heart, RefreshCw, Save, Trash } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useNavigate } from "react-router-dom";
import { Button } from "../../components/ui/Button";
import { Badge, EmptyState, ErrorState, KeyValueList, LoadingBlock, Section } from "../../components/ui/feedback";
import { SelectField, TextareaField } from "../../components/ui/form";
import { ConfirmDialog, Modal } from "../../components/ui/Modal";
import { VndbSearch } from "../../components/VndbSearch";
import type { Game, PlayStatus, VndbSearchResult } from "../../contract";
import { useI18n } from "../../i18n";
import { call, isTauri } from "../../lib/api";
import { openInFileManager } from "../../lib/dialogs";
import { formatDate, formatDateTime, formatDuration, formatRating, formatRelative, hasCjk, stripBbcode } from "../../lib/format";
import { useCommand } from "../../lib/hooks";
import { useAppSettings, useLibrary } from "../../state/app";
import { useToasts } from "../../state/toasts";
import { ALL_STATUSES } from "../library/filter";

export function OverviewTab({ game, onGameUpdated }: { game: Game; onGameUpdated: (g: Game) => void }) {
  const { t, lang } = useI18n();
  const toasts = useToasts();
  const navigate = useNavigate();
  const library = useLibrary();
  const { appSettings } = useAppSettings();
  const [notes, setNotes] = useState(game.notes ?? "");
  const [notesBase, setNotesBase] = useState(game.notes);
  if (notesBase !== game.notes) {
    setNotesBase(game.notes);
    setNotes(game.notes ?? "");
  }
  const [savingNotes, setSavingNotes] = useState(false);
  const [matchOpen, setMatchOpen] = useState(false);
  const [match, setMatch] = useState<VndbSearchResult | null>(null);
  const [applying, setApplying] = useState(false);
  const [removeOpen, setRemoveOpen] = useState(false);
  const [removing, setRemoving] = useState(false);
  // Refetch history whenever playtime changes (a session ended).
  const plays = useCommand("library_play_sessions", { gameId: game.id });
  const reloadPlays = plays.reload;
  const lastPlaytime = useRef(game.playtimeSeconds);
  useEffect(() => {
    if (lastPlaytime.current !== game.playtimeSeconds) {
      lastPlaytime.current = game.playtimeSeconds;
      void reloadPlays();
    }
  }, [game.playtimeSeconds, reloadPlays]);

  const update = async (patch: Partial<Game>, what: string) => {
    try {
      const updated = await call("library_update_game", { game: { ...game, ...patch } });
      onGameUpdated(updated);
      return true;
    } catch (e) {
      toasts.error(e, t("game.updateFailed", { what }));
      return false;
    }
  };

  const applyMatch = async () => {
    if (!match) return;
    setApplying(true);
    try {
      const updated = await call("vndb_apply", { gameId: game.id, vndbId: match.id });
      onGameUpdated(updated);
      toasts.success(t("vndb.applied", { title: match.title }));
      setMatchOpen(false);
    } catch (e) {
      toasts.error(e, t("vndb.applyFailed"));
    } finally {
      setApplying(false);
    }
  };

  const remove = async () => {
    setRemoving(true);
    try {
      await call("library_remove_game", { id: game.id });
      library.removeLocal(game.id);
      toasts.success(t("game.removed", { title: game.title }));
      void navigate("/");
    } catch (e) {
      toasts.error(e, t("game.removeFailed"));
      setRemoving(false);
    }
  };

  const openFolder = async () => {
    try {
      if (!(await openInFileManager(game.installDir))) toasts.info(t("demo.noFileManager"), game.installDir);
    } catch (e) {
      toasts.error(e, t("game.openFolderFailed"));
    }
  };

  const openVndb = async () => {
    const id = game.metadata.vndbId;
    if (!id) return;
    const url = `https://vndb.org/${id}`;
    if (isTauri()) {
      try {
        await openUrl(url);
      } catch (e) {
        toasts.error(e);
      }
    } else window.open(url, "_blank", "noopener");
  };

  const m = game.metadata;
  const sessions = plays.data ?? [];
  const avg = sessions.length ? sessions.reduce((a, s) => a + s.durationSeconds, 0) / sessions.length : 0;

  return (
    <div className="overview-grid">
      <div className="stack gap-lg">
        <Section
          title={t("game.about")}
          actions={
            <Button size="sm" variant="ghost" icon={RefreshCw} onClick={() => setMatchOpen(true)} disabled={appSettings?.vndb.enabled === false} title={appSettings?.vndb.enabled === false ? t("vndb.disabled") : undefined}>
              {m.vndbId ? t("vndb.rematch") : t("vndb.match")}
            </Button>
          }
        >
          {m.description ? (
            <p className="description" lang={hasCjk(m.description) ? "ja" : undefined}>
              {stripBbcode(m.description)}
            </p>
          ) : (
            <p className="muted">{m.vndbId ? t("game.noDescription") : t("game.noMetadata")}</p>
          )}
          {m.tags.length > 0 && (
            <ul className="tag-list" aria-label={t("game.tags")}>
              {m.tags.map((tag) => (
                <li key={tag} className="tag">
                  {tag}
                </li>
              ))}
            </ul>
          )}
          <KeyValueList
            items={[
              [t("game.developers"), m.developers.length ? m.developers.join(", ") : "—"],
              [t("game.released"), m.releaseDate ?? "—"],
              [t("game.rating"), m.rating !== null ? `${formatRating(m.rating)} / 10` : "—"],
              [t("game.length"), m.lengthMinutes ? `~${formatDuration(m.lengthMinutes * 60, lang)}` : "—"],
              ...(m.aliases.length ? ([[t("game.aliases"), <span key="a" lang="ja">{m.aliases.join(" / ")}</span>]] as Array<[string, ReactNode]>) : []),
              [
                "VNDB",
                m.vndbId ? (
                  <button type="button" className="link-btn" onClick={() => void openVndb()}>
                    {m.vndbId} <ExternalLink size={12} aria-hidden />
                  </button>
                ) : (
                  t("game.notMatched")
                ),
              ],
            ]}
          />
        </Section>

        <Section title={t("game.files")}>
          <KeyValueList
            items={[
              [t("game.installDir"), <code key="d">{game.installDir}</code>],
              [t("game.exe"), <code key="e">{game.exePath}</code>],
              [t("game.added"), formatDate(game.addedAt, lang)],
            ]}
          />
          <div className="row gap-sm wrap">
            <Button size="sm" icon={FolderOpen} onClick={() => void openFolder()}>
              {t("game.openFolder")}
            </Button>
            <Button size="sm" variant="danger" icon={Trash} onClick={() => setRemoveOpen(true)}>
              {t("game.remove")}
            </Button>
          </div>
        </Section>
      </div>

      <div className="stack gap-lg">
        <Section title={t("game.yourProgress")}>
          <div className="grid-2">
            <SelectField label={t("library.status")} value={game.status} onChange={(v) => void update({ status: v as PlayStatus }, t("library.status"))} options={ALL_STATUSES.map((s) => ({ value: s, label: t(`status.${s}`) }))} />
            <div className="field">
              <span className="field-label">{t("library.favorite")}</span>
              <Button variant={game.favorite ? "primary" : "secondary"} icon={Heart} aria-pressed={game.favorite} onClick={() => void update({ favorite: !game.favorite }, t("library.favorite"))}>
                {game.favorite ? t("game.inFavorites") : t("game.addFavorite")}
              </Button>
            </div>
          </div>
          <TextareaField label={t("game.notes")} value={notes} onChange={setNotes} rows={4} placeholder={t("game.notesPlaceholder")} />
          <div className="row gap-sm">
            <Button
              size="sm"
              icon={Save}
              loading={savingNotes}
              disabled={(notes.trim() || null) === (game.notes?.trim() || null)}
              onClick={async () => {
                setSavingNotes(true);
                if (await update({ notes: notes.trim() ? notes : null }, t("game.notes"))) toasts.success(t("game.notesSaved"));
                setSavingNotes(false);
              }}
            >
              {t("game.saveNotes")}
            </Button>
          </div>
        </Section>

        <Section title={t("game.playtime")}>
          <div className="stat-row">
            <div className="stat">
              <span className="stat-value">{formatDuration(game.playtimeSeconds, lang)}</span>
              <span className="stat-label">{t("game.totalPlaytime")}</span>
            </div>
            <div className="stat">
              <span className="stat-value">{game.lastPlayedAt ? formatRelative(game.lastPlayedAt, lang) : t("library.never")}</span>
              <span className="stat-label">{t("library.col.lastPlayed")}</span>
            </div>
            <div className="stat">
              <span className="stat-value">{sessions.length}</span>
              <span className="stat-label">{t("game.sessions")}</span>
            </div>
            <div className="stat">
              <span className="stat-value">{sessions.length ? formatDuration(avg, lang) : "—"}</span>
              <span className="stat-label">{t("game.avgSession")}</span>
            </div>
          </div>
          {plays.error ? (
            <ErrorState error={plays.error} compact onRetry={() => void plays.reload()} />
          ) : plays.loading ? (
            <LoadingBlock />
          ) : sessions.length === 0 ? (
            <EmptyState title={t("game.noSessions")} />
          ) : (
            <table className="table compact">
              <caption className="sr-only">{t("game.sessionHistory")}</caption>
              <thead>
                <tr>
                  <th>{t("game.sessionStarted")}</th>
                  <th className="num">{t("game.sessionDuration")}</th>
                </tr>
              </thead>
              <tbody>
                {sessions.slice(0, 30).map((s) => (
                  <tr key={s.id}>
                    <td>{formatDateTime(s.startedAt, lang)}</td>
                    <td className="num">{formatDuration(s.durationSeconds, lang)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </Section>
      </div>

      <Modal
        open={matchOpen}
        onClose={() => setMatchOpen(false)}
        title={t("vndb.matchTitle", { title: game.title })}
        size="lg"
        footer={
          <>
            <Button variant="ghost" onClick={() => setMatchOpen(false)}>
              {t("common.cancel")}
            </Button>
            <Button variant="primary" onClick={() => void applyMatch()} disabled={!match} loading={applying}>
              {t("vndb.apply")}
            </Button>
          </>
        }
      >
        <p className="muted">{t("vndb.matchHint")}</p>
        <VndbSearch initialQuery={game.originalTitle || game.title} selectedId={match?.id ?? m.vndbId} onSelect={setMatch} />
        {m.vndbId && <Badge tone="info">{t("vndb.currentMatch", { id: m.vndbId })}</Badge>}
      </Modal>

      <ConfirmDialog open={removeOpen} title={t("game.removeTitle", { title: game.title })} confirmLabel={t("game.remove")} variant="danger" busy={removing} onConfirm={() => void remove()} onCancel={() => setRemoveOpen(false)}>
        <p>{t("game.removeText")}</p>
      </ConfirmDialog>
    </div>
  );
}
