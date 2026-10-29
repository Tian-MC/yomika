import { RefreshCw } from "lucide-react";
import { useState } from "react";
import { LogViewer } from "../../components/LogViewer";
import { runnerLabel } from "../../components/pickers";
import { Button } from "../../components/ui/Button";
import { Badge, Callout, CopyButton, ErrorState, KeyValueList, LoadingBlock, Section } from "../../components/ui/feedback";
import type { Game } from "../../contract";
import { useI18n } from "../../i18n";
import { formatDateTime } from "../../lib/format";
import { useCommand } from "../../lib/hooks";
import { useSessions } from "../../state/app";

export function LaunchTab({ game, settingsDirty }: { game: Game; settingsDirty: boolean }) {
  const { t, lang } = useI18n();
  const preview = useCommand("session_preview", { gameId: game.id });
  const { sessions } = useSessions();
  const gameSessions = sessions.filter((s) => s.gameId === game.id);
  const [picked, setPicked] = useState<string | null>(null);
  const sessionId = picked ?? gameSessions[0]?.id ?? null;
  const p = preview.data;

  return (
    <div className="stack gap-lg">
      <Section
        title={t("launch.preview")}
        description={t("launch.previewDesc")}
        actions={
          <Button size="sm" variant="ghost" icon={RefreshCw} onClick={() => void preview.reload()} loading={preview.refreshing}>
            {t("common.refresh")}
          </Button>
        }
      >
        {settingsDirty && <Callout tone="info">{t("launch.unsavedNote")}</Callout>}
        {preview.error && <ErrorState error={preview.error} onRetry={() => void preview.reload()} />}
        {preview.loading && <LoadingBlock />}
        {p && (
          <div className="stack gap-md">
            {p.warnings.map((w) => (
              <Callout key={w} tone="warning">
                {w}
              </Callout>
            ))}
            <KeyValueList
              items={[
                [t("compat.runner"), <span key="r">{runnerLabel(p.runner)} <Badge>{t(`runner.kind.${p.runner.kind}`)}</Badge> <code className="small">{p.runner.path}</code></span>],
                [t("launch.prefix"), <span key="p">{p.prefix.name} · {p.prefix.arch} {!p.prefix.initialized && <Badge tone="warning">{t("prefix.notInitialized")}</Badge>} <code className="small">{p.prefix.path}</code></span>],
                [t("compat.workingDir"), <code key="c">{p.command.cwd ?? "—"}</code>],
              ]}
            />
            <div>
              <div className="row space-between">
                <span className="field-label">{t("launch.finalCommand")}</span>
                <CopyButton text={p.preview} label={t("launch.copyCommand")} />
              </div>
              <pre className="code-block wrap" data-testid="launch-preview">{p.preview}</pre>
            </div>
            <details>
              <summary>{t("launch.argv", { n: p.command.args.length + 1 })}</summary>
              <ol className="argv-list mono">
                <li>{p.command.program}</li>
                {p.command.args.map((a, i) => (
                  <li key={i}>{a}</li>
                ))}
              </ol>
            </details>
            {Object.keys(p.command.env).length > 0 && (
              <details open>
                <summary>{t("launch.env", { n: Object.keys(p.command.env).length })}</summary>
                <table className="table compact mono">
                  <tbody>
                    {Object.entries(p.command.env).map(([k, v]) => (
                      <tr key={k}>
                        <th scope="row">{k}</th>
                        <td className="break">{v}</td>
                      </tr>
                    ))}
                    {p.command.envRemove.map((k) => (
                      <tr key={`rm-${k}`}>
                        <th scope="row">{k}</th>
                        <td className="muted">{t("launch.unset")}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </details>
            )}
            {p.companions.length > 0 && (
              <div>
                <span className="field-label">{t("launch.companions")}</span>
                {p.companions.map((c, i) => (
                  <pre className="code-block wrap" key={i}>
                    {[c.program, ...c.args].join(" ")}
                  </pre>
                ))}
              </div>
            )}
          </div>
        )}
      </Section>

      <Section title={t("launch.logs")} description={t("launch.logsDesc")}>
        {gameSessions.length === 0 ? (
          <p className="muted">{t("launch.noSessions")}</p>
        ) : (
          <>
            <label className="compact-field">
              <span>{t("logs.session")}</span>
              <select className="input select input-sm" value={sessionId ?? ""} onChange={(e) => setPicked(e.target.value)}>
                {gameSessions.map((s) => (
                  <option key={s.id} value={s.id}>
                    {formatDateTime(s.startedAt, lang)} · {t(`session.state.${s.state}`)}
                    {s.exitCode !== null ? ` · ${t("launch.exitCode", { code: s.exitCode })}` : ""}
                  </option>
                ))}
              </select>
            </label>
            {sessionId && <LogViewer key={sessionId} sessionId={sessionId} height={360} />}
          </>
        )}
      </Section>
    </div>
  );
}
