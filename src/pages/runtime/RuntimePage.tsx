import { FolderInput, FolderOpen, Package, Plus, RefreshCw, Settings2, Skull, Terminal, Wand } from "lucide-react";
import { useState } from "react";
import { RunnerSelect, runnerLabel } from "../../components/pickers";
import { Button } from "../../components/ui/Button";
import { Badge, Callout, EmptyState, ErrorState, LoadingBlock, Section } from "../../components/ui/feedback";
import { PathField, Segmented, TextField } from "../../components/ui/form";
import { ConfirmDialog, Modal } from "../../components/ui/Modal";
import type { PrefixArch, PrefixInfo, PrefixTool, Runner, RunnerRef } from "../../contract";
import { useI18n } from "../../i18n";
import { call } from "../../lib/api";
import { openInFileManager } from "../../lib/dialogs";
import { formatDate } from "../../lib/format";
import { useCommand } from "../../lib/hooks";
import { useAppSettings, useLibrary } from "../../state/app";
import { useToasts } from "../../state/toasts";

export function RuntimePage() {
  const { t } = useI18n();
  const runners = useCommand("runtime_list_runners", {});
  const prefixes = useCommand("runtime_list_prefixes", {});
  return (
    <div className="page">
      <header className="page-head">
        <div>
          <h1>{t("runtime.title")}</h1>
          <p className="page-sub">{t("runtime.subtitle")}</p>
        </div>
      </header>
      <RunnersSection runners={runners} />
      <PrefixesSection prefixes={prefixes} runners={runners.data} />
    </div>
  );
}

function RunnersSection({ runners }: { runners: ReturnType<typeof useCommand<"runtime_list_runners">> }) {
  const { t } = useI18n();
  const toasts = useToasts();
  const { appSettings } = useAppSettings();
  const [refreshing, setRefreshing] = useState(false);
  const [addOpen, setAddOpen] = useState(false);
  const [path, setPath] = useState("");
  const [name, setName] = useState("");
  const [adding, setAdding] = useState(false);

  const refresh = async () => {
    setRefreshing(true);
    try {
      runners.mutate(await call("runtime_refresh_runners", {}));
      toasts.success(t("runtime.refreshed"));
    } catch (e) {
      toasts.error(e, t("runtime.refreshFailed"));
    } finally {
      setRefreshing(false);
    }
  };

  const add = async () => {
    setAdding(true);
    try {
      const r = await call("runtime_add_custom_runner", { path: path.trim(), name: name.trim() || null });
      runners.mutate((list) => [...(list ?? []), r]);
      setAddOpen(false);
      setPath("");
      setName("");
      toasts.success(t("runtime.runnerAdded", { name: r.name }));
    } catch (e) {
      toasts.error(e, t("runtime.addFailed"));
    } finally {
      setAdding(false);
    }
  };

  return (
    <Section
      title={t("runtime.runners")}
      description={t("runtime.runnersDesc")}
      actions={
        <div className="row gap-sm">
          <Button size="sm" variant="ghost" icon={RefreshCw} onClick={() => void refresh()} loading={refreshing}>
            {t("runtime.rescan")}
          </Button>
          <Button size="sm" icon={Plus} onClick={() => setAddOpen(true)}>
            {t("runtime.addCustom")}
          </Button>
        </div>
      }
    >
      {runners.error && <ErrorState error={runners.error} onRetry={() => void runners.reload()} />}
      {runners.loading && <LoadingBlock />}
      {runners.data && runners.data.length === 0 && (
        <EmptyState icon={Package} title={t("runtime.noRunners")}>
          {t("runtime.noRunnersHint")}
        </EmptyState>
      )}
      {runners.data && runners.data.length > 0 && (
        <table className="table">
          <thead>
            <tr>
              <th>{t("runtime.name")}</th>
              <th>{t("runtime.kind")}</th>
              <th>{t("runtime.source")}</th>
              <th>{t("runtime.path")}</th>
            </tr>
          </thead>
          <tbody>
            {runners.data.map((r) => (
              <tr key={r.id}>
                <td>
                  <strong>{runnerLabel(r)}</strong> {appSettings?.defaultRunner?.id === r.id && <Badge tone="accent">{t("runtime.default")}</Badge>}
                </td>
                <td>
                  <Badge tone={r.kind === "proton" ? "accent" : r.kind === "wine" ? "rose" : "neutral"}>{t(`runner.kind.${r.kind}`)}</Badge>
                </td>
                <td>{t(`runner.source.${r.source}`)}</td>
                <td>
                  <code className="small break">{r.path}</code>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <Modal
        open={addOpen}
        onClose={() => setAddOpen(false)}
        title={t("runtime.addCustom")}
        footer={
          <>
            <Button variant="ghost" onClick={() => setAddOpen(false)}>
              {t("common.cancel")}
            </Button>
            <Button variant="primary" onClick={() => void add()} loading={adding} disabled={!path.trim()}>
              {t("common.add")}
            </Button>
          </>
        }
      >
        <p className="muted">{t("runtime.addCustomHint")}</p>
        <PathField label={t("runtime.path")} value={path} onChange={setPath} kind="file" demoDefault="/opt/wine-staging/bin/wine" />
        <TextField label={t("runtime.nameOptional")} value={name} onChange={setName} />
      </Modal>
    </Section>
  );
}

function PrefixesSection({ prefixes, runners }: { prefixes: ReturnType<typeof useCommand<"runtime_list_prefixes">>; runners: Runner[] | undefined }) {
  const { t } = useI18n();
  const toasts = useToasts();
  const [createOpen, setCreateOpen] = useState(false);
  const [registerOpen, setRegisterOpen] = useState(false);
  const [newName, setNewName] = useState("");
  const [arch, setArch] = useState<PrefixArch>("win64");
  const [newRunner, setNewRunner] = useState<RunnerRef | null>(null);
  const [regName, setRegName] = useState("");
  const [regPath, setRegPath] = useState("");
  const [busy, setBusy] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const wineRunners = runners?.filter((r) => r.kind !== "native");

  const create = async () => {
    if (!newRunner) return;
    setBusy(true);
    try {
      const p = await call("runtime_create_prefix", { request: { name: newName.trim(), arch, runnerId: newRunner.id } });
      prefixes.mutate((list) => [...(list ?? []), p]);
      setCreateOpen(false);
      setNewName("");
      toasts.success(t("prefix.created", { name: p.name }));
    } catch (e) {
      toasts.error(e, t("prefix.createFailed"));
    } finally {
      setBusy(false);
    }
  };

  const register = async () => {
    setBusy(true);
    try {
      const p = await call("runtime_register_prefix", { request: { name: regName.trim(), path: regPath.trim() } });
      prefixes.mutate((list) => [...(list ?? []), p]);
      setRegisterOpen(false);
      setRegName("");
      setRegPath("");
      toasts.success(t("prefix.registered", { name: p.name }));
    } catch (e) {
      toasts.error(e, t("prefix.registerFailed"));
    } finally {
      setBusy(false);
    }
  };

  const selected = prefixes.data?.find((p) => p.id === selectedId) ?? null;

  return (
    <Section
      title={t("prefix.title")}
      description={t("prefix.desc")}
      actions={
        <div className="row gap-sm">
          <Button size="sm" variant="ghost" icon={FolderInput} onClick={() => setRegisterOpen(true)}>
            {t("prefix.registerExisting")}
          </Button>
          <Button size="sm" icon={Plus} onClick={() => setCreateOpen(true)}>
            {t("prefix.create")}
          </Button>
        </div>
      }
    >
      {prefixes.error && <ErrorState error={prefixes.error} onRetry={() => void prefixes.reload()} />}
      {prefixes.loading && <LoadingBlock />}
      {prefixes.data && prefixes.data.length === 0 && <EmptyState icon={Wand} title={t("prefix.none")}>{t("prefix.noneHint")}</EmptyState>}
      {prefixes.data && prefixes.data.length > 0 && (
        <div className="prefix-grid">
          {prefixes.data.map((p) => (
            <PrefixCard key={p.id} prefix={p} runners={wineRunners} selected={p.id === selectedId} onSelect={() => setSelectedId(p.id === selectedId ? null : p.id)} />
          ))}
        </div>
      )}
      {selected && <PrefixComponents prefix={selected} runners={wineRunners} />}

      <Modal
        open={createOpen}
        onClose={() => setCreateOpen(false)}
        title={t("prefix.create")}
        locked={busy}
        footer={
          <>
            <Button variant="ghost" onClick={() => setCreateOpen(false)} disabled={busy}>
              {t("common.cancel")}
            </Button>
            <Button variant="primary" onClick={() => void create()} loading={busy} disabled={!newName.trim() || !newRunner}>
              {t("prefix.create")}
            </Button>
          </>
        }
      >
        <TextField label={t("runtime.name")} value={newName} onChange={setNewName} placeholder="Shared JP" autoFocus />
        <Segmented<PrefixArch> label={t("prefix.arch")} value={arch} onChange={setArch} options={[{ value: "win64", label: "64-bit (win64)" }, { value: "win32", label: "32-bit (win32)" }]} />
        <p className="field-hint">{t("prefix.archHint")}</p>
        <RunnerSelect label={t("compat.runner")} value={newRunner} onChange={setNewRunner} runners={wineRunners} allowDefault={false} />
      </Modal>

      <Modal
        open={registerOpen}
        onClose={() => setRegisterOpen(false)}
        title={t("prefix.registerExisting")}
        locked={busy}
        footer={
          <>
            <Button variant="ghost" onClick={() => setRegisterOpen(false)} disabled={busy}>
              {t("common.cancel")}
            </Button>
            <Button variant="primary" onClick={() => void register()} loading={busy} disabled={!regPath.trim()}>
              {t("prefix.register")}
            </Button>
          </>
        }
      >
        <Callout tone="info">{t("prefix.registerSafe")}</Callout>
        <PathField label={t("runtime.path")} kind="directory" value={regPath} onChange={setRegPath} demoDefault="/home/demo/Games/wine-prefixes/old" />
        <TextField label={t("runtime.nameOptional")} value={regName} onChange={setRegName} />
      </Modal>
    </Section>
  );
}

function PrefixCard({ prefix, runners, selected, onSelect }: { prefix: PrefixInfo; runners: Runner[] | undefined; selected: boolean; onSelect: () => void }) {
  const { t, lang } = useI18n();
  const toasts = useToasts();
  const library = useLibrary();
  const { appSettings } = useAppSettings();
  const [runner, setRunner] = useState<RunnerRef | null>(prefix.runnerId ? { id: prefix.runnerId } : null);
  const [verbs, setVerbs] = useState("");
  const [busy, setBusy] = useState<PrefixTool | null>(null);
  const [killOpen, setKillOpen] = useState(false);
  const runnerId = runner?.id ?? prefix.runnerId ?? appSettings?.defaultRunner?.id ?? null;

  const tool = async (tool: PrefixTool, v?: string[]) => {
    if (!runnerId) {
      toasts.push({ kind: "warning", title: t("prefix.pickRunner") });
      return;
    }
    setBusy(tool);
    try {
      await call("runtime_prefix_tool", { request: { prefixId: prefix.id, runnerId, tool, verbs: v } });
      toasts.success(t("prefix.toolStarted", { tool: t(`prefix.tool.${tool}`) }));
    } catch (e) {
      toasts.error(e, t("prefix.toolFailed", { tool: t(`prefix.tool.${tool}`) }));
    } finally {
      setBusy(null);
      setKillOpen(false);
    }
  };

  const owner = prefix.kind.type === "dedicated" ? library.byId(prefix.kind.gameId) : undefined;

  return (
    <article className={selected ? "prefix-card is-selected" : "prefix-card"} aria-label={prefix.name}>
      <header className="prefix-head">
        <div>
          <h3>{prefix.name}</h3>
          <code className="small break">{prefix.path}</code>
        </div>
        <div className="row gap-xs wrap">
          <Badge>{prefix.arch}</Badge>
          <Badge tone={prefix.kind.type === "dedicated" ? "accent" : prefix.kind.type === "shared" ? "info" : "neutral"}>{t(`prefix.kind.${prefix.kind.type}`)}</Badge>
          {!prefix.managed && <Badge tone="warning">{t("prefix.external")}</Badge>}
          {!prefix.initialized && <Badge tone="warning">{t("prefix.notInitialized")}</Badge>}
        </div>
      </header>
      <p className="muted small">
        {owner ? t("prefix.usedBy", { title: owner.title }) : null} {t("prefix.createdAt", { date: formatDate(prefix.createdAt, lang) })}
      </p>
      <RunnerSelect label={t("prefix.toolRunner")} value={runner} onChange={setRunner} runners={runners} defaultRunnerId={prefix.runnerId ?? appSettings?.defaultRunner?.id} />
      <div className="row gap-xs wrap">
        <Button size="sm" icon={Settings2} onClick={() => void tool("winecfg")} loading={busy === "winecfg"}>
          winecfg
        </Button>
        <Button size="sm" icon={Terminal} onClick={() => void tool("regedit")} loading={busy === "regedit"}>
          regedit
        </Button>
        <Button size="sm" icon={FolderOpen} onClick={() => void tool("explorer")} loading={busy === "explorer"}>
          {t("prefix.tool.explorer")}
        </Button>
        <Button size="sm" variant="ghost" icon={FolderOpen} onClick={() => openInFileManager(prefix.path).then((ok) => !ok && toasts.info(t("demo.noFileManager"), prefix.path)).catch((e: unknown) => toasts.error(e))}>
          {t("prefix.openFolder")}
        </Button>
        <Button size="sm" variant="danger" icon={Skull} onClick={() => setKillOpen(true)}>
          {t("prefix.tool.killAll")}
        </Button>
      </div>
      <form
        className="row gap-sm align-end"
        onSubmit={(e) => {
          e.preventDefault();
          const v = verbs.split(/[\s,]+/).filter(Boolean);
          if (v.length) void tool("winetricks", v);
        }}
      >
        <div className="grow">
          <TextField label={t("prefix.winetricksVerbs")} value={verbs} onChange={setVerbs} placeholder="cjkfonts quartz" mono />
        </div>
        <Button type="submit" size="md" icon={Wand} loading={busy === "winetricks"} disabled={!verbs.trim()}>
          {t("prefix.runWinetricks")}
        </Button>
      </form>
      <Button size="sm" variant="ghost" onClick={onSelect} aria-expanded={selected}>
        {selected ? t("prefix.hideComponents") : t("prefix.showComponents")}
      </Button>
      <ConfirmDialog open={killOpen} title={t("prefix.killTitle")} confirmLabel={t("prefix.tool.killAll")} variant="danger" busy={busy === "killAll"} onCancel={() => setKillOpen(false)} onConfirm={() => void tool("killAll")}>
        <p>{t("prefix.killText")}</p>
      </ConfirmDialog>
    </article>
  );
}

function PrefixComponents({ prefix, runners }: { prefix: PrefixInfo; runners: Runner[] | undefined }) {
  const { t } = useI18n();
  const toasts = useToasts();
  const { appSettings } = useAppSettings();
  const comps = useCommand("diagnostics_prefix_components", { prefixPath: prefix.path });
  const [installing, setInstalling] = useState<string | null>(null);
  const runnerId = prefix.runnerId ?? appSettings?.defaultRunner?.id ?? runners?.[0]?.id ?? null;

  const install = async (id: string, verbs: string[]) => {
    if (!runnerId) {
      toasts.push({ kind: "warning", title: t("prefix.pickRunner") });
      return;
    }
    setInstalling(id);
    try {
      await call("runtime_prefix_tool", { request: { prefixId: prefix.id, runnerId, tool: "winetricks", verbs } });
      toasts.success(t("prefix.installStarted", { verbs: verbs.join(" ") }));
    } catch (e) {
      toasts.error(e, t("prefix.toolFailed", { tool: "winetricks" }));
    } finally {
      setInstalling(null);
    }
  };

  return (
    <div className="subsection">
      <div className="row space-between">
        <h3>{t("prefix.components", { name: prefix.name })}</h3>
        <Button size="sm" variant="ghost" icon={RefreshCw} onClick={() => void comps.reload()} loading={comps.refreshing}>
          {t("common.refresh")}
        </Button>
      </div>
      {comps.error && <ErrorState error={comps.error} compact onRetry={() => void comps.reload()} />}
      {comps.loading && <LoadingBlock />}
      {comps.data && (
        <table className="table">
          <thead>
            <tr>
              <th>{t("runtime.name")}</th>
              <th>{t("prefix.category")}</th>
              <th>{t("prefix.status")}</th>
              <th className="sr-only">{t("prefix.install")}</th>
            </tr>
          </thead>
          <tbody>
            {comps.data.map((c) => (
              <tr key={c.id}>
                <td>
                  <strong>{c.name}</strong>
                  <div className="muted small">{c.description}</div>
                </td>
                <td>{c.category}</td>
                <td>{c.installed === true ? <Badge tone="success">{t("prefix.installed")}</Badge> : c.installed === false ? <Badge tone="warning">{t("prefix.missing")}</Badge> : <Badge>{t("prefix.unknown")}</Badge>}</td>
                <td className="num">
                  {c.installed !== true && c.winetricksVerbs.length > 0 && (
                    <Button size="sm" icon={Wand} loading={installing === c.id} onClick={() => void install(c.id, c.winetricksVerbs)} title={`winetricks ${c.winetricksVerbs.join(" ")}`}>
                      {t("prefix.installWinetricks")}
                    </Button>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
