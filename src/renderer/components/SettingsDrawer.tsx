import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { X, Download, AlertTriangle, Copy, RefreshCw, Check, Eye, EyeOff, Info } from 'lucide-react';
import type { NodeInstallation } from '@shared/types';
import type { McpStatus, SettingsState } from '@shared/ipc';
import { ErrorBanner } from './ErrorBanner';
import { openConfirm } from './PromptModal';
import { useDialog } from '../hooks/useDialog';
import { cn } from '../lib/cn';

/** id linking the dialog panel (aria-labelledby) to its visible heading for screen readers. */
const SETTINGS_TITLE_ID = 'settings-drawer-title';

export function SettingsDrawer({ onClose }: { onClose: () => void }): JSX.Element {
  const { t } = useTranslation();
  // Focus trap / Escape-to-close / focus-restore + role=dialog/aria-modal/aria-labelledby.
  const { dialogProps } = useDialog(onClose, SETTINGS_TITLE_ID);
  const [settings, setSettings] = useState<SettingsState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [nodeInstalls, setNodeInstalls] = useState<NodeInstallation[]>([]);
  const [dbPath, setDbPath] = useState<string>('');
  const [exported, setExported] = useState<string | null>(null);

  useEffect(() => {
    void window.api.invoke('settings:get', undefined).then(setSettings);
    void window.api.invoke('node:list', undefined).then(setNodeInstalls);
    void window.api.invoke('db:path', undefined).then(setDbPath);
  }, []);

  const update = async (patch: Partial<SettingsState>): Promise<void> => {
    setError(null);
    try {
      const next = await window.api.invoke('settings:set', { patch });
      setSettings(next);
      // Notify other parts of the renderer that may be observing settings (e.g. useTheme).
      window.dispatchEvent(new CustomEvent('settings-changed'));
    } catch (e) {
      setError((e as Error).message);
    }
  };

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/60 p-6">
      <div
        {...dialogProps}
        className="flex h-full max-h-[680px] w-full max-w-2xl flex-col overflow-hidden rounded-lg border border-border bg-base shadow-2xl"
      >
        <header className="flex items-center justify-between border-b border-border px-4 py-3">
          <h2 id={SETTINGS_TITLE_ID} className="text-sm font-medium text-fg">
            {t('settings.title')}
          </h2>
          <button
            onClick={onClose}
            className="rounded-md p-1 text-fg-subtle hover:bg-surface hover:text-fg"
            title={t('settings.closeTooltip')}
            aria-label={t('common.close')}
          >
            <X className="h-4 w-4" />
          </button>
        </header>

        {error && (
          <ErrorBanner
            message={error}
            onDismiss={() => setError(null)}
            className="m-3 rounded-md"
          />
        )}

        <div className="flex-1 overflow-y-auto p-5">
          {!settings ? (
            <div className="text-sm text-fg-subtle">{t('common.loading')}</div>
          ) : (
            <div className="space-y-6">
              <Section title={t('settings.general')}>
                <Field label={t('settings.theme')} description={t('settings.themeDesc')}>
                  <select
                    data-autofocus
                    value={settings.theme}
                    onChange={(e) =>
                      void update({ theme: e.target.value as SettingsState['theme'] })
                    }
                    className="rounded-md border border-border bg-surface px-2 py-1 text-sm text-fg"
                  >
                    <option value="system">{t('settings.themeSystem')}</option>
                    <option value="dark">{t('settings.themeDark')}</option>
                    <option value="light">{t('settings.themeLight')}</option>
                  </select>
                </Field>
                <Field label={t('settings.language')} description={t('settings.languageDesc')}>
                  <select
                    value={settings.language}
                    onChange={(e) =>
                      void update({ language: e.target.value as SettingsState['language'] })
                    }
                    className="rounded-md border border-border bg-surface px-2 py-1 text-sm text-fg"
                  >
                    <option value="system">{t('settings.languageSystem')}</option>
                    {/* Language names stay in their own language - that is how users find
                        the one they can read. */}
                    <option value="en">English</option>
                    <option value="zh-CN">简体中文</option>
                  </select>
                </Field>
                <Toggle
                  label={t('settings.autoUpdate')}
                  description={t('settings.autoUpdateDesc')}
                  checked={settings.auto_update}
                  onChange={(v) => void update({ auto_update: v })}
                />
                <Toggle
                  label={t('settings.launchAtLogin')}
                  description={t('settings.launchAtLoginDesc')}
                  checked={settings.launch_at_login}
                  onChange={(v) => void update({ launch_at_login: v })}
                />
                <Toggle
                  label={t('settings.trayEnabled')}
                  description={t('settings.trayEnabledDesc')}
                  checked={settings.tray_enabled}
                  onChange={(v) => void update({ tray_enabled: v })}
                />
              </Section>

              <Section title={t('settings.notifications')}>
                <Toggle
                  label={t('settings.notifyCrash')}
                  description={t('settings.notifyCrashDesc')}
                  checked={settings.notify_on_crash}
                  onChange={(v) => void update({ notify_on_crash: v })}
                />
                <Toggle
                  label={t('settings.notifyReady')}
                  description={t('settings.notifyReadyDesc')}
                  checked={settings.notify_on_ready}
                  onChange={(v) => void update({ notify_on_ready: v })}
                />
              </Section>

              <Section title={t('settings.dashboard')}>
                <Field label={t('settings.refreshLabel')} description={t('settings.refreshDesc')}>
                  <NumberInput
                    value={settings.dashboard_refresh_ms}
                    onCommit={(v) => void update({ dashboard_refresh_ms: clamp(v, 200, 10000) })}
                    suffix={t('settings.unitMs')}
                    step={100}
                    min={200}
                    max={10000}
                  />
                </Field>
              </Section>

              <Section title={t('settings.logs')}>
                <Field label={t('settings.ringLabel')} description={t('settings.ringDesc')}>
                  <NumberInput
                    value={settings.log_ring_size}
                    onCommit={(v) => void update({ log_ring_size: clamp(v, 100, 100000) })}
                    suffix={t('settings.unitLines')}
                    step={1000}
                    min={100}
                    max={100000}
                  />
                </Field>
                <Field label={t('settings.historyLabel')} description={t('settings.historyDesc')}>
                  <NumberInput
                    value={settings.run_history_limit}
                    onCommit={(v) => void update({ run_history_limit: clamp(v, 10, 10000) })}
                    suffix={t('settings.unitRuns')}
                    step={50}
                    min={10}
                    max={10000}
                  />
                </Field>
              </Section>

              <Section title={t('settings.processes')}>
                <Field label={t('settings.killGraceLabel')} description={t('settings.killGraceDesc')}>
                  <NumberInput
                    value={settings.kill_grace_ms}
                    onCommit={(v) => void update({ kill_grace_ms: clamp(v, 100, 30000) })}
                    suffix={t('settings.unitMs')}
                    step={500}
                    min={100}
                    max={30000}
                  />
                </Field>
                <Field
                  label={t('settings.readinessLabel')}
                  description={t('settings.readinessDesc')}
                >
                  <NumberInput
                    value={settings.readiness_timeout_ms}
                    onCommit={(v) => void update({ readiness_timeout_ms: clamp(v, 1000, 600000) })}
                    suffix={t('settings.unitMs')}
                    step={1000}
                    min={1000}
                    max={600000}
                  />
                </Field>
              </Section>

              <Section title={t('settings.node')}>
                <div className="text-[11px] text-fg-subtle">{t('settings.nodeDesc')}</div>
                <div className="rounded-md border border-border bg-base/40 p-3">
                  {nodeInstalls.length === 0 ? (
                    <div className="text-xs text-fg-subtle">{t('settings.nodeNone')}</div>
                  ) : (
                    <ul className="space-y-0.5 font-mono text-[11px]">
                      {nodeInstalls.map((n) => (
                        <li key={`${n.source}-${n.version}`} className="flex items-center gap-2">
                          <span className="inline-flex w-10 text-fg-subtle">{n.source}</span>
                          <span className="font-medium text-fg">v{n.version}</span>
                          <span className="truncate text-fg-subtle">{n.binDir}</span>
                        </li>
                      ))}
                    </ul>
                  )}
                </div>
              </Section>

              <McpSection settings={settings} update={update} />

              <Section title={t('settings.danger')}>
                <div className="text-[11px] text-fg-subtle">
                  {t('settings.dbPath')} <span className="font-mono">{dbPath}</span>
                </div>
                {exported && (
                  <div className="rounded-md border border-success-border bg-success-bg px-2 py-1 text-[11px] text-success-fg">
                    {t('settings.exportedTo')} <span className="font-mono">{exported}</span>
                  </div>
                )}
                <Field label={t('settings.exportLabel')} description={t('settings.exportDesc')}>
                  <button
                    onClick={async () => {
                      setError(null);
                      try {
                        const p = await window.api.invoke('db:export', undefined);
                        if (p) setExported(p);
                      } catch (e) {
                        setError((e as Error).message);
                      }
                    }}
                    className="inline-flex items-center gap-1.5 rounded-md border border-border bg-surface px-2 py-1 text-sm text-fg hover:bg-elevated"
                  >
                    <Download className="h-3.5 w-3.5" /> {t('settings.exportButton')}
                  </button>
                </Field>
                <Field label={t('settings.resetLabel')} description={t('settings.resetDesc')}>
                  <button
                    onClick={async () => {
                      const ok = await openConfirm({
                        title: t('settings.resetConfirmTitle'),
                        description: t('settings.resetConfirmDesc'),
                        confirmLabel: t('settings.resetConfirm'),
                        danger: true
                      });
                      if (!ok) return;
                      try {
                        await window.api.invoke('db:reset', undefined);
                      } catch (e) {
                        setError((e as Error).message);
                      }
                    }}
                    className="inline-flex items-center gap-1.5 rounded-md border border-danger-border bg-danger-bg px-2 py-1 text-sm text-danger-fg hover:bg-danger-bg-hover"
                  >
                    <AlertTriangle className="h-3.5 w-3.5" /> {t('settings.resetButton')}
                  </button>
                </Field>
              </Section>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }): JSX.Element {
  return (
    <section>
      <h3 className="mb-2 text-[11px] uppercase tracking-wider text-fg-subtle">{title}</h3>
      <div className="space-y-4 rounded-md border border-border bg-base/50 p-4">
        {children}
      </div>
    </section>
  );
}

function Field({
  label,
  description,
  children
}: {
  label: string;
  description?: string;
  children: React.ReactNode;
}): JSX.Element {
  return (
    <div className="flex items-start justify-between gap-4">
      <div className="flex-1">
        <div className="text-sm text-fg">{label}</div>
        {description && <div className="mt-0.5 text-[11px] text-fg-subtle">{description}</div>}
      </div>
      <div>{children}</div>
    </div>
  );
}

function Toggle({
  label,
  description,
  checked,
  onChange
}: {
  label: string;
  description?: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}): JSX.Element {
  return (
    <Field label={label} description={description}>
      <button
        onClick={() => onChange(!checked)}
        className={cn(
          'inline-flex h-5 w-9 items-center rounded-full transition-colors',
          checked ? 'bg-accent' : 'bg-elevated'
        )}
        aria-pressed={checked}
      >
        <span
          className={cn(
            'inline-block h-4 w-4 rounded-full bg-white transition-transform',
            checked ? 'translate-x-4' : 'translate-x-0.5'
          )}
        />
      </button>
    </Field>
  );
}

function NumberInput({
  value,
  onCommit,
  suffix,
  step = 1,
  min,
  max
}: {
  value: number;
  onCommit: (v: number) => void;
  suffix?: string;
  step?: number;
  min?: number;
  max?: number;
}): JSX.Element {
  const [local, setLocal] = useState(String(value));
  useEffect(() => setLocal(String(value)), [value]);
  return (
    <div className="inline-flex items-center gap-1">
      <input
        type="number"
        value={local}
        step={step}
        min={min}
        max={max}
        onChange={(e) => setLocal(e.target.value)}
        onBlur={() => {
          const n = Number(local);
          if (!Number.isNaN(n) && n !== value) onCommit(n);
        }}
        onKeyDown={(e) => {
          if (e.key === 'Enter') (e.target as HTMLInputElement).blur();
        }}
        className="w-24 rounded-md border border-border bg-surface px-2 py-1 text-right text-sm text-fg"
      />
      {suffix && <span className="text-[11px] text-fg-subtle">{suffix}</span>}
    </div>
  );
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, v));
}

/** A small copy-to-clipboard button that flips to a check for a moment after copying. */
function CopyButton({ text, label }: { text: string; label?: string }): JSX.Element {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  return (
    <button
      onClick={() => {
        void navigator.clipboard.writeText(text).then(() => {
          setCopied(true);
          setTimeout(() => setCopied(false), 1200);
        });
      }}
      className="inline-flex items-center gap-1.5 rounded-md border border-border bg-surface px-2 py-1 text-sm text-fg hover:bg-elevated"
    >
      {copied ? <Check className="h-3.5 w-3.5 text-success-fg" /> : <Copy className="h-3.5 w-3.5" />}
      {copied ? t('common.copied') : (label ?? t('common.copy'))}
    </button>
  );
}

/**
 * MCP server controls: enable toggle, live status, port, auth token, and copy-paste client
 * config. Status arrives via the `mcp:statusChanged` event so a port-in-use error (or a
 * successful start) shows without a manual refresh.
 */
function McpSection({
  settings,
  update
}: {
  settings: SettingsState;
  update: (patch: Partial<SettingsState>) => Promise<void>;
}): JSX.Element {
  const { t } = useTranslation();
  const [status, setStatus] = useState<McpStatus | null>(null);
  const [token, setToken] = useState<string | null>(null);
  const [reveal, setReveal] = useState(false);

  const refreshToken = (): void => {
    void window.api.invoke('mcp:token', undefined).then(setToken);
  };

  useEffect(() => {
    void window.api.invoke('mcp:status', undefined).then(setStatus);
    if (settings.mcp_require_auth) refreshToken();
    const off = window.api.on('mcp:statusChanged', (s) => setStatus(s));
    return off;
    // Re-subscribe when auth flips so the token field appears/updates.
  }, [settings.mcp_require_auth]);

  const url =
    status?.url ?? `${settings.mcp_https ? 'https' : 'http'}://127.0.0.1:${settings.mcp_port}/mcp`;
  const clientConfig = JSON.stringify(
    {
      mcpServers: {
        devharbor: {
          type: 'http',
          url,
          ...(settings.mcp_require_auth
            ? { headers: { Authorization: `Bearer ${token ?? '<token>'}` } }
            : {})
        }
      }
    },
    null,
    2
  );

  return (
    <Section title={t('settings.mcp')}>
      <div className="text-[11px] text-fg-subtle">{t('settings.mcpIntro')}</div>

      <Toggle
        label={t('settings.mcpEnable')}
        description={t('settings.mcpEnableDesc')}
        checked={settings.mcp_enabled}
        onChange={(v) => void update({ mcp_enabled: v })}
      />

      {settings.mcp_enabled && (
        <>
          <Field label={t('settings.mcpStatus')} description={t('settings.mcpStatusDesc')}>
            <div className="flex items-center gap-2 text-sm">
              <span
                className={cn(
                  'inline-block h-2 w-2 rounded-full',
                  status?.running ? 'bg-success-fg' : status?.lastError ? 'bg-danger-fg' : 'bg-elevated'
                )}
              />
              {status?.running ? (
                <span className="font-mono text-[11px] text-fg">{status.url}</span>
              ) : status?.lastError ? (
                <span className="text-[11px] text-danger-fg">{status.lastError}</span>
              ) : (
                <span className="text-[11px] text-fg-subtle">{t('settings.mcpStopped')}</span>
              )}
            </div>
          </Field>

          <Field label={t('settings.mcpPort')} description={t('settings.mcpPortDesc')}>
            <NumberInput
              value={settings.mcp_port}
              onCommit={(v) => void update({ mcp_port: clamp(Math.round(v), 1024, 65535) })}
              step={1}
              min={1024}
              max={65535}
            />
          </Field>

          <Toggle
            label={t('settings.mcpHttps')}
            description={t('settings.mcpHttpsDesc')}
            checked={settings.mcp_https}
            onChange={(v) => void update({ mcp_https: v })}
          />

          {settings.mcp_https && status?.certPath && (
            <Field label={t('settings.mcpCert')} description={t('settings.mcpCertDesc')}>
              <div className="flex items-center gap-1.5">
                <span
                  className="max-w-56 truncate font-mono text-[10.5px] text-fg-subtle"
                  title={status.certPath}
                >
                  {status.certPath}
                </span>
                <CopyButton text={status.certPath} label={t('settings.mcpCopyPath')} />
              </div>
            </Field>
          )}

          <Toggle
            label={t('settings.mcpRequireAuth')}
            description={t('settings.mcpRequireAuthDesc')}
            checked={settings.mcp_require_auth}
            onChange={(v) => void update({ mcp_require_auth: v })}
          />

          {!settings.mcp_require_auth && (
            <div className="flex items-start gap-2 rounded-md border border-border bg-surface px-2 py-1.5 text-[11px] text-fg-subtle">
              <Info className="mt-0.5 h-3.5 w-3.5 shrink-0 text-fg-subtle" />
              <span>{t('settings.mcpNoAuthWarning')}</span>
            </div>
          )}

          {settings.mcp_require_auth && (
            <Field label={t('settings.mcpToken')} description={t('settings.mcpTokenDesc')}>
              <div className="flex items-center gap-1.5">
                <input
                  readOnly
                  value={reveal ? token ?? '' : token ? '•'.repeat(24) : ''}
                  className="w-40 rounded-md border border-border bg-surface px-2 py-1 font-mono text-[11px] text-fg"
                />
                <button
                  onClick={() => setReveal((r) => !r)}
                  title={reveal ? t('settings.mcpTokenHide') : t('settings.mcpTokenReveal')}
                  className="rounded-md border border-border bg-surface p-1.5 text-fg hover:bg-elevated"
                >
                  {reveal ? <EyeOff className="h-3.5 w-3.5" /> : <Eye className="h-3.5 w-3.5" />}
                </button>
                {token && <CopyButton text={token} />}
                <button
                  onClick={async () => {
                    const ok = await openConfirm({
                      title: t('settings.mcpTokenRegenerateTitle'),
                      description: t('settings.mcpTokenRegenerateDesc'),
                      confirmLabel: t('settings.mcpTokenRegenerate'),
                      danger: true
                    });
                    if (!ok) return;
                    const next = await window.api.invoke('mcp:regenerateToken', undefined);
                    setToken(next);
                  }}
                  title={t('settings.mcpTokenRegenerateTooltip')}
                  className="rounded-md border border-border bg-surface p-1.5 text-fg hover:bg-elevated"
                >
                  <RefreshCw className="h-3.5 w-3.5" />
                </button>
              </div>
            </Field>
          )}

          {status?.tokenStoredPlaintext && (
            <div className="rounded-md border border-danger-border bg-danger-bg px-2 py-1 text-[11px] text-danger-fg">
              {t('settings.mcpPlaintextWarning')}
            </div>
          )}

          <div>
            <div className="mb-1 flex items-center justify-between">
              <span className="text-sm text-fg">{t('settings.mcpClientConfig')}</span>
              <CopyButton text={clientConfig} label={t('settings.mcpCopyConfig')} />
            </div>
            <pre className="max-h-40 overflow-auto rounded-md border border-border bg-base/60 p-2 font-mono text-[10.5px] leading-relaxed text-fg-subtle">
              {clientConfig}
            </pre>
          </div>
        </>
      )}
    </Section>
  );
}
